//! A REAL REQUEST TO THE REAL UPDATE HOST, RUN BY HAND.
//!
//! `#[ignore]` on every test, which is this crate's standing rule and is written out at the top of
//! `tests/live_twitch.rs`: `cargo test` runs on other people's machines and in CI, and a suite
//! that opens sockets to a live service fails when the service is down and cannot be trusted to be
//! measuring what it says. The unit tests in `src/updater/` drive the whole check order over bytes
//! they made themselves; this one file dials, only when somebody asks:
//!
//!     cargo test -p grimoire-desktop --test live_update_channel -- --ignored --nocapture
//!
//! # WHAT IT PROVES THAT A UNIT TEST CANNOT
//!
//! That the host resolves, that the TLS handshake completes with the rustls provider this crate
//! compiles, that a real key answers 200 and a missing one answers 404, and that whatever is
//! actually being served today meets the client that will meet it. Everything else about the
//! manifest is provable from bytes and is proved there.
//!
//! # WHAT IT EXPECTS TODAY, AND WHY THAT IS A PASS
//!
//! The bucket holds one placeholder object: a smoke-test document with `"version":
//! "0.0.0-smoketest"` and an empty `artifacts` array. It is not a signed envelope, so the client
//! refuses it as `ManifestUnsigned` before a single field of it is read. That IS the correct
//! behaviour and this test asserts it rather than skipping it, because "the client refuses what is
//! there" and "the client refuses everything" look identical until somebody writes down which one
//! is expected. When a real signed manifest is published, this assertion is the one that has to be
//! changed, deliberately, by whoever publishes it.
//!
//! IT READS AND IT WRITES NOTHING. One GET, no credential, nothing to disk.

use grimoire_desktop::updater::fetch;
use grimoire_desktop::updater::verify;

/// THE URL AND THE AGENT COME OUT OF THE SHIPPED SOURCE NOW.
///
/// THIS FILE USED TO CARRY ITS OWN COPY OF BOTH, and it said why: the poll half owned the URL and
/// had not landed, and a constant in the binary that no line of the binary reads is the defect
/// `Cargo.toml` apologises for twice. It has landed. A live test that dialled an address the
/// client does not use, with an agent the client does not build, would be proving something about
/// this file rather than about the app: the one defect this file exists to catch is the SHIPPED
/// agent and the SHIPPED URL meeting the real host, and it can only catch that by using them.
///
/// The wrapper stays so the two tests below read unchanged, and so this note has somewhere to be.
fn agent() -> ureq::Agent {
    fetch::agent()
}

#[test]
#[ignore = "dials the real update host; run by hand with --ignored"]
fn the_stable_channel_answers_and_what_it_serves_meets_the_client() {
    let url = fetch::manifest_url("stable");
    let body = agent()
        .get(&url)
        .call()
        .unwrap_or_else(|e| panic!("{url} did not answer: {e}"))
        .body_mut()
        .with_config()
        /* The manifest is a few kilobytes. The cap is here for the same reason every read in
         * `channel_art.rs` has one: a host that answers with something enormous must not be able
         * to decide how much memory this process uses. */
        .limit(256 * 1024)
        .read_to_string()
        .unwrap_or_else(|e| panic!("{url} answered with a body that would not read: {e}"));
    println!("{url} served {} bytes", body.len());

    match verify::open(&body) {
        Ok(v) => println!(
            "a signed manifest: version {}, channel {}, trusted comment {:?}",
            v.doc().version,
            v.doc().channel,
            v.trusted_comment()
        ),
        Err(no) => {
            println!("refused: {} ({})", no, no.code());
            assert_eq!(
                no.code(),
                "ManifestUnsigned",
                "the host is serving something this client refuses for a reason nobody expected. \
                 Today the only object there is the placeholder smoke-test document, which is \
                 refused as ManifestUnsigned. Anything else is either a real manifest (update \
                 this test) or a broken publish."
            );
        }
    }
}

#[test]
#[ignore = "dials the real update host; run by hand with --ignored"]
fn a_channel_nobody_publishes_is_a_404_and_not_a_silent_empty_answer() {
    /* THE NEGATIVE HALF, AND IT IS NOT DECORATION. A bucket misconfigured to answer every path
     * with an index page, or with an empty 200, would make "there is no beta channel" look
     * exactly like "the beta channel is empty", and the client would refuse forever with a
     * confusing cause. One request settles it. */
    let url = fetch::manifest_url("a-channel-that-does-not-exist");
    match agent().get(&url).call() {
        Ok(r) => panic!(
            "{url} answered {} instead of 404; the bucket is serving something for keys that do \
             not exist",
            r.status()
        ),
        Err(e) => println!("{url} refused as it should: {e}"),
    }
}

/// THE WHOLE WIRED FEATURE, END TO END, AGAINST THE REAL HOST.
///
/// # WHAT THIS PROVES THAT THE TWO ABOVE DO NOT
///
/// Those two make a request with an agent this file builds. This one starts the real `Updater`,
/// which builds the production `Https` wire, spawns the real worker, runs the real check order and
/// writes the real `state.json`, and then reads what the Settings screen would read. It is the
/// only test anywhere that exercises the join: the switch, the thread, the pump, the fight gate,
/// the signature gate and the view, in the arrangement `App` puts them in.
///
/// # IT DRIVES THE PUMP FROM THIS THREAD, EXACTLY AS `App::heartbeat` DOES
///
/// `pump` is what hands the worker the pulse and the "snapshot has finished loading" flag, and
/// without it nothing is ever due. `Pulse::Closed` is passed because no log is being read here, so
/// there is no encounter; the unit tests are what prove the gate refuses the other two.
///
/// # IT WRITES UNDER A SCRATCH ROOT AND NOT THE OWNER'S INSTALL
///
/// `Updater::spawn` takes the `Layout`, which is the whole reason it does. `Updater::start` would
/// reach `%LOCALAPPDATA%\eql-grimoire` and leave a `state.json` behind on the machine of whoever
/// ran this by hand.
///
/// # WHAT IT EXPECTS TODAY
///
/// The same placeholder the first test meets, reaching the client through the real path: a refusal
/// whose cause is `ManifestUnsigned`, surfaced as a `Phase::Refused` with a sentence and a
/// `Failure` carrying the stable word. When a real signed manifest is published this is the second
/// assertion that has to be changed on purpose.
#[test]
#[ignore = "dials the real update host; run by hand with --ignored"]
fn the_wired_updater_checks_the_real_channel_and_reports_what_it_found() {
    use grimoire_desktop::fights::Pulse;
    use grimoire_desktop::updater::install::Layout;
    use grimoire_desktop::updater::run::{self, Phase, UpdaterSettings};
    use std::time::{Duration, Instant};

    /* THE SWITCH `main` FLIPS. Nothing else in any test binary calls this, which is what keeps
     * `cargo test` off the network; this file is the one place it is deliberate. */
    run::allow_updating();

    let root = std::env::temp_dir().join(format!("grimoire-live-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let settings = UpdaterSettings::default();
    assert!(settings.enabled, "the default settings do not check at all");
    println!("checking channel {:?}", settings.channel);

    let u = run::Updater::spawn(
        Layout::at(&root),
        Box::new(grimoire_desktop::updater::fetch::Https::default()),
        verify::KEYS,
        None,
        &settings,
        run::CHECK_EVERY,
    );

    /* The give-up point covers one `HTTP_TIMEOUT` plus a tick, with room for a slow handshake. It
     * returns the instant the phase settles, so a working host pays none of it. */
    let end = Instant::now() + Duration::from_secs(40);
    while Instant::now() < end {
        u.pump(Pulse::Closed, true, &settings);
        if !matches!(u.view().phase, Phase::NeverChecked | Phase::Checking) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let view = u.view();
    println!("phase: {:?}", view.phase);
    println!("last check: {:?}", view.last_check);
    if let Some(f) = &view.failure {
        println!("failure: {} / {} / {:?}", f.code, f.sentence, f.version);
    }
    assert!(
        view.last_check.is_some(),
        "the worker never finished a check, so nothing below means anything. Phase was {:?}",
        view.phase
    );
    assert!(
        view.problem.is_none(),
        "the worker could not start: {:?}",
        view.problem
    );

    match &view.phase {
        Phase::Refused { why, .. } => {
            let f = view
                .failure
                .as_ref()
                .expect("a refusal with no failure recorded beside it");
            assert_eq!(
                f.code, "ManifestUnsigned",
                "the host is serving something this client refuses for a reason nobody expected. \
                 Today the only object there is the placeholder smoke-test document. Anything \
                 else is either a real manifest (update this test) or a broken publish: {why}"
            );
        }
        other => panic!(
            "the update channel served something this test was not written for: {other:?}. If a \
             real manifest has been published, this is the assertion to change on purpose."
        ),
    }

    drop(u);
    let _ = std::fs::remove_dir_all(&root);
}
