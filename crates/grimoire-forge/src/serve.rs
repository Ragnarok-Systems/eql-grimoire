//! `grimoire serve` — the engine over HTTP, for development.
//!
//! **This is a transport, not a second engine.** Every request lands in the same
//! [`grimoire_wasm::dispatch`] the wasm module wraps, so a page driven this way computes
//! exactly what the shipped page computes. The rule the project actually cares about — one
//! implementation of the maths — is untouched; what changes is how the browser reaches it.
//!
//! Why it exists: the `wasm32-unknown-unknown` target cannot be installed everywhere, and a
//! UI that can only be looked at in screenshots is a UI nobody has used. With this running,
//! `app.html?engine=http://127.0.0.1:8787` is the real thing, clickable.
//!
//! It also serves the `web/` directory, so there is one command instead of two.
//!
//! Hand-rolled HTTP because this is a dev tool on localhost and pulling a web framework into
//! the workspace for it would be a poor trade. It speaks the minimum: POST for the engine,
//! GET for files, CORS for either.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

pub fn run(args: &[String]) -> Result<(), String> {
    let port: u16 = crate::flag(args, "--port")
        .and_then(|p| p.to_string_lossy().parse().ok())
        .unwrap_or(8787);
    let root: PathBuf = crate::flag(args, "--root").unwrap_or_else(|| PathBuf::from("web"));
    let corpus = match crate::flag(args, "--corpus") {
        Some(p) => Some(std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?),
        None => {
            let guess = root.join("corpus.grim");
            std::fs::read(&guess).ok()
        }
    };
    let corpus = corpus.map(serde_json::Value::from);

    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    println!("grimoire serve — http://127.0.0.1:{port}/app.html");
    println!("  engine at POST /engine   files from {}", root.display());
    if corpus.is_none() {
        println!("  no corpus found — requests must carry their own bytes");
    }

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                if let Err(e) = handle(s, &root, corpus.as_ref()) {
                    eprintln!("  {e}");
                }
            }
            Err(e) => eprintln!("  accept: {e}"),
        }
    }
    Ok(())
}

fn handle(
    mut stream: TcpStream,
    root: &Path,
    corpus: Option<&serde_json::Value>,
) -> Result<(), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

    let mut request_line = String::new();
    if reader
        .read_line(&mut request_line)
        .map_err(|e| e.to_string())?
        == 0
    {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();

    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
    }

    if method == "OPTIONS" {
        return reply(&mut stream, 204, "text/plain", b"");
    }

    if method == "POST" && target.starts_with("/engine") {
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&body).into_owned();

        // Splice the corpus in when the request left it out, exactly as `dispatch` does.
        let request = match corpus {
            Some(c) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(mut v) if v.get("corpus").map_or(true, |x| x.is_null()) => {
                    if let Some(o) = v.as_object_mut() {
                        o.insert("corpus".into(), c.clone());
                    }
                    v.to_string()
                }
                _ => text,
            },
            None => text,
        };
        let out = grimoire_wasm::dispatch::dispatch(&request);
        return reply(&mut stream, 200, "application/json", out.as_bytes());
    }

    // Static files. Path traversal is refused rather than normalised — this listens on
    // localhost, but a dev server that will serve `../../../etc/passwd` is still a dev server
    // that will serve it.
    let mut rel = target
        .split('?')
        .next()
        .unwrap_or("/")
        .trim_start_matches('/')
        .to_string();
    if rel.is_empty() {
        rel = "app.html".into();
    }
    if rel.contains("..") {
        return reply(&mut stream, 403, "text/plain", b"no");
    }
    let path = root.join(&rel);
    match std::fs::read(&path) {
        Ok(body) => reply(&mut stream, 200, mime(&path), &body),
        Err(_) => reply(&mut stream, 404, "text/plain", b"not here"),
    }
}

fn mime(p: &Path) -> &'static str {
    match p.extension().and_then(|s| s.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

fn reply(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) -> Result<(), String> {
    let head = format!(
        "HTTP/1.1 {code} {}\r\n\
         content-type: {ctype}\r\n\
         content-length: {}\r\n\
         access-control-allow-origin: *\r\n\
         access-control-allow-headers: content-type\r\n\
         access-control-allow-methods: GET, POST, OPTIONS\r\n\
         cache-control: no-store\r\n\
         connection: close\r\n\r\n",
        if code == 200 { "OK" } else { "" },
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())
}
