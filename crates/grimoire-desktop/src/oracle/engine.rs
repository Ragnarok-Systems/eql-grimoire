//! The answer engine: llama.cpp's `llama-server`, run as a child process on a loopback port.
//!
//! A PROCESS AND NOT A LINKED LIBRARY. The engine is a first-use download (the owner's ruling), so
//! the app binary carries no inference code and its size does not change; the server is MIT,
//! prebuilt by llama.cpp for every processor generation, and spoken to over plain HTTP on
//! 127.0.0.1 with the same `ureq` the rest of the app uses.
//!
//! THE PORT IS CHOSEN, NOT ASKED FOR. Finding a free port would mean opening a listener, which the
//! purity gate allows in one file only (`loopback.rs`). A random high port is tried instead, and a
//! server that exits at once (the port was taken) is started again on another.
//!
//! IT DIES WITH THE APP, AND `Drop` ALONE DID NOT MAKE THAT TRUE. The engine is owned by Oracle's
//! worker thread, and when the process exits that thread is simply stopped: no destructor runs, and
//! the first end-to-end run left a llama-server holding a gigabyte behind it. Every engine is
//! therefore put in a Windows job object that kills its members when the last handle to it closes,
//! which the kernel does as the app exits, however it exits (a crash included). `Drop` still kills
//! the child for the ordinary case of an engine being replaced while the app runs.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const SERVER_EXE: &str = if cfg!(windows) {
    "llama-server.exe"
} else {
    "llama-server"
};

/// Threads for the answer: four, or fewer on a smaller processor. Oracle runs beside the game.
pub fn threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).clamp(1, 4))
        .unwrap_or(2)
}

/// No console window for a child process on Windows.
pub fn quiet(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

pub struct Engine {
    child: Child,
    port: u16,
    agent: ureq::Agent,
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One turn of the conversation sent to the model.
pub struct Turn<'a> {
    pub role: &'a str,
    pub content: &'a str,
}

fn pick_port(attempt: u32) -> u16 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let seed =
        nanos ^ std::process::id().wrapping_mul(2_654_435_761) ^ attempt.wrapping_mul(40_503);
    49_152 + (seed % 16_000) as u16
}

impl Engine {
    /// Start the server on `model` and wait until it answers its health check.
    pub fn start(engine_dir: &Path, model: &Path) -> Result<Engine, String> {
        let exe: PathBuf = engine_dir.join(SERVER_EXE);
        if !exe.is_file() {
            return Err(format!("the engine is not at {}", exe.display()));
        }
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(2)))
            .http_status_as_error(true)
            .build()
            .new_agent();
        let mut last = String::new();
        for attempt in 0..4 {
            let port = pick_port(attempt);
            let mut cmd = Command::new(&exe);
            cmd.current_dir(engine_dir)
                .arg("-m")
                .arg(model)
                .args(["--host", "127.0.0.1", "--port", &port.to_string()])
                .args(["-t", &threads().to_string(), "-c", "4096", "--no-webui"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            quiet(&mut cmd);
            let child = cmd
                .spawn()
                .map_err(|e| format!("could not start the engine: {e}"))?;
            job::adopt(&child);
            let mut eng = Engine {
                child,
                port,
                agent: agent.clone(),
            };
            let t0 = Instant::now();
            loop {
                if let Ok(Some(st)) = eng.child.try_wait() {
                    last = format!("the engine stopped as it started ({st})");
                    break;
                }
                if eng.healthy() {
                    return Ok(eng);
                }
                if t0.elapsed() > Duration::from_secs(90) {
                    return Err("the engine did not become ready in 90 seconds".to_owned());
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        Err(last)
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn healthy(&self) -> bool {
        self.agent
            .get(&self.url("/health"))
            .call()
            .ok()
            .and_then(|r| r.into_body().read_to_string().ok())
            .is_some_and(|b| b.contains("ok"))
    }

    /// Whether the server is still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Ask for an answer, streaming each piece of text to `on_token` as it is written. Returns the
    /// whole answer.
    pub fn chat(
        &self,
        turns: &[Turn],
        max_tokens: u32,
        stop: &[&str],
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let messages: Vec<serde_json::Value> = turns
            .iter()
            .map(|t| serde_json::json!({"role": t.role, "content": t.content}))
            .collect();
        let body = serde_json::json!({
            "messages": messages,
            "temperature": 0.2,
            "max_tokens": max_tokens,
            "stop": stop,
            "stream": true,
        });
        let resp = self
            .agent
            .post(&self.url("/v1/chat/completions"))
            .config()
            .timeout_recv_body(Some(Duration::from_secs(120)))
            .build()
            .send_json(&body)
            .map_err(|e| format!("the engine refused the question: {e}"))?;
        let reader = BufReader::new(resp.into_body().into_reader());
        let mut all = String::new();
        for line in reader.lines() {
            let line = line.map_err(|e| format!("the answer broke off: {e}"))?;
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data.trim() == "[DONE]" {
                break;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            if let Some(piece) = v
                .pointer("/choices/0/delta/content")
                .and_then(|c| c.as_str())
            {
                all.push_str(piece);
                on_token(piece);
            }
        }
        Ok(all)
    }
}

/// The process-wide job that takes Oracle's engine down with the app.
mod job {
    use std::process::Child;

    /// Put `child` in the app's kill-on-close job. Best effort: a PC that refuses a job (some
    /// sandboxes do) still gets `Drop`'s kill on an orderly exit.
    pub fn adopt(child: &Child) {
        #[cfg(windows)]
        if let Some(job) = app_job() {
            let _ = assign(job, child);
        }
        #[cfg(not(windows))]
        let _ = child;
    }

    /// The handle, kept as an integer so it can live in a static. Never closed: the kernel closes
    /// it when the process ends, and that is the moment the job exists for.
    #[cfg(windows)]
    fn app_job() -> Option<isize> {
        use std::sync::OnceLock;
        static JOB: OnceLock<Option<isize>> = OnceLock::new();
        *JOB.get_or_init(|| kill_on_close().ok())
    }

    #[cfg(windows)]
    pub(super) fn kill_on_close() -> windows::core::Result<isize> {
        use windows::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: the info struct outlives the call and its size is passed with it.
        unsafe {
            let job = CreateJobObjectW(None, windows::core::PCWSTR::null())?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
            Ok(job.0 as isize)
        }
    }

    #[cfg(windows)]
    pub(super) fn assign(job: isize, child: &Child) -> windows::core::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;
        // SAFETY: both handles are live: the job is never closed, the child is borrowed.
        unsafe {
            AssignProcessToJobObject(
                HANDLE(job as *mut core::ffi::c_void),
                HANDLE(child.as_raw_handle()),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DEFECT THIS PREVENTS: AN ENGINE OUTLIVING THE APP. The first end-to-end run left a
    /// llama-server running after its process had exited, because the worker thread that owned it
    /// was stopped without its destructor running. Closing the job's last handle (what the kernel
    /// does when the app exits) must take the child down with it.
    #[cfg(windows)]
    #[test]
    fn closing_the_job_kills_the_child() {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        let job = job::kill_on_close().expect("a job");
        let mut child = Command::new("cmd")
            .args(["/c", "ping -n 30 127.0.0.1 > nul"])
            .stdout(Stdio::null())
            .spawn()
            .expect("a child");
        job::assign(job, &child).expect("assigned");
        assert!(
            matches!(child.try_wait(), Ok(None)),
            "running before the close"
        );
        // SAFETY: the job handle is live and closed exactly once, here.
        unsafe { CloseHandle(HANDLE(job as *mut core::ffi::c_void)).expect("closed") };
        let t0 = Instant::now();
        while matches!(child.try_wait(), Ok(None)) {
            assert!(
                t0.elapsed() < Duration::from_secs(5),
                "the child outlived its job"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn ports_are_high_and_threads_leave_room_for_the_game() {
        for a in 0..50 {
            let p = pick_port(a);
            assert!((49_152..65_152).contains(&p));
        }
        assert!((1..=4).contains(&threads()));
    }
}
