//! A REAL SIGN-IN AGAINST A REAL ACCOUNTS WORKER, RUN BY HAND.
//!
//! `#[ignore]`, for the reason `tests/live_twitch.rs` gives: `cargo test` never dials anything. This
//! file is the end to end proof the goal doc asks for ("Done when", item 3), run against
//! `wrangler dev` of `ragnarok-accounts` with the `fake` provider switched on. In that repo, with
//! `.dev.vars` copied from `.dev.vars.example`, start the Worker with `npm run dev`, then:
//!
//!     GRIMOIRE_ACCOUNT_BASE=http://127.0.0.1:8787/account \
//!     cargo test -p grimoire-desktop --test live_account -- --ignored --nocapture
//!
//! `GRIMOIRE_ACCOUNT_FAKE_USER` names the fake identity, default `e2e-tester`.
//!
//! TWO WAYS TO GET A 403 ON EVERY POST, BOTH MEASURED. The base must be the Worker's `PUBLIC_ORIGIN`
//! to the letter (`http://127.0.0.1:8787`, not `localhost`), because the played browser sends it as
//! the `Origin` of every POST. And a bare `wrangler dev` will not do: with the production routes in
//! `wrangler.jsonc` it rewrites each request's `Origin` to `http://ragnarok.systems`, which is why
//! `npm run dev` passes `--local-upstream`.
//!
//! WHAT IS REAL. All of the app's flow code: the loopback listener, the PKCE pair, the browser URL,
//! the exchange over `ureq`, the DPAPI store, `/api/me`, and sign-out. The one thing standing in is
//! the PERSON: [`Player`] plays the browser with its own `ureq` agent, following redirects by hand,
//! keeping cookies by hand, and pressing the buttons a person would press.
//!
//! THE BROWSER HALF READS THE WORKER'S REAL PAGES (`src/pages.ts`): the chooser's plain links to
//! `/account/start/fake`, the fake provider's `username` field, and the confirm page's Continue.
//!
//! NOTHING SECRET IS PRINTED. The trail prints the method, the status and the PATH of every page,
//! never a query string, so the code and the state stay off the terminal; the token is never
//! printed at all, and a page body is quoted only when the browser gives up on it.

use grimoire_desktop::account::{
    self, Account, AccountView, Ask, Config, Hidden, Https, Loopback, Pkce, Refused, Waited, Wire,
};
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a sign-in waits for the played browser. Well under the app's five minutes: a browser
/// that has not arrived in two has given up, and the log says why.
const WAIT: Duration = Duration::from_secs(120);

fn base() -> String {
    let raw = std::env::var(account::BASE_ENV).unwrap_or_else(|_| {
        panic!(
            "set {} to the Worker's account URL, for example http://localhost:8787/account",
            account::BASE_ENV
        )
    });
    account::base_url(Some(&raw)).unwrap_or_else(|e| panic!("{e}"))
}

fn user() -> String {
    std::env::var("GRIMOIRE_ACCOUNT_FAKE_USER").unwrap_or_else(|_| "e2e-tester".to_owned())
}

/// Where a live run keeps its token: the temp dir, never the owner's config directory.
fn scratch(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "eqlg-live-account-{}-{tag}.bin",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&p);
    p
}

/* ------------------------------------------------------------------ the tests -- */

/// THE FLOW CODE SIGNS IN, THE TOKEN IT GETS WORKS, AND AFTER SIGN-OUT IT DOES NOT.
///
/// Runs `account::sign_in` itself, so the token is in hand to prove the revocation with: a second
/// `/api/me` after sign-out must be a 401, which is the only observable that says the website
/// really forgot it rather than the app merely deleting a file.
#[test]
#[ignore = "needs a running accounts Worker with the fake provider; run by hand with --ignored"]
fn the_flow_code_signs_in_and_a_signed_out_token_is_refused() {
    let base = base();
    let user = user();
    let cancel = Arc::new(AtomicBool::new(false));
    let (browser, log) = playing(&base, &user, Some(cancel.clone()));
    let mut wire = Https::default();

    let signed = account::sign_in(&base, &mut wire, browser.as_ref(), &cancel, WAIT);
    print_log(&log);
    let (token, profile) = signed
        .unwrap_or_else(|e| panic!("the sign-in ended as {e:?}; see the browser trail above"));
    println!(
        "signed in as {:?} with {:?}",
        profile.display_name,
        profile
            .identities
            .iter()
            .map(|i| format!("{}:{}", i.provider, i.username))
            .collect::<Vec<_>>()
    );
    assert!(
        profile
            .identities
            .iter()
            .any(|i| i.provider == "fake" && i.username == user),
        "the account is not the fake identity {user:?}: {profile:?}"
    );

    /* THE STORE, WHERE THERE IS ONE. Off Windows nothing persists, by design, so the token in
     * hand stands in for the one that would have come back off disk. */
    let path = scratch("flow");
    account::save_token(Some(&path), &token);
    let held = if grimoire_desktop::secret::persists() {
        account::load_token(Some(&path)).expect("the saved token did not come back off disk")
    } else {
        assert!(!path.exists(), "a token was written with no way to seal it");
        token.clone()
    };

    assert_eq!(
        account::me(&mut wire, &base, &held),
        Ok(profile.clone()),
        "/api/me does not agree with the account the exchange returned"
    );
    assert_eq!(account::sign_out(&mut wire, &base, &held), Ok(()));
    assert_eq!(
        account::me(&mut wire, &base, &held),
        Err(Refused::Unauthenticated),
        "a signed out token still answers /api/me, so sign-out did not revoke it"
    );
    account::forget_token(Some(&path));
}

/// THE APP'S DRIVER SIGNS IN, COMES BACK AFTER A RESTART, AND SIGNS OUT.
///
/// The same driver `App` owns, with the production wire and a scratch token file. The restart is
/// a second driver over the same file calling `restore`, which is exactly what the next launch does.
#[test]
#[ignore = "needs a running accounts Worker with the fake provider; run by hand with --ignored"]
fn the_app_driver_signs_in_comes_back_after_a_restart_and_signs_out() {
    let base = base();
    let user = user();
    let path = scratch("driver");
    let make = |browser: account::Browser| {
        Account::new(Config {
            base: base.clone(),
            path: Some(path.clone()),
            wire: Arc::new(|| Box::new(Https::default()) as Box<dyn Wire>),
            browser,
            wait: WAIT,
        })
    };
    let ctx = egui::Context::default();

    let (browser, log) = playing(&base, &user, None);
    let first = make(browser);
    first.ask(&ctx, Ask::SignIn);
    let view = settled(&first, |v| !matches!(v, AccountView::Waiting), &log);
    print_log(&log);
    let AccountView::In { name, providers } = view else {
        panic!("the driver ended as {view:?}; see the browser trail above")
    };
    println!("the driver says: signed in as {name}, with {providers:?}");
    assert!(
        providers.iter().any(|p| p == &format!("fake ({user})")),
        "the Settings screen would not list the fake identity: {providers:?}"
    );

    if !grimoire_desktop::secret::persists() {
        first.ask(&ctx, Ask::SignOut);
        assert_eq!(first.view(), AccountView::Out);
        println!("no DPAPI here, so nothing persists and the restart half does not apply");
        return;
    }

    let token = account::load_token(Some(&path)).expect("the driver did not save the sign-in");
    drop(first);

    let again = make(Arc::new(|_: &str| -> Result<(), String> {
        Err("a restore opened the browser".to_owned())
    }));
    again.restore(&ctx);
    let restored = settled(&again, |v| !matches!(v, AccountView::Checking), &log);
    assert!(
        matches!(&restored, AccountView::In { name: n, .. } if *n == name),
        "the saved sign-in did not come back after a restart: {restored:?}"
    );

    again.ask(&ctx, Ask::SignOut);
    assert_eq!(again.view(), AccountView::Out);
    assert!(!path.exists(), "the token file survived sign-out");
    /* The revoke runs on its own thread, so ask until the website agrees, or give up. */
    let mut wire = Https::default();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match account::me(&mut wire, &base, &token) {
            Err(Refused::Unauthenticated) => break,
            other if Instant::now() > deadline => {
                panic!("after sign-out the token still answers /api/me with {other:?}")
            }
            _ => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

/// A SECOND SIGN-IN AS THE SAME PERSON IS THE SAME ACCOUNT, AND ANOTHER PERSON IS NOT.
///
/// `Profile` carries no account `id` (nothing in the app reads one), so [`Ids`] reads it off the
/// token reply. The second person is what makes the first comparison mean something: a Worker that
/// handed every sign-in the same id would pass "the same" and fail "not the same".
#[test]
#[ignore = "needs a running accounts Worker with the fake provider; run by hand with --ignored"]
fn signing_in_twice_as_one_person_lands_on_one_account() {
    let base = base();
    let user = user();
    let other = format!("{user}-other");
    let mut wire = Ids::default();
    let mut tokens = Vec::new();
    for who in [&user, &user, &other] {
        let cancel = Arc::new(AtomicBool::new(false));
        let (browser, log) = playing(&base, who, Some(cancel.clone()));
        let signed = account::sign_in(&base, &mut wire, browser.as_ref(), &cancel, WAIT);
        print_log(&log);
        let (token, _) = signed.unwrap_or_else(|e| panic!("the sign-in as {who:?} ended as {e:?}"));
        tokens.push(token);
    }
    for t in &tokens {
        assert_eq!(account::sign_out(&mut wire, &base, t), Ok(()));
    }
    let ids = &wire.ids;
    assert_eq!(ids.len(), 3, "a token reply carried no account id");
    assert!(ids.iter().all(|i| !i.is_empty()), "an empty account id");
    assert_eq!(
        ids[0], ids[1],
        "a second sign-in as {user:?} made a second account"
    );
    assert_ne!(ids[0], ids[2], "{user:?} and {other:?} share one account");
    println!("two sign-ins as {user:?}: one account; {other:?}: another");
}

/// A SIGN-IN CODE IS SPENT ON ITS FIRST USE, RIGHT OR WRONG.
///
/// The contract: "The code is consumed on the first attempt, right or wrong." Two rounds through
/// the app's own listener, PKCE pair and exchange. A code that bought a token is refused when it is
/// replayed; and a code first tried with somebody else's verifier is refused even with its own
/// verifier after, so a stolen code cannot be retried against guessed verifiers.
#[test]
#[ignore = "needs a running accounts Worker with the fake provider; run by hand with --ignored"]
fn a_sign_in_code_is_spent_on_its_first_use_right_or_wrong() {
    let base = base();
    let user = user();
    let mut wire = Https::default();

    let (pkce, code) = a_code(&base, &user);
    let (token, _) = account::exchange(&mut wire, &base, &code, &pkce.verifier)
        .unwrap_or_else(|e| panic!("the first exchange was refused: {e:?}"));
    assert_eq!(
        account::exchange(&mut wire, &base, &code, &pkce.verifier).map(|_| ()),
        Err(Refused::InvalidGrant),
        "a replayed code bought a second token"
    );
    assert_eq!(account::sign_out(&mut wire, &base, &token), Ok(()));

    let (pkce, code) = a_code(&base, &user);
    let wrong = Pkce::new().expect("random");
    assert_eq!(
        account::exchange(&mut wire, &base, &code, &wrong.verifier).map(|_| ()),
        Err(Refused::InvalidGrant),
        "a code was exchanged with a verifier that is not its own"
    );
    assert_eq!(
        account::exchange(&mut wire, &base, &code, &pkce.verifier).map(|_| ()),
        Err(Refused::InvalidGrant),
        "a code survived a wrong first attempt"
    );
}

/// One sign-in code, brought to the app's own listener by the played browser, and the PKCE pair
/// it belongs to.
fn a_code(base: &str, user: &str) -> (Pkce, Hidden) {
    let pkce = Pkce::new().expect("random");
    let listener = Loopback::bind().expect("a loopback port");
    let cancel = Arc::new(AtomicBool::new(false));
    let (browser, log) = playing(base, user, Some(cancel.clone()));
    browser(&account::app_url(
        base,
        &pkce.challenge,
        listener.port(),
        &pkce.state,
    ))
    .expect("the played browser opens");
    let waited = listener.wait(&pkce.state, &cancel, WAIT);
    print_log(&log);
    match waited {
        Waited::Code(code) => (pkce, code),
        other => panic!("the listener ended as {other:?}; see the browser trail above"),
    }
}

/// The production wire, keeping the `account.id` of each token reply it carries. Nothing else is
/// kept from those bodies: they hold the token.
#[derive(Default)]
struct Ids {
    inner: Https,
    ids: Vec<String>,
}

impl Wire for Ids {
    fn post_json(&mut self, url: &str, json: &str) -> Result<(u16, String), String> {
        let answer = self.inner.post_json(url, json)?;
        if let Some(id) = serde_json::from_str::<serde_json::Value>(&answer.1)
            .ok()
            .and_then(|v| v.pointer("/account/id")?.as_str().map(str::to_owned))
        {
            self.ids.push(id);
        }
        Ok(answer)
    }
    fn get_bearer(&mut self, url: &str, token: &str) -> Result<(u16, String), String> {
        self.inner.get_bearer(url, token)
    }
    fn post_bearer(&mut self, url: &str, token: &str) -> Result<(u16, String), String> {
        self.inner.post_bearer(url, token)
    }
}

/// Wait for the driver to leave a state, failing early if the played browser gave up.
fn settled(
    a: &Account,
    done: impl Fn(&AccountView) -> bool,
    log: &Arc<Mutex<Vec<String>>>,
) -> AccountView {
    let deadline = Instant::now() + WAIT + Duration::from_secs(10);
    while Instant::now() < deadline {
        let v = a.view();
        if done(&v) {
            return v;
        }
        if log
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.starts_with("the browser gave up"))
        {
            print_log(log);
            panic!("the played browser gave up while the app was {v:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    a.view()
}

fn print_log(log: &Arc<Mutex<Vec<String>>>) {
    println!("--- the browser ---");
    for l in log.lock().unwrap().iter() {
        println!("  {l}");
    }
}

/// A `Browser` for the app that plays the whole website on a thread, and the trail it leaves.
/// When it gives up it sets `cancel`, so the flow ends at once instead of waiting out `WAIT`.
fn playing(
    base: &str,
    user: &str,
    cancel: Option<Arc<AtomicBool>>,
) -> (account::Browser, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let (base, user, trail) = (base.to_owned(), user.to_owned(), log.clone());
    let browser: account::Browser = Arc::new(move |url: &str| {
        let (url, base, user, trail) = (url.to_owned(), base.clone(), user.clone(), trail.clone());
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            let mut p = Player::new(&base, &user);
            let r = p.play(&url);
            let mut t = trail.lock().unwrap();
            t.append(&mut p.trail);
            match r {
                Ok(()) => t.push("the browser reached the app".to_owned()),
                Err(e) => {
                    t.push(format!("the browser gave up: {e}"));
                    if let Some(c) = &cancel {
                        c.store(true, Ordering::SeqCst);
                    }
                }
            }
        });
        Ok(())
    });
    (browser, log)
}

/* ---------------------------------------------------------------- the browser -- */

/// What the fake provider's page calls its username field (`fakeAuthorizePage` in `src/pages.ts`).
const USERNAME_FIELD: &str = "username";

struct Page {
    url: String,
    status: u16,
    location: Option<String>,
    /// The page's `Referrer-Policy`, which decides the `Origin` of a form posted from it.
    referrer_policy: Option<String>,
    body: String,
}

/// A person with a browser, minus the rendering: GETs, form POSTs with the `Origin` a browser would
/// send, redirects and cookies, all by hand so each one is visible and nothing is followed that
/// should not be.
struct Player {
    agent: ureq::Agent,
    /// The Worker's origin, and the only place cookies are sent.
    origin: String,
    /// Name to value. Kept whatever their `Secure` flag says: a real browser treats `localhost` as
    /// a secure context and sends `__Host-` cookies there, and a cookie jar that dropped them over
    /// `http://localhost` would break the flow in a way no browser does.
    cookies: BTreeMap<String, String>,
    user: String,
    trail: Vec<String>,
}

impl Player {
    fn new(base: &str, user: &str) -> Player {
        Player {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(20)))
                .http_status_as_error(false)
                .max_redirects(0)
                .build()
                .new_agent(),
            origin: origin_of(base),
            cookies: BTreeMap::new(),
            user: user.to_owned(),
            trail: Vec::new(),
        }
    }

    fn get(&mut self, url: &str) -> Result<Page, String> {
        let mut rb = self.agent.get(url);
        if let Some(c) = self.cookie_header(url) {
            rb = rb.header("Cookie", &c);
        }
        let r = rb
            .call()
            .map_err(|e| format!("GET {}: {e}", path_of(url)))?;
        Ok(self.read("GET", url, r))
    }

    fn post(
        &mut self,
        url: &str,
        origin: &str,
        fields: &[(String, String)],
    ) -> Result<Page, String> {
        let mut rb = self.agent.post(url).header("Origin", origin);
        if let Some(c) = self.cookie_header(url) {
            rb = rb.header("Cookie", &c);
        }
        let form: Vec<(&str, &str)> = fields
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let r = rb
            .send_form(form)
            .map_err(|e| format!("POST {}: {e}", path_of(url)))?;
        Ok(self.read("POST", url, r))
    }

    fn read(&mut self, method: &str, url: &str, mut r: ureq::http::Response<ureq::Body>) -> Page {
        if url.starts_with(&self.origin) {
            let set: Vec<String> = r
                .headers()
                .get_all("set-cookie")
                .iter()
                .filter_map(|v| v.to_str().ok().map(str::to_owned))
                .collect();
            for s in set {
                self.keep_cookie(&s);
            }
        }
        let status = r.status().as_u16();
        let location = r
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let referrer_policy = r
            .headers()
            .get("referrer-policy")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = r.body_mut().read_to_string().unwrap_or_default();
        self.trail
            .push(format!("{method} {status} {}", path_of(url)));
        Page {
            url: url.to_owned(),
            status,
            location,
            referrer_policy,
            body,
        }
    }

    fn keep_cookie(&mut self, set_cookie: &str) {
        let pair = set_cookie.split(';').next().unwrap_or("");
        let Some((name, value)) = pair.split_once('=') else {
            return;
        };
        let name = name.trim().to_owned();
        let gone = value.is_empty()
            || set_cookie
                .to_ascii_lowercase()
                .replace(' ', "")
                .contains("max-age=0");
        if gone {
            self.cookies.remove(&name);
        } else {
            self.cookies.insert(name, value.trim().to_owned());
        }
    }

    fn cookie_header(&self, url: &str) -> Option<String> {
        (url.starts_with(&self.origin) && !self.cookies.is_empty()).then(|| {
            self.cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    fn follow(&mut self, mut page: Page) -> Result<Page, String> {
        for _ in 0..15 {
            if !(300..400).contains(&page.status) {
                return Ok(page);
            }
            let Some(loc) = page.location.clone() else {
                return Ok(page);
            };
            let next = resolve(&page.url, &loc);
            page = self.get(&next)?;
        }
        Err("more than fifteen redirects".to_owned())
    }

    /// Submit `form`, found on `from`, as a person would: its hidden and prefilled fields, anything
    /// in `extra`, and the button whose words contain `press` (or the first button).
    fn submit(
        &mut self,
        from: &Page,
        form: &Form,
        press: Option<&str>,
        extra: &[(String, String)],
    ) -> Result<Page, String> {
        let at = from.url.as_str();
        let mut fields = form.fields.clone();
        for (k, v) in extra {
            match fields.iter_mut().find(|(n, _)| n == k) {
                Some(f) => f.1 = v.clone(),
                None => fields.push((k.clone(), v.clone())),
            }
        }
        let button = match press {
            Some(word) => form.buttons.iter().find(|b| {
                b.label.to_lowercase().contains(word) || b.value.to_lowercase().contains(word)
            }),
            None => form.buttons.first(),
        };
        if let Some(Button {
            name: Some(n),
            value,
            ..
        }) = button
        {
            fields.push((n.clone(), value.clone()));
        }
        let action = if form.action.is_empty() {
            at.to_owned()
        } else {
            resolve(at, &form.action)
        };
        let page = if form.method.eq_ignore_ascii_case("post") {
            let origin = origin_header(at, from.referrer_policy.as_deref(), &action);
            self.post(&action, &origin, &fields)?
        } else {
            let q = fields
                .iter()
                .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
                .collect::<Vec<_>>()
                .join("&");
            let sep = if action.contains('?') { '&' } else { '?' };
            self.get(&format!("{action}{sep}{q}"))?
        };
        self.follow(page)
    }

    /// From the app's URL to the app's listener, pressing what a person presses.
    fn play(&mut self, app_url: &str) -> Result<(), String> {
        listener_port(app_url).ok_or_else(|| "the app's URL carries no port".to_owned())?;
        let first = self.get(app_url)?;
        let mut page = self.follow(first)?;
        for _ in 0..12 {
            if at_the_app(&page.url, app_url) {
                return if page.status == 200 && page.body.contains("close this tab") {
                    Ok(())
                } else {
                    Err(format!("the app's listener answered {}", page.status))
                };
            }
            if page.status >= 400 {
                return Err(format!(
                    "{} answered {}: {}",
                    path_of(&page.url),
                    page.status,
                    snippet(&page.body)
                ));
            }
            let found = forms(&page.body);
            let at = page.url.clone();

            /* THE CONFIRM PAGE: "Sign Grimoire on this computer in as NAME", Continue.
             *
             * One form POSTs to /account/app/approve with two buttons, `decision=approve`
             * ("Continue") and `decision=deny` ("Cancel"), so the word picks the button. "Use a
             * different account" is a separate form to /account/sign-out. */
            if let Some(f) = found.iter().find(|f| f.action.contains("/app/approve")) {
                page = self.submit(&page, f, Some("continue"), &[])?;
                continue;
            }

            /* THE FAKE PROVIDER: a username, and submit. */
            if let Some(f) = found
                .iter()
                .find(|f| f.fields.iter().any(|(k, _)| k == USERNAME_FIELD))
            {
                let extra = [(USERNAME_FIELD.to_owned(), self.user.clone())];
                page = self.submit(&page, f, None, &extra)?;
                continue;
            }

            /* THE PROVIDER CHOOSER: whatever leads to /account/start/fake. The Worker's chooser
             * is plain links carrying the app's query; a form is handled too. */
            if let Some(f) = found.iter().find(|f| f.action.contains("/start/fake")) {
                page = self.submit(&page, f, None, &[])?;
                continue;
            }
            if let Some(href) = link_to(&page.body, "/start/fake") {
                let next = resolve(&at, &href);
                let p = self.get(&next)?;
                page = self.follow(p)?;
                continue;
            }

            return Err(format!(
                "nothing this browser knows how to press on {} ({}): {}",
                path_of(&at),
                page.status,
                snippet(&page.body)
            ));
        }
        Err("gave up after twelve pages".to_owned())
    }
}

#[derive(Clone)]
struct Form {
    method: String,
    action: String,
    fields: Vec<(String, String)>,
    buttons: Vec<Button>,
}

#[derive(Clone)]
struct Button {
    name: Option<String>,
    value: String,
    label: String,
}

/// An attribute's value from inside a tag, quoted either way or bare.
fn attr(tag: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(?i)\s{name}\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#
    ))
    .ok()?;
    let c = re.captures(tag)?;
    let v = c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3))?;
    Some(unescape(v.as_str()))
}

/// Every form on a page, with the fields a submit would carry and the buttons it offers.
fn forms(html: &str) -> Vec<Form> {
    let form_re = Regex::new(r"(?is)<form\b([^>]*)>(.*?)</form>").expect("regex");
    let input_re = Regex::new(r"(?is)<input\b([^>]*)>").expect("regex");
    let button_re = Regex::new(r"(?is)<button\b([^>]*)>(.*?)</button>").expect("regex");
    let tags_re = Regex::new(r"(?s)<[^>]*>").expect("regex");
    form_re
        .captures_iter(html)
        .map(|c| {
            let open = format!(" {}", &c[1]);
            let inner = &c[2];
            let mut fields = Vec::new();
            let mut buttons = Vec::new();
            for i in input_re.captures_iter(inner) {
                let tag = format!(" {}", &i[1]);
                let ty = attr(&tag, "type")
                    .unwrap_or_else(|| "text".to_owned())
                    .to_ascii_lowercase();
                let name = attr(&tag, "name");
                let value = attr(&tag, "value").unwrap_or_default();
                if ty == "submit" {
                    buttons.push(Button {
                        name,
                        label: value.clone(),
                        value,
                    });
                    continue;
                }
                let Some(name) = name else { continue };
                let unchecked = matches!(ty.as_str(), "checkbox" | "radio")
                    && !tag.to_ascii_lowercase().contains("checked");
                if !unchecked {
                    fields.push((name, value));
                }
            }
            for b in button_re.captures_iter(inner) {
                let tag = format!(" {}", &b[1]);
                let ty = attr(&tag, "type").unwrap_or_else(|| "submit".to_owned());
                if !ty.eq_ignore_ascii_case("submit") {
                    continue;
                }
                buttons.push(Button {
                    name: attr(&tag, "name"),
                    value: attr(&tag, "value").unwrap_or_default(),
                    label: tags_re.replace_all(&b[2], "").trim().to_owned(),
                });
            }
            Form {
                method: attr(&open, "method").unwrap_or_else(|| "get".to_owned()),
                action: attr(&open, "action").unwrap_or_default(),
                fields,
                buttons,
            }
        })
        .collect()
}

/// The first `href` on the page that contains `needle`.
fn link_to(html: &str, needle: &str) -> Option<String> {
    let re = Regex::new(r#"(?i)href\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("regex");
    let found = re
        .captures_iter(html)
        .filter_map(|c| c.get(1).or_else(|| c.get(2)).map(|m| unescape(m.as_str())))
        .find(|h| h.contains(needle));
    found
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn origin_of(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    format!("{scheme}://{authority}")
}

/// The path of a URL with its query and fragment cut off. What the trail prints.
fn path_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.find('/').map_or("/", |i| &rest[i..]);
    path.split(['?', '#']).next().unwrap_or(path).to_owned()
}

fn resolve(current: &str, loc: &str) -> String {
    if loc.starts_with("http://") || loc.starts_with("https://") {
        return loc.to_owned();
    }
    if loc.starts_with("//") {
        let scheme = current.split_once("://").map_or("http", |(s, _)| s);
        return format!("{scheme}:{loc}");
    }
    if loc.starts_with('/') {
        return format!("{}{loc}", origin_of(current));
    }
    let path = current.split(['?', '#']).next().unwrap_or(current);
    let dir = path.rfind('/').map_or(path, |i| &path[..=i]);
    format!("{dir}{loc}")
}

/// The `Origin` a browser sends with a form POST from the page at `page_url` to `action`.
///
/// THE PAGE'S `Referrer-Policy` DECIDES IT, which is the Fetch standard's "serializing a request
/// origin": `no-referrer` makes it `null` on every POST, `same-origin` makes it `null` across
/// origins, and the `strict-*` and `no-referrer-when-downgrade` policies make it `null` from https
/// to http. Measured in Chromium on 2026-09-12 (a same-origin form posting to an echo server):
/// `no-referrer` sent `null`; `same-origin`, `strict-origin-when-cross-origin` and no policy sent
/// the page's origin. A harness that always sent the right `Origin` passed against a Worker whose
/// every POST a real browser was refused.
fn origin_header(page_url: &str, policy: Option<&str>, action: &str) -> String {
    let (from, to) = (origin_of(page_url), origin_of(action));
    /* A policy header can list several; the last one a browser knows wins. */
    let policy = policy
        .and_then(|p| p.split(',').map(str::trim).rfind(|t| !t.is_empty()))
        .unwrap_or("")
        .to_ascii_lowercase();
    let downgrade = from.starts_with("https://") && !to.starts_with("https://");
    let null = match policy.as_str() {
        "no-referrer" => true,
        "same-origin" => from != to,
        "no-referrer-when-downgrade" | "strict-origin" | "strict-origin-when-cross-origin" => {
            downgrade
        }
        _ => false,
    };
    if null {
        "null".to_owned()
    } else {
        from
    }
}

/// THE PLAYED BROWSER SENDS THE `Origin` A REAL BROWSER SENDS, NOT THE ONE THE WORKER WANTS.
///
/// No network, so not ignored. WHAT MUTATION MAKES THIS RED: `origin_header` returning the page's
/// origin whatever the policy, which is the harness that could not see the Worker's 403.
#[test]
fn a_form_post_carries_the_origin_a_browser_would() {
    let page = "http://127.0.0.1:8787/account/fake/authorize?state=S";
    let same = "http://127.0.0.1:8787/account/fake/authorize";
    let cross = "http://127.0.0.1:51234/callback";
    let ours = "http://127.0.0.1:8787";
    assert_eq!(origin_header(page, Some("no-referrer"), same), "null");
    assert_eq!(origin_header(page, Some("same-origin"), same), ours);
    assert_eq!(origin_header(page, Some("same-origin"), cross), "null");
    assert_eq!(
        origin_header(page, Some("strict-origin-when-cross-origin"), same),
        ours
    );
    assert_eq!(origin_header(page, None, same), ours);
    assert_eq!(
        origin_header(page, Some("same-origin, no-referrer"), same),
        "null"
    );
    assert_eq!(
        origin_header(
            "https://ragnarok.systems/account",
            Some("strict-origin"),
            "http://ragnarok.systems/account/sign-out"
        ),
        "null"
    );
}

/// The `port` the app put in the URL it opened.
fn listener_port(app_url: &str) -> Option<u16> {
    app_url
        .split_once('?')?
        .1
        .split(['&', '#'])
        .find_map(|p| p.strip_prefix("port="))?
        .parse()
        .ok()
}

/// Whether `url` is the app's own listener: `http://127.0.0.1:P/` with `P` from the app's URL.
///
/// BY PORT, NOT BY HOST. `wrangler dev` serves the Worker on 127.0.0.1 as well, and the first live
/// run, which matched any `http://127.0.0.1:` URL, took the Worker's chooser for the listener.
fn at_the_app(url: &str, app_url: &str) -> bool {
    listener_port(app_url).is_some_and(|p| url.starts_with(&format!("http://127.0.0.1:{p}/")))
}

/// THE PLAYED BROWSER TELLS THE APP'S LISTENER FROM A WORKER THAT IS ALSO ON 127.0.0.1.
///
/// No network, so not ignored: this is the harness's own rule. WHAT MUTATION MAKES THIS RED:
/// `at_the_app` matching on the host alone, which is how the first live run failed.
#[test]
fn the_played_browser_knows_the_app_by_its_port() {
    let app = "http://127.0.0.1:8787/account/app?challenge=C&port=51234&state=S";
    assert!(at_the_app(
        "http://127.0.0.1:51234/callback?code=x&state=S",
        app
    ));
    for not_the_app in [
        app,
        "http://127.0.0.1:8787/account/fake/authorize?state=S",
        "http://127.0.0.1:512345/callback?code=x&state=S",
        "http://127.0.0.2:51234/callback?code=x&state=S",
    ] {
        assert!(
            !at_the_app(not_the_app, app),
            "{not_the_app} was taken for the app's listener"
        );
    }
    let no_port = "http://127.0.0.1:8787/account/app?challenge=C&state=S";
    assert!(!at_the_app("http://127.0.0.1:51234/callback", no_port));
}

fn snippet(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(600)
        .collect()
}
