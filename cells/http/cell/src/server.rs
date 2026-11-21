//! The listener: a small HTTP/1.1 server (keep-alive, one thread per connection). This file is the only place in
//! the repository that opens a listening socket (`cargo cell check` fails on any other).
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

pub struct Req {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
}

pub struct Resp {
    pub status: u16,
    pub ctype: String,
    pub body: Vec<u8>,
    /// `true` for bytes named by their sha256 (`/_c/<sha256>`): `Cache-Control: immutable`.
    pub immutable: bool,
}

impl Resp {
    pub fn json(v: &serde_json::Value) -> Resp {
        Resp { status: 200, ctype: "application/json".into(), body: v.to_string().into_bytes(), immutable: false }
    }
    pub fn status(s: u16, msg: &str) -> Resp {
        Resp { status: s, ctype: "text/plain; charset=utf-8".into(), body: msg.as_bytes().to_vec(), immutable: false }
    }
}

pub fn serve<H: Fn(Req) -> Resp + Send + Sync + 'static>(addr: &str, h: H) {
    let l = TcpListener::bind(addr).unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    let h = Arc::new(h);
    for c in l.incoming().flatten() {
        let h = h.clone();
        std::thread::spawn(move || {
            let _ = conn(c, &*h);
        });
    }
}

fn conn<H: Fn(Req) -> Resp>(c: TcpStream, h: &H) -> std::io::Result<()> {
    c.set_nodelay(true).ok();
    let mut w = c.try_clone()?;
    let mut r = BufReader::new(c);
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let path = parts.next().unwrap_or("/").to_string();
        let mut len = 0usize;
        let mut close = false;
        loop {
            let mut hl = String::new();
            if r.read_line(&mut hl)? == 0 {
                return Ok(());
            }
            let hl = hl.trim_end();
            if hl.is_empty() {
                break;
            }
            let lower = hl.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
            if lower.starts_with("connection:") && lower.contains("close") {
                close = true;
            }
        }
        let mut body = vec![0; len];
        r.read_exact(&mut body)?;
        let resp = if method == "OPTIONS" { Resp::status(204, "") } else { h(Req { method, path, body }) };
        let reason = match resp.status {
            200 => "OK",
            204 => "No Content",
            403 => "Forbidden",
            404 => "Not Found",
            _ => "Error",
        };
        let head = format!(
            "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: content-type\r\nCache-Control: {}\r\n\r\n",
            resp.status, resp.ctype, resp.body.len(), if resp.immutable { "public, max-age=31536000, immutable" } else { "no-store" }
        );
        w.write_all(head.as_bytes())?;
        w.write_all(&resp.body)?;
        if close {
            return Ok(());
        }
    }
}

pub fn ctype(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" | "jsonl" => "application/json",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
