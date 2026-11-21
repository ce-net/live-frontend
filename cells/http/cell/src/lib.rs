//! http.serve@1: the one cell that speaks HTTP (FULLSTACK section 15.3 of docs/DESIGN.md).
//!
//! | Route | What it does | Chain |
//! |---|---|---|
//! | `GET /` | `page.compose Resolve`, the shell's page by hash, then `Render.Html` of each public view inlined | the anonymous chain |
//! | `GET /_c/<sha256>` | `Render.Bundle` of the owning cell, any holder; `Cache-Control: immutable` | the anonymous chain |
//! | `GET /<file of the shell>` | `Render.Bundle` of `fe.shell` for that part, by the hash in the shell's signed entry | the anonymous chain |
//! | `POST /door` | forwards the browser's signed frame; holds no right of its own | the caller's chain, checked by every gate |
//!
//! It never reads a file and has no route table: every byte it sends is the answer of a `render:*` or `compose:*`
//! op, and `served()` says which (cell, node, sha256) for each path, which `cargo cell export` and proof step P13
//! record. `cargo cell export` writes the same answers to files for a static host.
pub mod server;

use base64::Engine;
use fe_chain::Identity;
use fe_host::sim::Holder;
use serde_json::{json, Value};
use server::{Req, Resp};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// The way this cell reaches the mesh: the same door frames a browser sends (`Hello`, `Anon`, `Call`), and the
/// node's content store for files installed by hash.
pub trait Wire: Send + Sync {
    /// One frame to the door; the answer as the browser gets it (`ok`, `body`, `refused`, `node`, or `transport`).
    fn frame(&self, op: &str, body: Value) -> Value;
    /// Bytes of the node's content store, by sha256 (only after a gate allowed the `Render.Bundle` that named it).
    fn blob(&self, sha256: &str) -> Option<Vec<u8>>;
    fn now(&self) -> u64;
}

/// Which render or compose step produced the bytes of one path.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct Served {
    pub path: String,
    pub step: String,
    pub cell: String,
    pub node: String,
    pub sha256: String,
    pub bytes: usize,
}

pub struct HttpServe<W: Wire> {
    pub wire: W,
    /// The route this cell holds `http-serve` for (`/live-frontend/`, or `/` on a dev node).
    pub route: String,
    id: Identity,
    anon: Mutex<Option<(Holder, u64)>>,
    pub served: Mutex<BTreeMap<String, Served>>,
    /// Answers named by their hash do not change; once a gate allowed one for this chain it is kept.
    cache: Mutex<BTreeMap<String, (Vec<u8>, String)>>,
    tree: Mutex<Option<(Vec<Value>, u64)>>,
}

const SHELL_RIGHT: &str = "fe.shell:render:page";

impl<W: Wire> HttpServe<W> {
    pub fn new(wire: W, route: &str, seed: [u8; 32]) -> HttpServe<W> {
        let route = if route.ends_with('/') { route.to_string() } else { format!("{route}/") };
        HttpServe { wire, route, id: Identity::from_seed(seed), anon: Mutex::new(None), served: Mutex::new(BTreeMap::new()),
            cache: Mutex::new(BTreeMap::new()), tree: Mutex::new(None) }
    }

    /// A call with the anonymous chain, joined (with the issuer's proof of work) when it is missing or near its end.
    fn call(&self, to: &str, api: &str, op: &str, body: Value) -> Value {
        let now = self.wire.now();
        let mut g = self.anon.lock().unwrap();
        if g.as_ref().is_none_or(|(_, until)| *until <= now + 30_000) {
            let mut h = Holder::new(0);
            h.id = Identity::from_seed_hex(&self.id.seed_hex()).unwrap();
            // four leading zeros satisfy any issuer asking for four or fewer
            let j = self.wire.frame("Anon", json!({"holder": h.id.public_hex(), "pow": h.pow(4)}));
            if j["ok"] != true {
                return json!({"ok": false, "refused": format!("anonymous chain: {}", j["refused"]), "body": null});
            }
            h.chain = serde_json::from_value(j["body"]["chain"].clone()).unwrap_or_default();
            let until = j["body"]["until"].as_u64().unwrap_or(0);
            *g = Some((h, until));
        }
        let (h, _) = g.as_mut().unwrap();
        let c = h.call(to, api, op, body, now);
        drop(g);
        self.wire.frame("Call", c)
    }

    fn tree(&self) -> Result<Vec<Value>, String> {
        let now = self.wire.now();
        if let Some((t, at)) = self.tree.lock().unwrap().clone() {
            if now < at + 1000 {
                return Ok(t);
            }
        }
        let t = self.resolve()?;
        *self.tree.lock().unwrap() = Some((t.clone(), now));
        Ok(t)
    }

    fn resolve(&self) -> Result<Vec<Value>, String> {
        let hello = self.wire.frame("Hello", Value::Null);
        let door = hello["body"]["door"].as_str().unwrap_or("d").to_string();
        let r = self.call(&door, "page.compose@1", "Resolve", Value::Null);
        if r["ok"] != true {
            return Err(format!("page.compose refused: {}", r["refused"].as_str().unwrap_or("?")));
        }
        Ok(r["body"]["entries"].as_array().cloned().unwrap_or_default())
    }

    /// `Render.Bundle` of `sha` at the entry's holders in turn; the bytes, checked against the hash.
    fn bundle(&self, e: &Value, sha: &str) -> Result<(Vec<u8>, String), String> {
        if let Some(c) = self.cache.lock().unwrap().get(sha) {
            return Ok(c.clone());
        }
        let r = self.bundle_from_holders(e, sha)?;
        self.cache.lock().unwrap().insert(sha.to_string(), r.clone());
        Ok(r)
    }

    fn bundle_from_holders(&self, e: &Value, sha: &str) -> Result<(Vec<u8>, String), String> {
        let api = e["body"]["api"].as_str().unwrap_or("");
        let mut last = String::from("no holder");
        for h in e["holders"].as_array().into_iter().flatten() {
            let node = h["node"].as_str().unwrap_or("");
            let r = self.call(node, api, "Render.Bundle", json!({"sha256": sha}));
            if r["ok"] != true {
                last = r["refused"].as_str().or(r["transport"].as_str()).unwrap_or("refused").to_string();
                continue;
            }
            let bytes = if r["body"]["store"] == true {
                match self.wire.blob(sha) {
                    Some(b) => b,
                    None => {
                        last = format!("{node}: not in the content store");
                        continue;
                    }
                }
            } else {
                base64::engine::general_purpose::STANDARD.decode(r["body"]["b64"].as_str().unwrap_or("")).map_err(|e| e.to_string())?
            };
            if fe_chain::sha256_hex(&bytes) != sha {
                last = format!("{node}: sha256 does not match the signed entry");
                continue;
            }
            return Ok((bytes, node.to_string()));
        }
        Err(last)
    }

    fn note(&self, path: &str, step: &str, cell: &str, node: &str, bytes: &[u8]) {
        let s = Served { path: path.into(), step: step.into(), cell: cell.into(), node: node.into(), sha256: fe_chain::sha256_hex(bytes), bytes: bytes.len() };
        self.served.lock().unwrap().insert(path.into(), s);
    }

    /// `GET /`: the page for the anonymous chain.
    pub fn page(&self) -> Result<Vec<u8>, String> {
        let tree = self.tree()?;
        let shell = tree.iter().find(|e| e["body"]["render"] == SHELL_RIGHT).ok_or("no cell offers fe.shell:render:page to this chain")?;
        let parts: BTreeMap<String, String> = shell["body"]["parts"].as_array().into_iter().flatten().filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str()?.to_string()))).collect();
        let tpl_sha = parts.get("index.html").ok_or("the shell has no index.html part")?;
        let (tpl, node) = self.bundle(shell, tpl_sha)?;
        let mut html = String::from_utf8(tpl).map_err(|e| e.to_string())?;
        let main = shell["body"]["bundle"].as_str().unwrap_or("");
        let (code, _) = self.bundle(shell, main)?;
        let integrity = base64::engine::general_purpose::STANDARD.encode(sha256_bytes(&code));
        html = html.replace(
            "<script type=\"module\" src=\"page.js\"></script>",
            &format!("<script type=\"module\" src=\"_c/{main}.js\" integrity=\"sha256-{integrity}\"></script>"),
        );
        // first paint and readers without scripts: Render.Html of each public view, from its own cell
        let mut frags = String::new();
        let mut views: Vec<&Value> = tree.iter().filter(|e| e["body"]["render"] != SHELL_RIGHT && e["body"]["place"] != "slot").collect();
        views.sort_by_key(|e| (e["body"]["view"].as_str().unwrap_or("").to_string(), e["body"]["instance"].as_str().unwrap_or("").to_string()));
        for e in views {
            let api = e["body"]["api"].as_str().unwrap_or("");
            let body = json!({"view": e["body"]["view"], "instance": e["body"]["instance"]});
            for h in e["holders"].as_array().into_iter().flatten() {
                let r = self.call(h["node"].as_str().unwrap_or(""), api, "Render.Html", body.clone());
                if r["ok"] == true {
                    frags.push_str(r["body"]["html"].as_str().unwrap_or(""));
                    frags.push('\n');
                    break;
                }
            }
        }
        html = html.replace("<!--first-paint-->", &frags);
        let out = html.into_bytes();
        self.note("index.html", "page.compose Resolve + fe.shell Render.Bundle + Render.Html", "page.compose", &node, &out);
        Ok(out)
    }

    /// A file by path: `_c/<sha256>[.ext]` (any cell in the tree), or a named part of the shell.
    pub fn file(&self, path: &str) -> Result<(Vec<u8>, bool), String> {
        let tree = self.tree()?;
        if let Some(rest) = path.strip_prefix("_c/") {
            let sha = rest.split('.').next().unwrap_or("");
            let e = tree
                .iter()
                .find(|e| e["body"]["bundle"] == sha || e["body"]["parts"].as_array().into_iter().flatten().any(|p| p[1] == sha))
                .ok_or("no view this chain may render has that hash")?;
            let (b, node) = self.bundle(e, sha)?;
            self.note(path, "Render.Bundle", e["body"]["api"].as_str().unwrap_or(""), &node, &b);
            return Ok((b, true));
        }
        let shell = tree.iter().find(|e| e["body"]["render"] == SHELL_RIGHT).ok_or("no shell")?;
        let sha = shell["body"]["parts"].as_array().into_iter().flatten().find(|p| p[0] == path).and_then(|p| p[1].as_str()).ok_or("not a file of the shell")?;
        let (b, node) = self.bundle(shell, sha)?;
        self.note(path, "Render.Bundle", "fe.shell@1", &node, &b);
        Ok((b, false))
    }

    /// Every path the anonymous chain can be sent: `index.html`, the shell's code by hash, and its parts.
    pub fn paths(&self) -> Result<Vec<String>, String> {
        let tree = self.tree()?;
        let shell = tree.iter().find(|e| e["body"]["render"] == SHELL_RIGHT).ok_or("no shell")?;
        let mut out = vec![format!("_c/{}.js", shell["body"]["bundle"].as_str().unwrap_or(""))];
        for p in shell["body"]["parts"].as_array().into_iter().flatten() {
            let name = p[0].as_str().unwrap_or("");
            if name != "index.html" {
                out.push(name.to_string());
            }
        }
        Ok(out)
    }

    /// One HTTP request. Paths outside this cell's route are not its to answer.
    pub fn handle(&self, req: Req) -> Resp {
        let path = req.path.split('?').next().unwrap_or("/").to_string();
        let Some(rel) = path.strip_prefix(self.route.trim_end_matches('/')).map(|r| r.trim_start_matches('/').to_string()) else {
            return Resp::status(404, "not under this cell's route");
        };
        if req.method == "POST" && rel == "door" {
            let v: Value = serde_json::from_slice(&req.body).unwrap_or_default();
            return Resp::json(&self.wire.frame(v["op"].as_str().unwrap_or(""), v["body"].clone()));
        }
        if rel == "door.json" {
            let live = std::env::var("LF_MODE").map(|m| m == "live").unwrap_or(false);
            return Resp::json(&json!({"mode": if live { "live" } else { "local" }, "door": "door"}));
        }
        if rel == "_served" {
            // what this cell has sent so far, path by path: the step, cell, node and sha256 behind each (P13)
            let v: Vec<Served> = self.served.lock().unwrap().values().cloned().collect();
            return Resp::json(&serde_json::to_value(v).unwrap());
        }
        if req.method != "GET" {
            return Resp::status(404, "no");
        }
        if rel.is_empty() || rel == "index.html" {
            return match self.page() {
                Ok(b) => Resp { status: 200, ctype: "text/html; charset=utf-8".into(), body: b, immutable: false },
                Err(e) => Resp::status(503, &e),
            };
        }
        match self.file(&rel) {
            Ok((b, immutable)) => Resp { status: 200, ctype: server::ctype(&rel).into(), body: b, immutable },
            Err(e) => Resp::status(if e.contains("refused") || e.contains("Refused") { 403 } else { 404 }, &e),
        }
    }

    /// `cargo cell export`: the answers for the anonymous chain, written to files, plus `served.json` naming the
    /// render or compose step behind every file.
    pub fn export(&self, out: &std::path::Path) -> Result<Vec<Served>, String> {
        std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
        let page = self.page()?;
        std::fs::write(out.join("index.html"), &page).map_err(|e| e.to_string())?;
        for p in self.paths()? {
            let (b, _) = self.file(&p)?;
            let f = out.join(&p);
            if let Some(d) = f.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::write(f, b).map_err(|e| e.to_string())?;
        }
        let served: Vec<Served> = self.served.lock().unwrap().values().cloned().collect();
        std::fs::write(out.join("served.json"), serde_json::to_string_pretty(&served).unwrap()).map_err(|e| e.to_string())?;
        Ok(served)
    }
}

fn sha256_bytes(b: &[u8]) -> Vec<u8> {
    (0..32).map(|i| u8::from_str_radix(&fe_chain::sha256_hex(b)[2 * i..2 * i + 2], 16).unwrap()).collect()
}

/// The shell's install (`fe.shell@1`, view `shell.page`) from a built `web/dist`: `page.js` is the bundle, every
/// other file a part by hash. Returns the ctl body (hashes only) and the content store (sha256 -> bytes).
pub fn shell_install(dist: &std::path::Path, skip: &[&str]) -> std::io::Result<(Value, BTreeMap<String, Vec<u8>>)> {
    let mut store = BTreeMap::new();
    let mut parts = serde_json::Map::new();
    let mut stack = vec![dist.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p.strip_prefix(dist).unwrap().to_string_lossy().replace('\\', "/");
            if rel == "page.js" || skip.iter().any(|s| rel.starts_with(s)) {
                continue;
            }
            let b = std::fs::read(&p)?;
            let sha = fe_chain::sha256_hex(&b);
            parts.insert(rel, Value::String(sha.clone()));
            store.insert(sha, b);
        }
    }
    let main = std::fs::read(dist.join("page.js"))?;
    let sha = fe_chain::sha256_hex(&main);
    store.insert(sha.clone(), main);
    let body = json!({"view": "shell.page", "api": "fe.shell", "version": "1.0", "dir": "shell", "ref_path": "web/shell",
        "bundle_sha": sha, "parts_sha": parts, "peers": [], "label": "the empty page: loader, bus, sandbox"});
    Ok((body, store))
}
