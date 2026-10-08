//! End to end, against the real internet and the real engine, when `GRIMOIRE_ORACLE_E2E` names a
//! folder: download (resuming any `.part` already there), start the engine, answer questions.
//! Skips (passes) when it is not set, so CI downloads nothing.

use grimoire_desktop::oracle::{Cmd, Dirs, Oracle, Phase};
use std::time::{Duration, Instant};

fn wait(o: &Oracle, until: impl Fn(&Phase, usize) -> bool, secs: u64) {
    let t0 = Instant::now();
    let mut last = String::new();
    loop {
        let (p, h) = o.view();
        let now = format!("{p:?}");
        if now != last && !matches!(p, Phase::Downloading { .. }) {
            eprintln!("  phase {now}");
            last = now;
        }
        if let Phase::Failed(e) = &p {
            panic!("failed: {e}");
        }
        if until(&p, h.iter().filter(|x| x.done).count()) {
            return;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "timed out at {p:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn download_start_and_answer() {
    let Ok(root) = std::env::var("GRIMOIRE_ORACLE_E2E") else {
        eprintln!("GRIMOIRE_ORACLE_E2E not set; skipped");
        return;
    };
    let mut o = Oracle::new(Some(Dirs { root: root.into() }));
    o.send(Cmd::Download);
    wait(&o, |p, _| *p == Phase::Ready, 3600);
    let qs = [
        "what level is Fallen Pass?",
        "who gives the quest A Sweet Treat?",
        "what drops the Acid-Washed Skinsuit?",
        "which zones border Ail'Vorith?",
        "how do I tame a dragon in Ail'Vorith?",
        "what is the phone number for Monsters & Memories customer support?",
        "how does a necromancer get their first zombie pet?",
    ];
    for (n, q) in qs.iter().enumerate() {
        o.send(Cmd::Ask((*q).to_owned()));
        wait(&o, |_, done| done > n, 300);
        let (_, h) = o.view();
        let x = &h[n];
        eprintln!(
            "\nQ: {}\nA: {}\n   ({:.1}s) check {:?}",
            x.question, x.answer, x.seconds, x.check
        );
        for (i, s) in x.sources.iter().enumerate() {
            eprintln!("   [{}] {}", i + 1, s.page);
        }
        assert!(x.error.is_none(), "{:?}", x.error);
        assert!(!x.answer.is_empty());
    }
}
