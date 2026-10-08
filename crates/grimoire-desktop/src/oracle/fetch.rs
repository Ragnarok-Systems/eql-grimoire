//! Oracle's two downloads: the llama.cpp engine and the answer model, each pinned by sha256.
//!
//! RESUMABLE. The model is 731 MB, and a player on a slow line who closes the app halfway has to
//! pick up where it stopped: the bytes go to `<name>.part` and a later attempt asks for the rest
//! with a `Range` header. A server that ignores the range answers 200 with the whole file, and the
//! part is started again rather than appended to.
//!
//! NOTHING UNVERIFIED IS EVER USED. The finished part is hashed and renamed into place only when
//! the hash is the pinned one; a mismatch deletes the part, so the next attempt starts clean.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A file Oracle needs, pinned.
#[derive(Clone, Copy, Debug)]
pub struct Pin {
    /// What the consent card calls it.
    pub what: &'static str,
    pub url: &'static str,
    /// The name it is saved under.
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub from: &'static str,
    pub licence: &'static str,
}

/// llama.cpp's own Windows build, processor only. MIT.
pub const ENGINE: Pin = Pin {
    what: "the llama.cpp answer engine",
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b11476/llama-b11476-bin-win-cpu-x64.zip",
    file: "llama-b11476-bin-win-cpu-x64.zip",
    bytes: 19_441_535,
    sha256: "a23e548c6b3525c38bcfeceaff919786ae06741857043cb670279b70100e5483",
    from: "GitHub (ggml-org/llama.cpp)",
    licence: "MIT",
};

/// The answer model the owner chose (2026-10-07): LFM2.5 1.2B, 71/78 on the CPU benchmark.
pub const MODEL: Pin = Pin {
    what: "the LFM2.5 1.2B answer model",
    url: "https://huggingface.co/LiquidAI/LFM2.5-1.2B-Instruct-GGUF/resolve/8ed288026e23958ad9dfa92d53ed773a8eee7125/LFM2.5-1.2B-Instruct-Q4_K_M.gguf",
    file: "LFM2.5-1.2B-Instruct-Q4_K_M.gguf",
    bytes: 730_895_168,
    sha256: "b1b3de114215d9507409a662a501a631095a479a419584e8a2ded6304b19b4f5",
    from: "Hugging Face (LiquidAI)",
    licence: "LFM Open License v1.0",
};

/// The embedder for meaning search: BAAI's bge-small-en-v1.5 (MIT), the model the bundle's vectors
/// were built with, as an 8-bit GGUF the same engine runs.
pub const EMBEDDER: Pin = Pin {
    what: "the bge-small search model",
    url: "https://huggingface.co/CompendiumLabs/bge-small-en-v1.5-gguf/resolve/d32f8c040ea3b516330eeb75b72bcc2d3a780ab7/bge-small-en-v1.5-q8_0.gguf",
    file: "bge-small-en-v1.5-q8_0.gguf",
    bytes: 36_806_944,
    sha256: "ec38e8da142596baa913124ae50550de284b6916bf59577ef2f0cb9660c2f514",
    from: "Hugging Face (BAAI bge-small, GGUF by CompendiumLabs)",
    licence: "MIT",
};

/// How a download is going: bytes so far of bytes in all.
pub type Progress<'a> = &'a mut dyn FnMut(u64, u64);

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(crate::watcher::HTTP_TIMEOUT))
        /* A slow line is allowed to be slow; a dead one is given up on. */
        .timeout_recv_body(Some(Duration::from_secs(60)))
        .user_agent(crate::watcher::USER_AGENT)
        .http_status_as_error(true)
        .build()
        .new_agent()
}

/// The sha256 of a file, as lower-case hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Whether `dir` already holds the pinned file (by size; the hash was checked when it arrived).
pub fn have(dir: &Path, pin: &Pin) -> bool {
    std::fs::metadata(dir.join(pin.file)).is_ok_and(|m| m.len() == pin.bytes)
}

/// Download `pin` into `dir`, resuming a part left by an earlier attempt. Returns the final path.
pub fn fetch(
    dir: &Path,
    pin: &Pin,
    progress: Progress,
    stop: &dyn Fn() -> bool,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
    let done = dir.join(pin.file);
    if have(dir, pin) {
        return Ok(done);
    }
    let part = dir.join(format!("{}.part", pin.file));
    let mut have_bytes = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have_bytes > pin.bytes {
        let _ = std::fs::remove_file(&part);
        have_bytes = 0;
    }
    if have_bytes < pin.bytes {
        let mut req = agent().get(pin.url);
        if have_bytes > 0 {
            req = req.header("Range", format!("bytes={have_bytes}-"));
        }
        let resp = req
            .call()
            .map_err(|e| format!("could not download {}: {e}", pin.what))?;
        let resumed = resp.status().as_u16() == 206;
        if !resumed {
            have_bytes = 0;
        }
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(&part)
            .map_err(|e| format!("could not write {}: {e}", part.display()))?;
        let mut body = resp
            .into_body()
            .into_with_config()
            .limit(pin.bytes + 1)
            .reader();
        let mut buf = vec![0u8; 1 << 16];
        progress(have_bytes, pin.bytes);
        loop {
            if stop() {
                return Err("stopped".to_owned());
            }
            let n = body
                .read(&mut buf)
                .map_err(|e| format!("the download of {} broke off: {e}", pin.what))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])
                .map_err(|e| format!("could not write {}: {e}", part.display()))?;
            have_bytes += n as u64;
            progress(have_bytes, pin.bytes);
        }
        out.flush().map_err(|e| e.to_string())?;
    }
    if have_bytes != pin.bytes {
        return Err(format!(
            "the download of {} ended at {have_bytes} of {} bytes; it will resume next time",
            pin.what, pin.bytes
        ));
    }
    let got = sha256_file(&part).map_err(|e| e.to_string())?;
    if got != pin.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(format!(
            "{} did not match its pinned fingerprint, so it was thrown away (got {got})",
            pin.what
        ));
    }
    std::fs::rename(&part, &done)
        .map_err(|e| format!("could not move {} into place: {e}", pin.what))?;
    Ok(done)
}

/// Unpack the engine zip into `dest`, whole: into a folder beside it first, then renamed.
///
/// WINDOWS' OWN `tar.exe` READS ZIP (it is bsdtar, in System32 since Windows 10 1803), so the app
/// carries no unzip code. Called by its full path: a `tar` earlier on the PATH (Git's GNU tar)
/// cannot read a zip.
pub fn unpack(zip: &Path, dest: &Path) -> Result<(), String> {
    let incoming = dest.with_extension("incoming");
    let _ = std::fs::remove_dir_all(&incoming);
    std::fs::create_dir_all(&incoming).map_err(|e| e.to_string())?;
    let tar = system_tar();
    let mut cmd = std::process::Command::new(&tar);
    cmd.arg("-xf").arg(zip).arg("-C").arg(&incoming);
    crate::oracle::engine::quiet(&mut cmd);
    let st = cmd
        .status()
        .map_err(|e| format!("could not run {}: {e}", tar.display()))?;
    if !st.success() {
        return Err(format!("unpacking the engine failed ({st})"));
    }
    if !incoming.join(crate::oracle::engine::SERVER_EXE).is_file() {
        return Err("the engine download holds no llama-server".to_owned());
    }
    let _ = std::fs::remove_dir_all(dest);
    std::fs::rename(&incoming, dest)
        .map_err(|e| format!("could not move the engine into place: {e}"))
}

fn system_tar() -> PathBuf {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join("tar.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_whole_and_point_at_their_sources() {
        for p in [ENGINE, MODEL, EMBEDDER] {
            assert_eq!(p.sha256.len(), 64, "{}", p.what);
            assert!(p
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
            assert!(p.url.starts_with("https://"), "{}", p.what);
            assert!(
                p.url.ends_with(p.file),
                "{}: saved under the name it is published as",
                p.what
            );
            assert!(p.bytes > 1_000_000);
        }
        assert!(
            MODEL
                .url
                .contains("/resolve/8ed288026e23958ad9dfa92d53ed773a8eee7125/"),
            "pinned by revision"
        );
    }

    #[test]
    fn a_finished_file_is_had_and_a_short_one_is_not() {
        let d = std::env::temp_dir().join(format!("grimoire-oracle-fetch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let pin = Pin {
            bytes: 3,
            file: "x.bin",
            ..ENGINE
        };
        assert!(!have(&d, &pin));
        std::fs::write(d.join("x.bin"), b"ab").unwrap();
        assert!(!have(&d, &pin));
        std::fs::write(d.join("x.bin"), b"abc").unwrap();
        assert!(have(&d, &pin));
        assert_eq!(
            sha256_file(&d.join("x.bin")).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
