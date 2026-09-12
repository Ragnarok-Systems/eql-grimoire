//! `grimoire fights` end to end, driven the way the owner drives it.
//!
//! THIS IS THE REACHABILITY GATE. Everything else in this change can be green while the command
//! is unwired from `main.rs`'s argv match, and this repository has shipped exactly that defect
//! three times in two days. So this test does not call a function: it runs the built binary,
//! passes it the real capture, and reads what came out of the pipe. If the dispatch entry is
//! deleted, the module is unlinked, or the report stops printing numbers, this goes red.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/grimoire-forge has a parent")
        .parent()
        .expect("crates/ has a parent")
        .to_path_buf()
}

fn capture() -> PathBuf {
    workspace_root().join("web/fixtures/eqlog-tail-200k.txt")
}

fn grimoire(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_grimoire"))
        .args(args)
        .current_dir(workspace_root())
        .output()
        .expect("the grimoire binary runs")
}

fn stdout_of(args: &[&str]) -> String {
    let out = grimoire(args);
    assert!(
        out.status.success(),
        "`grimoire {}` exited {:?}\n--- stderr ---\n{}",
        args.join(" "),
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn assert_has(text: &str, needle: &str) {
    assert!(
        text.contains(needle),
        "the report is missing {needle:?}\n--- report ---\n{text}"
    );
}

/// The command runs on the real capture and prints something the owner can act on.
#[test]
fn the_command_reads_the_real_capture() {
    let path = capture();
    let report = stdout_of(&["fights", path.to_str().expect("path"), "--me", "Reviir"]);

    // The coverage line, which the owner needs in order to know whether to believe the rest.
    assert_has(&report, "2385 stamped lines");
    assert_has(&report, "1978 parsed (82.9%)");
    assert_has(&report, "306 ignored (12.8%)");
    assert_has(&report, "101 spell flavour (4.2%)");
    assert_has(&report, "0 unrecognised (0.0%)");

    // The fights, named, timed, and with the reason each one ended.
    assert_has(
        &report,
        "12 fights, cut where combat goes quiet for more than 30s",
    );
    assert_has(&report, "a dry bone skeleton");
    assert_has(&report, "A tormented dead");
    assert_has(&report, "Guard Ullindin");
    assert_has(&report, "ended: quiet");
    assert_has(&report, "ended: zoned");
    assert_has(&report, "ended: the log stopped");

    // Participants with DPS, which is the whole point of the screen.
    assert_has(&report, "entity");
    assert_has(&report, "dps");
    assert_has(&report, "Tanefilo");
    assert_has(&report, "77.4 dps");

    // The footer that makes the fights checkable rather than merely asserted.
    assert_has(&report, "combat lines in the file                   1779");
    assert_has(&report, "folded into a fight                        1768");
    assert_has(&report, "damage in the file                        19707");
    assert_has(&report, "inside a fight                            19695");

    // And it must not be a table of nothing.
    assert!(
        report.lines().count() > 40,
        "the report is too short to contain four fights:\n{report}"
    );
}

/// The boundary constant is reachable from the command line, and moving it moves the answer.
#[test]
fn the_quiet_window_flag_reaches_the_aggregator() {
    let path = capture();
    let p = path.to_str().expect("path");
    let wide = stdout_of(&["fights", p, "--quiet", "30", "--fights", "0"]);
    let narrow = stdout_of(&["fights", p, "--quiet", "28", "--fights", "0"]);
    let wider = stdout_of(&["fights", p, "--quiet", "168", "--fights", "0"]);
    assert_has(
        &wide,
        "12 fights, cut where combat goes quiet for more than 30s",
    );
    assert_has(
        &narrow,
        "13 fights, cut where combat goes quiet for more than 28s",
    );
    assert_has(
        &wider,
        "11 fights, cut where combat goes quiet for more than 168s",
    );
}

/// `--me` folds the owner's two names into one person, and the report says which it did.
#[test]
fn naming_the_character_changes_the_table_and_says_so() {
    let path = capture();
    let p = path.to_str().expect("path");
    /* THREE FIGHTS AND NOT ONE. The capture opens on the four line remnant of the pull it was cut
     * inside, and the reader is not in it: asking for one fight now asks for the one fight of the
     * fifteen that cannot show whether his name was folded. */
    let anonymous = stdout_of(&["fights", p, "--fights", "3", "--top", "30"]);
    let named = stdout_of(&[
        "fights", p, "--fights", "3", "--top", "30", "--me", "Reviir",
    ]);

    assert_has(&anonymous, "no character name given");
    assert_has(&named, "reading as Reviir, folded together with `you`");
    assert!(
        anonymous.contains("Reviir"),
        "unnamed, Reviir should still be his own row:\n{anonymous}"
    );
    assert!(
        !named.lines().any(|l| l.trim_start().starts_with("Reviir ")),
        "named, Reviir should have been folded into `you`:\n{named}"
    );
    assert_eq!(
        anonymous.matches("12 fights").count(),
        named.matches("12 fights").count(),
        "folding a name must not move a fight boundary"
    );
}

/// A command that does not appear in `--help` is a command nobody finds.
#[test]
fn the_command_is_in_the_help_text() {
    let help = stdout_of(&["--help"]);
    assert_has(&help, "grimoire fights <eqlog.txt>...");
    for flag in ["--me NAME", "--quiet N", "--fights N", "--top N"] {
        assert_has(&help, flag);
    }
}

/// Called with nothing to read, it says so and fails, rather than printing an empty table that
/// looks like an answer.
#[test]
fn with_no_log_it_fails_loudly() {
    let out = grimoire(&["fights"]);
    assert!(
        !out.status.success(),
        "an empty invocation reported success"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("give me at least one eqlog"),
        "no explanation on stderr: {stderr}"
    );
}
