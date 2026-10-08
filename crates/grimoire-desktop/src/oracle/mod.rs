//! Oracle: ask a question about Monsters & Memories, get an answer written from the wiki bundle,
//! with every source numbered and linked. Runs on this PC, on the processor, and costs nothing.
//!
//! THE SHAPE: the screen holds an [`Oracle`]; it owns one worker thread, started on the first
//! thing the reader asks for, which owns the engine process and the passage index. The screen
//! sends a [`Cmd`] and reads a [`Shared`] snapshot each frame, so the UI never waits on a download,
//! a model load or an answer.
//!
//! CONSENT FIRST. Nothing is downloaded until the reader presses the button on the consent card,
//! which names both files, their sizes, where they come from and their licences. The owner's rule:
//! the model is never part of the app download.
//!
//! THE PROMPT IS `mnm-lore/src/oracle.js`'s, word for word, because that is the prompt the
//! benchmark measured (71/78 with this model).

pub mod engine;
pub mod fetch;
pub mod retrieve;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

pub const NOT_FOUND: &str = "The wiki doesn't say.";

pub fn system_prompt() -> String {
    [
        "You are Oracle, a guide to the game Monsters & Memories inside the Grimoire app.",
        "Answer ONLY from the numbered sources in the user message. They come from the community wiki.",
        "Rules:",
        "- Put the source number in square brackets after each fact, like [1] or [2][3].",
        &format!("- If the sources do not answer the question, reply exactly: {NOT_FOUND}"),
        "- Never use outside knowledge of other games (EverQuest included) to fill gaps.",
        "- Be brief: two to five sentences, or a short list. Use the names exactly as the sources write them.",
        "- If sources disagree, say so and cite both.",
    ]
    .join("\n")
}

/// The user turn: the question, the numbered sources, the question again. Asking it first as well
/// as last measured 73/80 against 69/80 for last only (2026-10-08, this engine, keyword retrieval).
pub fn build_prompt(question: &str, passages: &[&retrieve::Passage]) -> String {
    let sources: Vec<String> = passages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let head = match &p.section {
                Some(s) => format!("{} > {s}", p.page),
                None => p.page.clone(),
            };
            format!("[{}] {head}\n{}", i + 1, p.text)
        })
        .collect();
    format!(
        "Question: {question}\n\nSources:\n\n{}\n\nQuestion: {question}",
        sources.join("\n\n")
    )
}

/// Where generation stops: a new line opening with a source number.
///
/// THE 1.2B MODEL MIRRORS THE NUMBERED SOURCE LIST. It writes its answer on the first line, then
/// one line per source restating it (59 of 80 answers did, 2026-10-08), often ending "The wiki
/// doesn't say." because the later sources are off topic. Six prompt wordings were measured and
/// none stopped it; stopping here keeps the answer and drops the mirror. A real answer cites after
/// its facts, never at the start of a line. The same stop is in `mnm-lore/src/oracle.js`.
pub const STOP: &str = "\n[";

/// A citation the model put first ("[1] Fallen Pass is ...") moves to the end of that line.
pub fn tidy_answer(answer: &str) -> String {
    let answer = answer.trim();
    let (first, rest) = match answer.split_once('\n') {
        Some((a, b)) => (a, Some(b)),
        None => (answer, None),
    };
    let mut lead = String::new();
    let mut body = first;
    loop {
        let t = body.trim_start();
        let Some(after) = t.strip_prefix('[') else {
            break;
        };
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || !after[digits.len()..].starts_with(']') {
            break;
        }
        lead.push_str(&format!("[{digits}]"));
        body = &after[digits.len() + 1..];
    }
    let mut out = if lead.is_empty() || body.trim().is_empty() {
        first.to_owned()
    } else {
        format!("{} {lead}", body.trim())
    };
    if let Some(r) = rest {
        out.push('\n');
        out.push_str(r);
    }
    out.trim().to_owned()
}

/// Whether an answer is the "doesn't say" reply, in any of the ways small models phrase it.
pub fn is_decline(answer: &str) -> bool {
    use std::sync::OnceLock;
    static RES: OnceLock<[regex::Regex; 2]> = OnceLock::new();
    let res = RES.get_or_init(|| {
        [
            regex::Regex::new(r"(?i)^(the )?(wiki|sources?|provided (sources|materials|information))( provided)? (does not|doesn't|do not|don't) (say|specify|mention|provide|cover|contain|include|state)").expect("decline pattern"),
            regex::Regex::new(r"(?i)^(i (can't|cannot|could not) find|there is no information|no information)").expect("decline pattern"),
        ]
    });
    let a: String = answer.trim().chars().take(160).collect();
    a.starts_with(NOT_FOUND) || res.iter().any(|r| r.is_match(&a))
}

/// The citations of an answer against its source count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Check {
    pub cited: Vec<usize>,
    pub invalid: Vec<usize>,
    pub declined: bool,
    /// A decline, or at least one citation and none pointing at a source that does not exist.
    pub grounded: bool,
}

pub fn check_citations(answer: &str, sources: usize) -> Check {
    let mut cited = Vec::new();
    let mut invalid = Vec::new();
    let b = answer.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'[' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b']' {
                if let Ok(n) = answer[i + 1..j].parse::<usize>() {
                    let list = if (1..=sources).contains(&n) {
                        &mut cited
                    } else {
                        &mut invalid
                    };
                    if !list.contains(&n) {
                        list.push(n);
                    }
                }
                i = j;
            }
        }
        i += 1;
    }
    cited.sort_unstable();
    let declined = is_decline(answer);
    Check {
        grounded: declined || (!cited.is_empty() && invalid.is_empty()),
        cited,
        invalid,
        declined,
    }
}

/* ---------------------------------------------------------------- the worker -- */

/// A source shown under an answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub page: String,
    pub section: Option<String>,
    pub url: Option<String>,
    pub kind: Option<String>,
}

/// One question and its answer, as it stands.
#[derive(Clone, Debug, Default)]
pub struct Exchange {
    pub question: String,
    pub answer: String,
    pub sources: Vec<Source>,
    pub done: bool,
    pub check: Option<Check>,
    pub error: Option<String>,
    pub seconds: f32,
}

/// What Oracle is doing.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// The engine or the model is not on this PC; the consent card shows.
    NeedFiles,
    Downloading {
        what: &'static str,
        done: u64,
        total: u64,
    },
    /// Files present; nothing started yet, or the engine is loading the model.
    Idle,
    Starting,
    Ready,
    Answering,
    Failed(String),
}

#[derive(Debug)]
pub struct Shared {
    pub phase: Phase,
    pub history: Vec<Exchange>,
}

pub enum Cmd {
    Download,
    Ask(String),
}

/// Where Oracle keeps its files: `<data root>/oracle/{engine,models}`.
#[derive(Clone, Debug)]
pub struct Dirs {
    pub root: PathBuf,
}

impl Dirs {
    pub fn platform() -> Option<Dirs> {
        crate::updater::install::Layout::platform().map(|l| Dirs {
            root: l.root().join("oracle"),
        })
    }
    pub fn downloads(&self) -> PathBuf {
        self.root.join("downloads")
    }
    pub fn engine(&self) -> PathBuf {
        self.root.join("engine")
    }
    pub fn models(&self) -> PathBuf {
        self.root.join("models")
    }
    pub fn model(&self) -> PathBuf {
        self.models().join(fetch::MODEL.file)
    }
    pub fn ready(&self) -> bool {
        self.engine().join(engine::SERVER_EXE).is_file()
            && fetch::have(&self.models(), &fetch::MODEL)
    }
}

/// The screen's handle on Oracle. Idle until the first command; dropping it stops the worker and
/// the engine with it.
pub struct Oracle {
    dirs: Option<Dirs>,
    shared: Arc<Mutex<Shared>>,
    tx: Option<mpsc::Sender<Cmd>>,
    stop: Arc<AtomicBool>,
}

impl Default for Oracle {
    fn default() -> Self {
        Oracle::new(Dirs::platform())
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Oracle {
    pub fn new(dirs: Option<Dirs>) -> Oracle {
        let phase = match &dirs {
            None => {
                Phase::Failed("This PC has no local app data folder for Oracle's files.".to_owned())
            }
            Some(d) if d.ready() => Phase::Idle,
            Some(_) => Phase::NeedFiles,
        };
        Oracle {
            dirs,
            shared: Arc::new(Mutex::new(Shared {
                phase,
                history: Vec::new(),
            })),
            tx: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A copy of where things stand, for this frame.
    pub fn view(&self) -> (Phase, Vec<Exchange>) {
        let s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
        (s.phase.clone(), s.history.clone())
    }

    pub fn dirs(&self) -> Option<&Dirs> {
        self.dirs.as_ref()
    }

    pub fn send(&mut self, cmd: Cmd) {
        let Some(dirs) = self.dirs.clone() else {
            return;
        };
        if self.tx.is_none() {
            let (tx, rx) = mpsc::channel();
            let shared = self.shared.clone();
            let stop = self.stop.clone();
            let spawned = std::thread::Builder::new()
                .name("grimoire-oracle".to_owned())
                .spawn(move || worker(dirs, rx, shared, stop));
            if spawned.is_err() {
                set_phase(
                    &self.shared,
                    Phase::Failed("could not start Oracle's worker".to_owned()),
                );
                return;
            }
            self.tx = Some(tx);
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }
}

fn set_phase(shared: &Mutex<Shared>, phase: Phase) {
    shared.lock().unwrap_or_else(|p| p.into_inner()).phase = phase;
}

fn with<R>(shared: &Mutex<Shared>, f: impl FnOnce(&mut Shared) -> R) -> R {
    f(&mut shared.lock().unwrap_or_else(|p| p.into_inner()))
}

fn download(dirs: &Dirs, shared: &Mutex<Shared>, stop: &AtomicBool) -> Result<(), String> {
    let stopped = || stop.load(Ordering::Relaxed);
    if !dirs.engine().join(engine::SERVER_EXE).is_file() {
        let pin = fetch::ENGINE;
        let zip = fetch::fetch(
            &dirs.downloads(),
            &pin,
            &mut |done, total| {
                set_phase(
                    shared,
                    Phase::Downloading {
                        what: pin.what,
                        done,
                        total,
                    },
                )
            },
            &stopped,
        )?;
        fetch::unpack(&zip, &dirs.engine())?;
        let _ = std::fs::remove_file(&zip);
    }
    let pin = fetch::MODEL;
    fetch::fetch(
        &dirs.models(),
        &pin,
        &mut |done, total| {
            set_phase(
                shared,
                Phase::Downloading {
                    what: pin.what,
                    done,
                    total,
                },
            )
        },
        &stopped,
    )?;
    Ok(())
}

fn worker(dirs: Dirs, rx: mpsc::Receiver<Cmd>, shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>) {
    let mut eng: Option<engine::Engine> = None;
    let mut index: Option<retrieve::Index> = None;
    while let Ok(cmd) = rx.recv() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match cmd {
            Cmd::Download => {
                match download(&dirs, &shared, &stop) {
                    Ok(()) => set_phase(&shared, Phase::Idle),
                    Err(e) => set_phase(&shared, Phase::Failed(e)),
                }
                if dirs.ready() {
                    set_phase(&shared, Phase::Starting);
                    match engine::Engine::start(&dirs.engine(), &dirs.model()) {
                        Ok(e) => {
                            eng = Some(e);
                            set_phase(&shared, Phase::Ready);
                        }
                        Err(e) => set_phase(&shared, Phase::Failed(e)),
                    }
                }
            }
            Cmd::Ask(question) => {
                let t0 = std::time::Instant::now();
                with(&shared, |s| {
                    s.history.push(Exchange {
                        question: question.clone(),
                        ..Default::default()
                    });
                });
                let fail = |msg: String| {
                    with(&shared, |s| {
                        if let Some(x) = s.history.last_mut() {
                            x.error = Some(msg.clone());
                            x.done = true;
                        }
                        s.phase = if dirs.ready() {
                            Phase::Ready
                        } else {
                            Phase::NeedFiles
                        };
                    })
                };
                if !dirs.ready() {
                    fail("Oracle needs its engine and model first.".to_owned());
                    continue;
                }
                if index.is_none() {
                    match crate::lore::locate()
                        .ok_or_else(|| "The wiki data is not on this PC yet.".to_owned())
                        .and_then(|root| retrieve::Index::load(&root))
                    {
                        Ok(i) => index = Some(i),
                        Err(e) => {
                            fail(e);
                            continue;
                        }
                    }
                }
                if !eng.as_mut().is_some_and(|e| e.alive()) {
                    set_phase(&shared, Phase::Starting);
                    match engine::Engine::start(&dirs.engine(), &dirs.model()) {
                        Ok(e) => eng = Some(e),
                        Err(e) => {
                            eng = None;
                            fail(e);
                            continue;
                        }
                    }
                }
                set_phase(&shared, Phase::Answering);
                let Some(idx) = index.as_ref() else { continue };
                let Some(e) = eng.as_ref() else { continue };
                let passages = idx.retrieve(&question, 5, 2);
                let sources: Vec<Source> = passages
                    .iter()
                    .map(|p| Source {
                        page: p.page.clone(),
                        section: p.section.clone(),
                        url: p.url.clone(),
                        kind: p.kind.clone(),
                    })
                    .collect();
                with(&shared, |s| {
                    if let Some(x) = s.history.last_mut() {
                        x.sources = sources.clone();
                    }
                });
                let system = system_prompt();
                let user = build_prompt(&question, &passages);
                let turns = [
                    engine::Turn {
                        role: "system",
                        content: &system,
                    },
                    engine::Turn {
                        role: "user",
                        content: &user,
                    },
                ];
                let result = e.chat(&turns, 400, &[STOP], &mut |piece| {
                    with(&shared, |s| {
                        if let Some(x) = s.history.last_mut() {
                            x.answer.push_str(piece);
                        }
                    })
                });
                match result {
                    Ok(answer) => with(&shared, |s| {
                        if let Some(x) = s.history.last_mut() {
                            x.answer = tidy_answer(&answer);
                            x.check = Some(check_citations(&x.answer, sources.len()));
                            x.done = true;
                            x.seconds = t0.elapsed().as_secs_f32();
                        }
                        s.phase = Phase::Ready;
                    }),
                    Err(err) => fail(err),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citations_are_checked_like_the_js_checker() {
        let c = check_citations("Fallen Pass is level 10 to 20 [1][2]. Also [9].", 5);
        assert_eq!(c.cited, vec![1, 2]);
        assert_eq!(c.invalid, vec![9]);
        assert!(!c.grounded);
        assert!(check_citations("It borders Night Harbor [3].", 5).grounded);
        assert!(!check_citations("It borders Night Harbor.", 5).grounded);
        let d = check_citations("The wiki doesn't say. The sources cover zones.", 5);
        assert!(d.declined && d.grounded);
        assert!(is_decline("The sources do not mention that quest."));
        assert!(is_decline("I cannot find anything about it."));
        assert!(!is_decline("Guard Pippi gives the quest [1]."));
    }

    #[test]
    fn the_prompt_numbers_its_sources() {
        let a = retrieve::Passage {
            page: "Fallen Pass".into(),
            kind: None,
            section: Some("Facts".into()),
            url: None,
            text: "levels 10 to 20".into(),
        };
        let p = build_prompt("what level?", &[&a]);
        assert!(p.starts_with(
            "Question: what level?\n\nSources:\n\n[1] Fallen Pass > Facts\nlevels 10 to 20"
        ));
        assert!(p.ends_with("Question: what level?"));
        assert!(system_prompt().contains(NOT_FOUND));
        assert!(!system_prompt().contains("say what the sources do cover"));
    }

    /// DEFECT THIS PREVENTS: THE MIRRORED ANSWER. Measured 2026-10-08: line one answered, then a
    /// line per source, then "The wiki doesn't say." Cut at `STOP` and tidied, only the answer is
    /// left, cited and not a decline.
    #[test]
    fn the_mirror_is_cut_and_a_leading_citation_moves_to_the_end() {
        let mirrored = "[1] Fallen Pass is for levels 19 to 44.\n[2] Headhunter's Companion is dropped in Fallen Pass.\nThe wiki doesn't say.";
        let cut = &mirrored[..mirrored.find(STOP).unwrap()];
        let a = tidy_answer(cut);
        assert_eq!(a, "Fallen Pass is for levels 19 to 44. [1]");
        let c = check_citations(&a, 5);
        assert_eq!(c.cited, vec![1]);
        assert!(c.grounded && !c.declined);
        assert_eq!(
            tidy_answer("[1][3] Guard Pippi gives it."),
            "Guard Pippi gives it. [1][3]"
        );
        assert_eq!(
            tidy_answer("Guard Pippi gives it [1]."),
            "Guard Pippi gives it [1]."
        );
        assert_eq!(tidy_answer("[1]"), "[1]");
    }

    #[test]
    fn with_no_files_oracle_asks_first_and_a_question_says_why() {
        let d = std::env::temp_dir().join(format!("grimoire-oracle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut o = Oracle::new(Some(Dirs { root: d.clone() }));
        assert_eq!(o.view().0, Phase::NeedFiles);
        o.send(Cmd::Ask("what level is Fallen Pass?".into()));
        let t0 = std::time::Instant::now();
        loop {
            let (_, h) = o.view();
            if h.first().is_some_and(|x| x.done) {
                assert!(h[0]
                    .error
                    .as_deref()
                    .unwrap_or("")
                    .contains("engine and model"));
                break;
            }
            assert!(
                t0.elapsed().as_secs() < 10,
                "no answer to a question with no files"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            !d.join("downloads").exists(),
            "a question never downloads anything"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
