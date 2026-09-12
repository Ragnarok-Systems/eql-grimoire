//! THE DRIVER: one field on `App`, a thread per sign-in, and a view the Settings screen draws.
//!
//! THE SHAPE `twitch_auth::Auth` AND `updater::run::Updater` ALREADY HAVE. The UI thread never
//! makes a request: `ask` claims the state under a lock, spawns, and returns. The thread writes a
//! new [`AccountView`] and wakes the root viewport. The screen clones the view once a frame and
//! draws it; it cannot reach the token.
//!
//! GENERATIONS AND NOT A RUNNING LATCH. `twitch_auth` learned that a `running` flag nothing clears
//! makes Try again a dead button. Here every sign-in, check and sign-out takes a new generation
//! under the lock, and a thread may only write if its generation is still current. So Cancel takes
//! effect on screen at once, a new Sign in can start while the cancelled thread is still winding
//! down, and a thread that finishes late cannot overwrite anything: its token is revoked instead of
//! kept.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::{
    app_url, exchange, forget_token, load_token, me, provider_label, save_token, AppToken, Https,
    Loopback, Pkce, Profile, Refused, Waited, Wire, WAIT_FOR_BROWSER,
};

/// Builds a wire for one thread. `Https` in production, a script in tests.
pub type WireMaker = Arc<dyn Fn() -> Box<dyn Wire> + Send + Sync>;
/// Opens a URL in the system browser. `open` in production, a test client in tests.
pub type Browser = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// Everything a driver needs from outside. A test hands in a scratch path, a scripted wire, a
/// browser that is a socket client, and a short wait; nothing of the owner's is touched.
pub struct Config {
    pub base: String,
    /// Where the sealed token lives. `None` keeps it in memory only.
    pub path: Option<PathBuf>,
    pub wire: WireMaker,
    pub browser: Browser,
    /// How long a sign-in waits for the browser. [`WAIT_FOR_BROWSER`] in production.
    pub wait: Duration,
}

/// What the Settings screen draws. It carries no token: a view is drawn, and a drawn token is a
/// leaked token.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AccountView {
    /// Nobody is signed in.
    #[default]
    Out,
    /// A saved sign-in is being checked with the website.
    Checking,
    /// The browser is open and the app is waiting for it to come back.
    Waiting,
    /// Signed in, as `name`, with these sign-in methods, already in words ("Twitch (broken_stoic)").
    In {
        name: String,
        providers: Vec<String>,
    },
    /// It did not work, in a sentence. Nothing is held.
    Failed(String),
    /// A saved sign-in is held, but the website could not say whether it still works: no answer at
    /// all, or one that is neither yes (200) nor no (401). In a sentence. It can be checked again,
    /// or signed out.
    Unchecked(String),
}

/// What the Settings screen can ask for. An ask and not a call, exactly like
/// `settings::Changed::updater`: the screen has no `Account`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Sign in, or Try again.
    SignIn,
    /// Stop waiting for the browser.
    Cancel,
    SignOut,
    /// Open the account page in the browser.
    Manage,
}

/// How a sign-in ended, other than signed in.
#[derive(Debug)]
pub enum Ended {
    Cancelled,
    /// The owner said no on the website (`access_denied`).
    Denied,
    TimedOut,
    /// The website did not accept the code (`invalid_grant`).
    InvalidGrant,
    Failed(String),
}

/// ONE WHOLE SIGN-IN, START TO TOKEN, on the calling thread. The driver runs it on its own thread;
/// `tests/live_account.rs` runs it against a real Worker.
///
/// The order is the contract's, and each step's failure is its own [`Ended`]. The verifier never
/// leaves this function except in the body of the one exchange request.
pub fn sign_in(
    base: &str,
    wire: &mut dyn Wire,
    browser: &dyn Fn(&str) -> Result<(), String>,
    cancel: &AtomicBool,
    wait: Duration,
) -> Result<(AppToken, Profile), Ended> {
    let pkce = Pkce::new().map_err(Ended::Failed)?;
    let listener = Loopback::bind().map_err(Ended::Failed)?;
    browser(&app_url(
        base,
        &pkce.challenge,
        listener.port(),
        &pkce.state,
    ))
    .map_err(Ended::Failed)?;
    let code = match listener.wait(&pkce.state, cancel, wait) {
        Waited::Code(c) => c,
        Waited::Denied => return Err(Ended::Denied),
        Waited::Cancelled => return Err(Ended::Cancelled),
        Waited::TimedOut => return Err(Ended::TimedOut),
        Waited::Refused(word) => {
            return Err(Ended::Failed(format!(
                "The website did not sign Grimoire in ({word})."
            )))
        }
        Waited::Broken(e) => {
            return Err(Ended::Failed(format!(
                "Grimoire stopped listening for the browser: {e}"
            )))
        }
    };
    /* CLOSED BEFORE THE EXCHANGE. Nothing else may arrive on that port for this sign-in. */
    drop(listener);
    if cancel.load(Ordering::SeqCst) {
        return Err(Ended::Cancelled);
    }
    exchange(wire, base, &code, &pkce.verifier).map_err(|r| match r {
        Refused::InvalidGrant => Ended::InvalidGrant,
        Refused::Unreachable(e) => Ended::Failed(format!(
            "Could not reach the account service to finish signing in: {e}"
        )),
        other => Ended::Failed(format!("Signing in did not finish: {other}.")),
    })
}

/// What the lock guards. One lock, so a transition (view, token and generation together) is never
/// seen half made.
struct Inner {
    view: AccountView,
    /// Held while signed in, and while a saved token is being checked or could not be checked.
    token: Option<AppToken>,
    generation: u64,
    /// The waiting sign-in's cancel flag, if one is waiting.
    cancel: Option<Arc<AtomicBool>>,
}

/// The Ragnarok account sign-in, driven on its own threads.
pub struct Account {
    shared: Arc<Mutex<Inner>>,
    config: Arc<Config>,
}

/// A poisoned mutex holds a perfectly good value; see `updater::run::lock`.
fn lock(m: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// What one thread holds: the shared state, the config, the context to wake, and its generation.
struct Turn {
    shared: Arc<Mutex<Inner>>,
    config: Arc<Config>,
    ctx: egui::Context,
    generation: u64,
}

impl Turn {
    /// Apply `f` only if this thread's generation is still current. `false` when it is stale.
    fn settle(&self, f: impl FnOnce(&mut Inner)) -> bool {
        let mut g = lock(&self.shared);
        if g.generation != self.generation {
            return false;
        }
        f(&mut g);
        drop(g);
        self.ctx.request_repaint_of(egui::ViewportId::ROOT);
        true
    }
}

/// The system browser. NOT `shell::open`: its error names the path it was given, and this URL
/// carries the state, which has no business in a sentence on screen.
fn open_in_browser(url: &str) -> Result<(), String> {
    open::that_detached(url).map_err(|e| format!("Could not open the browser: {e}"))
}

/// A profile in the words the Settings screen draws.
fn view_in(p: &Profile) -> AccountView {
    let name = if p.display_name.trim().is_empty() {
        "your account".to_owned()
    } else {
        p.display_name.clone()
    };
    AccountView::In {
        name,
        providers: p
            .identities
            .iter()
            .map(|i| format!("{} ({})", provider_label(&i.provider), i.username))
            .collect(),
    }
}

impl Account {
    /// The real thing: the real service (or a checked override), the owner's token file, `ureq`,
    /// the system browser, and five minutes.
    pub fn production() -> Account {
        Account::new(Config {
            base: super::base(),
            path: super::token_path(),
            wire: Arc::new(|| Box::new(Https::default()) as Box<dyn Wire>),
            browser: Arc::new(open_in_browser),
            wait: WAIT_FOR_BROWSER,
        })
    }

    pub fn new(config: Config) -> Account {
        Account {
            shared: Arc::new(Mutex::new(Inner {
                view: AccountView::Out,
                token: None,
                generation: 0,
                cancel: None,
            })),
            config: Arc::new(config),
        }
    }

    /// What the screen draws. A clone of one small enum; safe every frame.
    pub fn view(&self) -> AccountView {
        lock(&self.shared).view.clone()
    }

    /// PICK UP A SAVED SIGN-IN, IF THERE IS ONE. Call once at startup.
    ///
    /// Silent with nothing saved, which is the first launch, every launch off Windows, and every
    /// launch after a sign-out: no thread and no request. Otherwise `/api/me` decides. A 401 means
    /// the token was revoked or expired and the file goes; no answer at all keeps it, because a
    /// laptop that starts offline has not been signed out.
    pub fn restore(&self, ctx: &egui::Context) {
        if let Some(t) = load_token(self.config.path.as_deref()) {
            self.check(ctx, t);
        }
    }

    /// Act on a press from the Settings screen. Returns at once.
    pub fn ask(&self, ctx: &egui::Context, a: Ask) {
        match a {
            Ask::SignIn => self.begin(ctx),
            Ask::Cancel => self.cancel(ctx),
            Ask::SignOut => self.sign_out(ctx),
            Ask::Manage => {
                if let Err(e) = (self.config.browser)(&self.config.base) {
                    log::warn!("{e}");
                }
            }
        }
    }

    fn turn(&self, ctx: &egui::Context, generation: u64) -> Turn {
        Turn {
            shared: self.shared.clone(),
            config: self.config.clone(),
            ctx: ctx.clone(),
            generation,
        }
    }

    /// Ask the website whether `token` still works, on a thread.
    fn check(&self, ctx: &egui::Context, token: AppToken) {
        let turn = {
            let mut g = lock(&self.shared);
            g.generation += 1;
            if let Some(c) = g.cancel.take() {
                c.store(true, Ordering::SeqCst);
            }
            g.view = AccountView::Checking;
            g.token = Some(token.clone());
            self.turn(ctx, g.generation)
        };
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        let generation = turn.generation;
        let spawned = std::thread::Builder::new()
            .name("account-check".to_owned())
            .spawn(move || {
                let mut wire = (turn.config.wire)();
                let path = turn.config.path.clone();
                match me(wire.as_mut(), &turn.config.base, &token) {
                    Ok(p) => {
                        turn.settle(|i| i.view = view_in(&p));
                    }
                    /* REVOKED OR EXPIRED: forgotten on disk as well as in memory, so a dead token
                     * is not checked again on every launch forever. Inside `settle`, so a stale
                     * thread can never delete a token a newer sign-in just saved. */
                    Err(Refused::Unauthenticated) => {
                        turn.settle(|i| {
                            i.token = None;
                            forget_token(path.as_deref());
                            i.view = AccountView::Out;
                        });
                    }
                    Err(Refused::Unreachable(e)) => {
                        log::info!("the saved account sign-in could not be checked: {e}");
                        turn.settle(|i| {
                            i.view = AccountView::Unchecked(
                                "Could not reach the account service to check your saved sign-in. \
                                 It is kept, so try again once you are online."
                                    .to_owned(),
                            )
                        });
                    }
                    Err(other) => {
                        turn.settle(|i| {
                            i.view = AccountView::Unchecked(format!(
                                "Your saved sign-in could not be checked: {other}."
                            ))
                        });
                    }
                }
            });
        if let Err(e) = spawned {
            let mut g = lock(&self.shared);
            if g.generation == generation {
                g.view = AccountView::Failed(format!("The account thread could not start: {e}"));
            }
        }
    }

    /// Start a browser sign-in, or re-check a saved token that could not be checked.
    fn begin(&self, ctx: &egui::Context) {
        let (turn, cancel) = {
            let mut g = lock(&self.shared);
            /* ONLY FROM A RESTING STATE. A second press while the browser is open would open a
             * second tab for a second sign-in racing the first. */
            if !matches!(
                g.view,
                AccountView::Out | AccountView::Failed(_) | AccountView::Unchecked(_)
            ) {
                return;
            }
            /* TRY AGAIN OVER A SAVED SIGN-IN THAT COULD NOT BE CHECKED IS A RE-CHECK, not a new
             * sign-in: the saved token was never refused, only unanswered. Signing in as somebody
             * else goes through Sign out, which takes the token. */
            if let Some(t) = g.token.clone() {
                drop(g);
                return self.check(ctx, t);
            }
            g.generation += 1;
            let cancel = Arc::new(AtomicBool::new(false));
            g.cancel = Some(cancel.clone());
            g.view = AccountView::Waiting;
            (self.turn(ctx, g.generation), cancel)
        };
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        let generation = turn.generation;
        let spawned = std::thread::Builder::new()
            .name("account-sign-in".to_owned())
            .spawn(move || {
                let mut wire = (turn.config.wire)();
                let config = turn.config.clone();
                let outcome = sign_in(
                    &config.base,
                    wire.as_mut(),
                    config.browser.as_ref(),
                    &cancel,
                    config.wait,
                );
                let failed = |why: String| {
                    turn.settle(|i| {
                        i.cancel = None;
                        i.view = AccountView::Failed(why);
                    });
                };
                match outcome {
                    Ok((token, profile)) => {
                        let kept = turn.settle(|i| {
                            save_token(config.path.as_deref(), &token);
                            i.token = Some(token.clone());
                            i.cancel = None;
                            i.view = view_in(&profile);
                        });
                        /* CANCELLED WHILE THE EXCHANGE WAS IN FLIGHT. The owner said stop, so
                         * this token is not kept; and a token nobody holds should not stay live
                         * on the website for ninety days either. */
                        if !kept {
                            let _ = super::sign_out(wire.as_mut(), &config.base, &token);
                        }
                    }
                    /* `cancel` already put the screen back; this thread is stale. */
                    Err(Ended::Cancelled) => {}
                    Err(Ended::Denied) => {
                        turn.settle(|i| {
                            i.cancel = None;
                            i.view = AccountView::Out;
                        });
                    }
                    Err(Ended::TimedOut) => failed(
                        "The sign-in was not finished in the browser in time, so it was stopped."
                            .to_owned(),
                    ),
                    Err(Ended::InvalidGrant) => failed(
                        "The website did not accept the sign-in code, so nothing was signed in."
                            .to_owned(),
                    ),
                    Err(Ended::Failed(why)) => failed(why),
                }
            });
        if let Err(e) = spawned {
            let mut g = lock(&self.shared);
            if g.generation == generation {
                g.cancel = None;
                g.view = AccountView::Failed(format!("The sign-in thread could not start: {e}"));
            }
        }
    }

    /// Stop waiting for the browser. The screen goes back at once; the thread notices within a
    /// moment and closes the port.
    fn cancel(&self, ctx: &egui::Context) {
        let mut g = lock(&self.shared);
        if g.view != AccountView::Waiting {
            return;
        }
        g.generation += 1;
        if let Some(c) = g.cancel.take() {
            c.store(true, Ordering::SeqCst);
        }
        g.view = AccountView::Out;
        drop(g);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    /// SIGN OUT: forgotten here first, then revoked on the website.
    ///
    /// ANY HELD TOKEN, NOT ONLY A CHECKED ONE. A saved sign-in the website could not confirm
    /// (offline, a challenge page, a 5xx) is still a token on disk that the next launch restores
    /// and that stays live on the website. Keyed on the view `In`, Sign out could not reach it,
    /// and Try again only re-asks the same question, so the owner could neither sign out nor sign
    /// in as anybody else until the service answered exactly 200 or 401. A press with nothing held
    /// does nothing.
    ///
    /// THE ORDER IS DELIBERATE. The file and the in-memory token go under the lock before any
    /// request, so a sign-out is complete on this computer the instant it is pressed, offline or
    /// not, and closing the app a moment later cannot leave a token on disk that the next launch
    /// would restore. The new generation makes a check still in flight stale, so it cannot put the
    /// view back. The revoke then runs on its own thread; if it cannot reach the website the token
    /// is already gone from here, and it lapses on its own.
    fn sign_out(&self, ctx: &egui::Context) {
        let token = {
            let mut g = lock(&self.shared);
            let Some(token) = g.token.take() else {
                return;
            };
            g.generation += 1;
            forget_token(self.config.path.as_deref());
            g.view = AccountView::Out;
            token
        };
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        let config = self.config.clone();
        let spawned = std::thread::Builder::new()
            .name("account-sign-out".to_owned())
            .spawn(move || {
                let mut wire = (config.wire)();
                if let Err(e) = super::sign_out(wire.as_mut(), &config.base, &token) {
                    log::info!(
                        "the app token could not be revoked on the website, and it is forgotten \
                         on this computer anyway: {e}"
                    );
                }
            });
        if let Err(e) = spawned {
            log::info!("the sign-out thread could not start, so the token was only forgotten: {e}");
        }
    }
}

impl Drop for Account {
    /// A sign-in waiting on the browser stops, and its port closes. Not joined, like every other
    /// worker in this app: the UI thread must not stall on a request in flight.
    fn drop(&mut self) {
        if let Some(c) = lock(&self.shared).cancel.take() {
            c.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::challenge_for;
    use std::collections::{BTreeMap, VecDeque};

    const BASE: &str = "https://accounts.test/account";
    const TOKEN: &str = "rsa_Zm9vYmFyYmF6cXV4cXV1eHF1dXhxdXV4cXV1eHF1dXg";
    const ACCOUNT: &str = r#"{"id":"acc_01","display_name":"Broken Stoic","identities":[{"provider":"twitch","username":"broken_stoic","linked_at":"2026-09-12T10:00:00Z"},{"provider":"google","username":"BrokenStoic","linked_at":"2026-09-12T10:05:00Z"}]}"#;

    fn token_ok() -> String {
        format!(r#"{{"token":"{TOKEN}","account":{ACCOUNT}}}"#)
    }

    fn signed_in() -> AccountView {
        AccountView::In {
            name: "Broken Stoic".to_owned(),
            providers: vec![
                "Twitch (broken_stoic)".to_owned(),
                "YouTube (BrokenStoic)".to_owned(),
            ],
        }
    }

    /// One request the fake wire was asked to make.
    struct Asked {
        method: &'static str,
        url: String,
        body: String,
        bearer: Option<String>,
    }

    /// The replies a fake wire gives, and what it was asked, shared with every wire it makes.
    #[derive(Clone, Default)]
    struct Script {
        replies: Arc<Mutex<VecDeque<(u16, String)>>>,
        asked: Arc<Mutex<Vec<Asked>>>,
    }

    impl Script {
        fn new(replies: &[(u16, &str)]) -> Script {
            let s = Script::default();
            for (c, b) in replies {
                s.push(*c, b);
            }
            s
        }
        fn push(&self, code: u16, body: &str) {
            self.replies
                .lock()
                .unwrap()
                .push_back((code, body.to_owned()));
        }
        fn asked(&self) -> std::sync::MutexGuard<'_, Vec<Asked>> {
            self.asked.lock().unwrap()
        }
        fn maker(&self) -> WireMaker {
            let s = self.clone();
            Arc::new(move || Box::new(Fake(s.clone())) as Box<dyn Wire>)
        }
    }

    struct Fake(Script);

    impl Fake {
        fn answer(
            &mut self,
            method: &'static str,
            url: &str,
            body: &str,
            bearer: Option<&str>,
        ) -> Result<(u16, String), String> {
            self.0.asked().push(Asked {
                method,
                url: url.to_owned(),
                body: body.to_owned(),
                bearer: bearer.map(str::to_owned),
            });
            self.0
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "the network is not there".to_owned())
        }
    }

    impl Wire for Fake {
        fn post_json(&mut self, url: &str, json: &str) -> Result<(u16, String), String> {
            self.answer("POST", url, json, None)
        }
        fn get_bearer(&mut self, url: &str, token: &str) -> Result<(u16, String), String> {
            self.answer("GET", url, "", Some(token))
        }
        fn post_bearer(&mut self, url: &str, token: &str) -> Result<(u16, String), String> {
            self.answer("POST", url, "", Some(token))
        }
    }

    /// What the fake browser did: the URLs it was opened at and the pages the listener answered.
    #[derive(Clone, Default)]
    struct Seen {
        opened: Arc<Mutex<Vec<String>>>,
        answers: Arc<Mutex<Vec<String>>>,
    }

    fn query(url: &str) -> BTreeMap<String, String> {
        url.split_once('?')
            .map(|(_, q)| {
                q.split('&')
                    .filter_map(|p| p.split_once('='))
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A GET on the loopback, played the way a browser plays it, and the whole response. Through
    /// `loopback::test_browser`, so this file never names `std::net`: see the note there.
    fn visit(port: u16, target: &str) -> String {
        crate::account::loopback::test_browser::visit(port, target)
    }

    /// A browser that, once opened, visits each target on the loopback in turn. `{state}` in a
    /// target becomes the state the app put in the URL.
    fn browser(seen: &Seen, targets: &[&str]) -> Browser {
        let seen = seen.clone();
        let targets: Vec<String> = targets.iter().map(|t| (*t).to_owned()).collect();
        Arc::new(move |url: &str| {
            seen.opened.lock().unwrap().push(url.to_owned());
            let q = query(url);
            let port: u16 = q
                .get("port")
                .and_then(|p| p.parse().ok())
                .ok_or_else(|| "no port in the URL".to_owned())?;
            let state = q.get("state").cloned().unwrap_or_default();
            let (targets, answers) = (targets.clone(), seen.answers.clone());
            std::thread::spawn(move || {
                for t in targets {
                    let got = visit(port, &t.replace("{state}", &state));
                    answers.lock().unwrap().push(got);
                }
            });
            Ok(())
        })
    }

    fn config(script: &Script, browser: Browser, path: &std::path::Path, wait: Duration) -> Config {
        Config {
            base: BASE.to_owned(),
            path: Some(path.to_owned()),
            wire: script.maker(),
            browser,
            wait,
        }
    }

    /// A file this test owns, under the OS temp dir and NEVER the owner's config directory: see
    /// `twitch_auth::tests::tmp` for the accident that rule came from.
    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("eqlg-account-{}-{tag}.bin", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// Wait for a condition a thread will make true, or give up. Generous for the reason
    /// `twitch_auth::tests::settles` gives: it returns the moment the condition holds.
    fn eventually(ok: impl Fn() -> bool) -> bool {
        for _ in 0..1_000 {
            if ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn port_of(seen: &Seen) -> u16 {
        query(&seen.opened.lock().unwrap()[0])["port"]
            .parse()
            .expect("a port")
    }

    fn port_closed(port: u16) -> bool {
        eventually(|| crate::account::loopback::test_browser::refuses(port))
    }

    /// THE WHOLE SIGN-IN, THROUGH A REAL LOOPBACK SOCKET, ENDS SIGNED IN AS THE ACCOUNT.
    ///
    /// Every step of the contract is asserted against what actually crossed the seams: the URL
    /// the browser was opened at, the favicon request a browser makes on its own, the page the
    /// listener answered, the exchange body, and the file on disk.
    ///
    /// THE VERIFIER IS CHECKED AGAINST THE CHALLENGE THE BROWSER CARRIED, which is PKCE's whole
    /// promise: a flow that minted a second pair for the exchange would pass every other assertion
    /// here and fail every real sign-in.
    #[test]
    fn a_whole_sign_in_through_a_real_loopback_socket_ends_signed_in() {
        let script = Script::new(&[(200, &token_ok())]);
        let seen = Seen::default();
        let path = tmp("whole");
        let a = Account::new(config(
            &script,
            browser(
                &seen,
                &["/favicon.ico", "/callback?code=THE-CODE&state={state}"],
            ),
            &path,
            Duration::from_secs(30),
        ));
        let ctx = egui::Context::default();
        a.ask(&ctx, Ask::SignIn);
        assert!(
            eventually(|| a.view() == signed_in()),
            "the sign-in did not finish: {:?}",
            a.view()
        );

        let url = seen.opened.lock().unwrap()[0].clone();
        assert!(url.starts_with(&format!("{BASE}/app?challenge=")), "{url}");
        let q = query(&url);
        assert_eq!(
            q.len(),
            3,
            "the URL carries exactly challenge, port and state: {url}"
        );
        let port: u16 = q["port"].parse().expect("port");
        assert!(port >= 1024);
        assert!((16..=128).contains(&q["state"].len()));
        assert_eq!(q["challenge"].len(), 43);

        {
            let asked = script.asked();
            assert_eq!(asked.len(), 1, "exactly one request: the exchange");
            assert_eq!(asked[0].method, "POST");
            assert_eq!(asked[0].url, format!("{BASE}/api/app/token"));
            assert_eq!(asked[0].bearer, None);
            let body: serde_json::Value = serde_json::from_str(&asked[0].body).expect("JSON");
            assert_eq!(body["code"], "THE-CODE");
            let verifier = body["verifier"].as_str().expect("a verifier");
            assert_eq!(
                challenge_for(verifier),
                q["challenge"],
                "the verifier sent to the exchange does not belong to the challenge the browser \
                 carried, so the website refuses every sign-in"
            );
            assert!(
                !url.contains(verifier),
                "the verifier went into the browser URL"
            );
        }

        assert!(eventually(|| seen.answers.lock().unwrap().len() == 2));
        let answers = seen.answers.lock().unwrap().clone();
        assert!(answers[0].starts_with("HTTP/1.1 404"), "{}", answers[0]);
        assert!(answers[1].starts_with("HTTP/1.1 200"), "{}", answers[1]);
        assert!(port_closed(port), "the port stayed open after the sign-in");

        if crate::secret::persists() {
            let raw = std::fs::read(&path).expect("the token was saved");
            assert!(
                !raw.windows(TOKEN.len()).any(|w| w == TOKEN.as_bytes()),
                "the token is on disk in the clear"
            );
            assert_eq!(
                load_token(Some(&path)).map(|t| t.expose().to_owned()),
                Some(TOKEN.to_owned())
            );
        } else {
            assert!(!path.exists(), "a token was written with no way to seal it");
        }
        forget_token(Some(&path));
    }

    /// A MISMATCHED STATE IS REFUSED AND ITS CODE IS NEVER EXCHANGED; THE REAL ONE STILL WORKS.
    #[test]
    fn a_mismatched_state_is_refused_and_its_code_is_never_exchanged() {
        let script = Script::new(&[(200, &token_ok())]);
        let seen = Seen::default();
        let path = tmp("mismatch");
        let a = Account::new(config(
            &script,
            browser(
                &seen,
                &[
                    "/callback?code=EVIL&state=not-the-state-this-app-made",
                    "/callback?code=EVIL",
                    "/callback?code=GOOD&state={state}",
                ],
            ),
            &path,
            Duration::from_secs(30),
        ));
        let ctx = egui::Context::default();
        a.ask(&ctx, Ask::SignIn);
        assert!(eventually(|| a.view() == signed_in()), "{:?}", a.view());
        let asked = script.asked();
        assert_eq!(asked.len(), 1);
        let body: serde_json::Value = serde_json::from_str(&asked[0].body).expect("JSON");
        assert_eq!(
            body["code"], "GOOD",
            "a code with the wrong state was exchanged"
        );
        drop(asked);
        assert!(eventually(|| seen.answers.lock().unwrap().len() == 3));
        let answers = seen.answers.lock().unwrap().clone();
        assert!(answers[0].starts_with("HTTP/1.1 400"), "{}", answers[0]);
        assert!(answers[1].starts_with("HTTP/1.1 400"), "{}", answers[1]);
        forget_token(Some(&path));
    }

    /// A MISMATCH ALONE NEVER SIGNS ANYBODY IN, AND NEVER ASKS THE WEBSITE ANYTHING.
    #[test]
    fn a_mismatched_state_alone_asks_the_website_nothing() {
        let script = Script::new(&[(200, &token_ok())]);
        let seen = Seen::default();
        let path = tmp("mismatch-alone");
        let a = Account::new(config(
            &script,
            browser(&seen, &["/callback?code=EVIL&state=guess"]),
            &path,
            Duration::from_millis(800),
        ));
        let ctx = egui::Context::default();
        a.ask(&ctx, Ask::SignIn);
        assert!(eventually(|| matches!(a.view(), AccountView::Failed(_))));
        assert!(
            script.asked().is_empty(),
            "a code with a guessed state went to the website"
        );
        assert!(!path.exists());
    }

    /// `access_denied` ENDS SIGNED OUT, NOT IN A RED ERROR, AND ASKS THE WEBSITE NOTHING.
    #[test]
    fn access_denied_ends_signed_out_and_asks_the_website_nothing() {
        let script = Script::default();
        let seen = Seen::default();
        let path = tmp("denied");
        let a = Account::new(config(
            &script,
            browser(&seen, &["/callback?error=access_denied&state={state}"]),
            &path,
            Duration::from_secs(30),
        ));
        let ctx = egui::Context::default();
        a.ask(&ctx, Ask::SignIn);
        assert_eq!(a.view(), AccountView::Waiting);
        /* WAIT ON THE PORT, NOT ON THE VIEW. `Out` is where a fresh driver already is, so a test
         * that waited for `Out` could pass before the thread ever ran. The port closing only
         * happens when the flow has really ended. */
        assert!(eventually(|| !seen.opened.lock().unwrap().is_empty()));
        assert!(port_closed(port_of(&seen)), "the flow never ended");
        assert!(
            eventually(|| a.view() == AccountView::Out),
            "{:?}",
            a.view()
        );
        assert!(script.asked().is_empty());
        assert!(!path.exists());
    }

    /// CANCEL PUTS THE SCREEN BACK AT ONCE, CLOSES THE PORT, AND A NEW SIGN-IN CAN START.
    ///
    /// WHAT MUTATION MAKES THIS RED: `cancel` not storing the flag (the port stays open for the
    /// whole wait), or a `running` latch in place of generations (the second Sign in is ignored).
    #[test]
    fn cancelling_goes_back_at_once_closes_the_port_and_can_start_again() {
        let script = Script::default();
        let seen = Seen::default();
        let path = tmp("cancel");
        let a = Account::new(config(
            &script,
            browser(&seen, &[]),
            &path,
            Duration::from_secs(60),
        ));
        let ctx = egui::Context::default();
        a.ask(&ctx, Ask::SignIn);
        assert!(eventually(|| !seen.opened.lock().unwrap().is_empty()));
        a.ask(&ctx, Ask::Cancel);
        assert_eq!(
            a.view(),
            AccountView::Out,
            "Cancel did not put the screen back at once"
        );
        assert!(
            port_closed(port_of(&seen)),
            "the port stayed open after Cancel"
        );

        a.ask(&ctx, Ask::SignIn);
        assert_eq!(
            a.view(),
            AccountView::Waiting,
            "a second Sign in did nothing"
        );
        assert!(eventually(|| seen.opened.lock().unwrap().len() == 2));
        assert!(script.asked().is_empty());
    }

    /// A BROWSER THAT NEVER COMES BACK ENDS IN WORDS AND CLOSES THE PORT.
    #[test]
    fn a_browser_that_never_comes_back_times_out() {
        let seen = Seen::default();
        let a = Account::new(config(
            &Script::default(),
            browser(&seen, &[]),
            &tmp("timeout"),
            Duration::from_millis(300),
        ));
        a.ask(&egui::Context::default(), Ask::SignIn);
        assert!(eventually(|| matches!(a.view(), AccountView::Failed(_))));
        let AccountView::Failed(why) = a.view() else {
            unreachable!()
        };
        assert!(why.contains("in time"), "{why}");
        assert!(port_closed(port_of(&seen)));
    }

    /// `invalid_grant` IS A FAILURE, NOTHING IS KEPT, AND TRY AGAIN STARTS A NEW SIGN-IN.
    #[test]
    fn invalid_grant_keeps_nothing_and_try_again_starts_over() {
        let script = Script::new(&[(400, r#"{"error":"invalid_grant"}"#)]);
        let seen = Seen::default();
        let path = tmp("invalid-grant");
        let a = Account::new(config(
            &script,
            browser(&seen, &["/callback?code=SPENT&state={state}"]),
            &path,
            Duration::from_secs(30),
        ));
        let ctx = egui::Context::default();
        a.ask(&ctx, Ask::SignIn);
        assert!(eventually(|| matches!(a.view(), AccountView::Failed(_))));
        let AccountView::Failed(why) = a.view() else {
            unreachable!()
        };
        assert!(why.contains("did not accept"), "{why}");
        assert!(!path.exists(), "a refused sign-in left a file");
        assert!(lock(&a.shared).token.is_none());

        a.ask(&ctx, Ask::SignIn);
        assert_eq!(a.view(), AccountView::Waiting, "Try again is a dead button");
        assert!(eventually(|| seen.opened.lock().unwrap().len() == 2));
    }

    /// SIGN OUT FORGETS AT ONCE AND REVOKES WITH THE BEARER; OFFLINE, IT STILL FORGETS.
    #[test]
    fn sign_out_forgets_at_once_and_revokes_even_when_offline() {
        for online in [true, false] {
            let script = Script::new(&[(200, &token_ok())]);
            if online {
                script.push(204, "");
            }
            let seen = Seen::default();
            let path = tmp(if online { "out-online" } else { "out-offline" });
            let a = Account::new(config(
                &script,
                browser(&seen, &["/callback?code=C&state={state}"]),
                &path,
                Duration::from_secs(30),
            ));
            let ctx = egui::Context::default();
            a.ask(&ctx, Ask::SignIn);
            assert!(eventually(|| a.view() == signed_in()), "{:?}", a.view());
            assert_eq!(path.exists(), crate::secret::persists());

            a.ask(&ctx, Ask::SignOut);
            assert_eq!(a.view(), AccountView::Out, "sign-out waited on the network");
            assert!(!path.exists(), "the token file survived sign-out");
            assert!(lock(&a.shared).token.is_none());
            assert!(
                eventually(|| script.asked().len() == 2),
                "no revoke was attempted"
            );
            let asked = script.asked();
            assert_eq!(asked[1].method, "POST");
            assert_eq!(asked[1].url, format!("{BASE}/api/app/sign-out"));
            assert_eq!(asked[1].bearer.as_deref(), Some(TOKEN));
            drop(asked);
            assert_eq!(a.view(), AccountView::Out, "online={online}");
        }
    }

    /// NOTHING SAVED MEANS NO THREAD AND NO REQUEST.
    #[test]
    fn with_nothing_saved_restore_does_nothing_at_all() {
        let path = tmp("absent");
        let a = Account::new(Config {
            base: BASE.to_owned(),
            path: Some(path),
            wire: Arc::new(|| -> Box<dyn Wire> {
                panic!("restore built a wire with nothing saved, so it would have dialled")
            }),
            browser: Arc::new(|_: &str| -> Result<(), String> {
                panic!("restore opened a browser")
            }),
            wait: Duration::from_secs(1),
        });
        a.restore(&egui::Context::default());
        assert_eq!(a.view(), AccountView::Out);
    }

    /// Save a token the way a sign-in would. DPAPI only, so Windows only, like the tests below.
    #[cfg(windows)]
    fn saved(tag: &str) -> PathBuf {
        let p = tmp(tag);
        save_token(Some(&p), &AppToken::parse(TOKEN).expect("token"));
        assert!(p.exists());
        p
    }

    /// A 401 ON RESTORE FORGETS THE TOKEN, ON DISK, AND ENDS SIGNED OUT RATHER THAN IN RED.
    ///
    /// WAITS ON THE FILE, for the reason `twitch_auth` gives: `Out` is also where the thread
    /// starts from, so only the deletion proves the thread got there.
    #[cfg(windows)]
    #[test]
    fn a_revoked_saved_sign_in_is_forgotten_on_restore() {
        let path = saved("revoked");
        let script = Script::new(&[(401, r#"{"error":"unauthenticated"}"#)]);
        let a = Account::new(config(
            &script,
            browser(&Seen::default(), &[]),
            &path,
            WAIT_FOR_BROWSER,
        ));
        a.restore(&egui::Context::default());
        assert!(
            eventually(|| !path.exists()),
            "a revoked token stayed on disk"
        );
        assert!(
            eventually(|| a.view() == AccountView::Out),
            "{:?}",
            a.view()
        );
        let asked = script.asked();
        assert_eq!(asked[0].method, "GET");
        assert_eq!(asked[0].url, format!("{BASE}/api/me"));
        assert_eq!(asked[0].bearer.as_deref(), Some(TOKEN));
    }

    /// A GOOD SAVED SIGN-IN COMES BACK SIGNED IN, WITH NO BROWSER.
    #[cfg(windows)]
    #[test]
    fn a_saved_sign_in_comes_back_after_a_restart() {
        let path = saved("restart");
        let script = Script::new(&[(200, ACCOUNT)]);
        let a = Account::new(Config {
            browser: Arc::new(|_: &str| -> Result<(), String> {
                panic!("a restore opened a browser")
            }),
            ..config(
                &script,
                browser(&Seen::default(), &[]),
                &path,
                WAIT_FOR_BROWSER,
            )
        });
        let ctx = egui::Context::default();
        a.restore(&ctx);
        assert!(eventually(|| a.view() == signed_in()), "{:?}", a.view());
        assert!(path.exists());
        forget_token(Some(&path));
    }

    /// OFFLINE AT STARTUP IS NOT SIGNED OUT: the file stays, the screen says why, and Try again
    /// re-checks the same token rather than opening a browser.
    #[cfg(windows)]
    #[test]
    fn an_offline_restore_keeps_the_token_and_try_again_rechecks_it() {
        let path = saved("offline");
        let script = Script::default();
        let a = Account::new(Config {
            browser: Arc::new(|_: &str| -> Result<(), String> {
                panic!("Try again opened a browser over a saved sign-in")
            }),
            ..config(
                &script,
                browser(&Seen::default(), &[]),
                &path,
                WAIT_FOR_BROWSER,
            )
        });
        let ctx = egui::Context::default();
        a.restore(&ctx);
        assert!(eventually(|| matches!(a.view(), AccountView::Unchecked(_))));
        assert!(path.exists(), "an offline start deleted the saved sign-in");

        script.push(200, ACCOUNT);
        a.ask(&ctx, Ask::SignIn);
        assert!(eventually(|| a.view() == signed_in()), "{:?}", a.view());
        forget_token(Some(&path));
    }

    /// A SAVED SIGN-IN THAT COULD NOT BE CHECKED CAN STILL BE SIGNED OUT, AND THEN REPLACED.
    ///
    /// Every answer here leaves the token held: no network at all, a Cloudflare challenge page, a
    /// 5xx, and a 3xx the wire does not follow. None of them is a 401, so none of them forgets the
    /// token, and an owner offline or behind a broken route must still be able to press Sign out
    /// and have it complete on this computer, then sign in as somebody else.
    ///
    /// WHAT MUTATION MAKES THIS RED: `sign_out` refusing anything but `In` (the old rule; the file
    /// and the token outlive the press), or `begin` re-checking a token Sign out should have taken.
    #[test]
    fn a_saved_sign_in_that_could_not_be_checked_can_still_be_signed_out() {
        for (tag, reply) in [
            ("unchecked-offline", None),
            (
                "unchecked-challenge",
                Some((403, "<!doctype html><title>Just a moment...</title>")),
            ),
            ("unchecked-down", Some((503, ""))),
            ("unchecked-moved", Some((302, ""))),
        ] {
            let path = tmp(tag);
            let token = AppToken::parse(TOKEN).expect("token");
            save_token(Some(&path), &token);
            let script = Script::default();
            if let Some((status, body)) = reply {
                script.push(status, body);
            }
            let seen = Seen::default();
            let a = Account::new(config(
                &script,
                browser(&seen, &[]),
                &path,
                Duration::from_secs(60),
            ));
            let ctx = egui::Context::default();
            /* THE REAL STARTUP WHERE A TOKEN CAN BE SAVED; THE SAME CHECK WHERE IT CANNOT. */
            if crate::secret::persists() {
                a.restore(&ctx);
            } else {
                a.check(&ctx, token);
            }
            assert!(
                eventually(|| a.view() != AccountView::Checking),
                "{tag}: the check never ended"
            );
            assert!(
                matches!(a.view(), AccountView::Unchecked(_)),
                "{tag}: {:?}",
                a.view()
            );
            assert!(
                lock(&a.shared).token.is_some(),
                "{tag}: the token was dropped"
            );

            a.ask(&ctx, Ask::SignOut);
            assert_eq!(
                a.view(),
                AccountView::Out,
                "{tag}: Sign out did nothing over a saved sign-in that could not be checked"
            );
            assert!(!path.exists(), "{tag}: the token file survived sign-out");
            assert!(lock(&a.shared).token.is_none(), "{tag}: the token survived");
            assert!(
                eventually(|| script.asked().len() == 2),
                "{tag}: no revoke was attempted"
            );
            {
                let asked = script.asked();
                assert_eq!(asked[1].url, format!("{BASE}/api/app/sign-out"), "{tag}");
                assert_eq!(asked[1].bearer.as_deref(), Some(TOKEN), "{tag}");
            }

            a.ask(&ctx, Ask::SignIn);
            assert_eq!(
                a.view(),
                AccountView::Waiting,
                "{tag}: no new sign-in could start after signing out"
            );
            assert!(
                eventually(|| seen.opened.lock().unwrap().len() == 1),
                "{tag}: the browser was never opened"
            );
            a.ask(&ctx, Ask::Cancel);
            assert!(port_closed(port_of(&seen)), "{tag}");
            assert_eq!(
                script.asked().len(),
                2,
                "{tag}: Sign in re-checked the token"
            );
        }
    }

    /// THE APP OWNS ONE, RESTORES IT WHERE BOTH CALLBACKS RUN, HANDS THE SCREEN ITS VIEW, AND
    /// ANSWERS THE SCREEN'S PRESSES.
    ///
    /// A SOURCE-TEXT FLOOR, the instrument `settings.rs` and `main.rs` already use for wiring a
    /// test cannot construct: `App` needs a real window. Everything above proves the driver; this
    /// proves the binary reaches it, which is the defect this tree keeps finding.
    #[test]
    fn the_app_owns_restores_draws_and_answers_the_account() {
        let main = include_str!("../main.rs");
        assert!(
            main.contains("account: grimoire_desktop::account::Account::production()"),
            "App does not construct the account driver"
        );
        let restored = main
            .find("if !self.restored {")
            .expect("the once-per-process restore block is gone");
        assert!(
            main[restored..restored + 300].contains("self.account.restore(ctx);"),
            "the saved account sign-in is never picked up at startup"
        );
        assert!(
            main.contains("self.settings_screen.account = Some(self.account.view());"),
            "the Settings screen is never handed the account view"
        );
        let asks = main
            .find("if let Some(a) = changed.account {")
            .expect("the Settings screen's account presses are never read");
        assert!(
            main[asks..asks + 200].contains("self.account.ask(&ctx, a);"),
            "the account presses are read and dropped"
        );
    }
}
