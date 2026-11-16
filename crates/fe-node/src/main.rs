//! `fe-node --config a.json --mesh mesh.json [--log inputs/a.jsonl] [--http 127.0.0.1:21200 --static web/dist]`
//!
//! One node: the host behind a sim-mesh door (every frame signed by a member's key, ctl frames by the run's root).
//! Each input is logged with the sha256 of its output, so `cargo cell verify` and the page can re-execute the log.
mod http;

use fe_host::{Config, Host, Input, Output};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sim_mesh::{Mesh, Reply as MReply, Roster};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Node {
    host: Mutex<Host>,
    log: Option<Mutex<std::fs::File>>,
    mesh: Arc<Mesh>,
    me: String,
}

impl Node {
    fn input(self: &Arc<Self>, inp: Input) -> Output {
        let mut h = self.host.lock().unwrap();
        let line = serde_json::to_string(&inp).unwrap();
        let o = h.input(inp);
        let os = serde_json::to_string(&o).unwrap();
        if let Some(l) = &self.log {
            let sha = hex::encode(Sha256::digest(os.as_bytes()));
            let _ = writeln!(l.lock().unwrap(), "{{\"in\":{line},\"out\":\"{sha}\"}}");
        }
        drop(h);
        self.send(&o);
        o
    }
    fn send(self: &Arc<Self>, o: &Output) {
        for m in o.out.clone() {
            let me = self.clone();
            std::thread::spawn(move || {
                let r = me.mesh.call(&m.to, &m.api, &m.op, m.body, Duration::from_millis(1500));
                if let (Some(tag), Ok(r)) = (m.tag, r) {
                    me.input(Input::Reply { tag, ok: r.ok, body: r.body, now: now() });
                }
            });
        }
    }
}

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned())
}

fn now() -> u64 {
    sim_mesh::now_ms()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut cfg: Config = serde_json::from_str(&std::fs::read_to_string(arg(&args, "--config").expect("--config")).unwrap()).unwrap();
    // live: the keys are never in a file. The controller made them in memory and hands them over in this
    // process's environment, which is cleared once read; they end with the process.
    if cfg.seed.is_empty() {
        cfg.seed = std::env::var("LF_NODE_SEED").expect("no seed in the config and no LF_NODE_SEED");
        std::env::remove_var("LF_NODE_SEED");
    }
    if let Some((s, _)) = cfg.issuer.as_mut() {
        if s.is_empty() {
            *s = std::env::var("LF_ISSUER_SEED").expect("no issuer seed");
            std::env::remove_var("LF_ISSUER_SEED");
        }
    }
    let roster: Roster = serde_json::from_str(&std::fs::read_to_string(arg(&args, "--mesh").expect("--mesh")).unwrap()).unwrap();
    let me = cfg.name.clone();
    let key = ed25519_dalek::SigningKey::from_bytes(&hex::decode(&cfg.seed).unwrap().try_into().unwrap());
    let mesh = Mesh::new(&me, key, roster);
    let log = arg(&args, "--log").map(|p| {
        if let Some(d) = std::path::Path::new(&p).parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
        // a (re)started process begins with fresh state: the replay starts a fresh host at this line
        let _ = writeln!(f, "{{\"boot\":{}}}", now());
        Mutex::new(f)
    });
    let node = Arc::new(Node { host: Mutex::new(Host::new(cfg).expect("config")), log, mesh: mesh.clone(), me: me.clone() });

    let n = node.clone();
    let parent = std::os::unix::process::parent_id();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(250));
        if std::os::unix::process::parent_id() != parent {
            std::process::exit(0); // the controller that started this node is gone
        }
        n.input(Input::Tick { now: now() });
    });

    if let Some(addr) = arg(&args, "--http") {
        let root = std::path::PathBuf::from(arg(&args, "--static").unwrap_or_else(|| "web/dist".into()));
        let n = node.clone();
        std::thread::spawn(move || {
            http::serve(&addr, move |req| {
                if req.method == "POST" && req.path == "/door" {
                    let v: Value = serde_json::from_slice(&req.body).unwrap_or_default();
                    return http::Resp::json(&door(&n, v["op"].as_str().unwrap_or(""), v["body"].clone()));
                }
                if req.path == "/door.json" {
                    let live = std::env::var("LF_MODE").map(|m| m == "live").unwrap_or(false);
                    return http::Resp::json(&json!({"mode": if live { "live" } else { "local" }, "door": "/door"}));
                }
                http::file(&root, &req.path)
            })
        });
    }

    let n = node.clone();
    mesh.serve(move |f| {
        let o = n.input(Input::Frame { from: f.from.clone(), api: f.api.clone(), op: f.op.clone(), body: f.body.clone(), now: now() });
        let r = o.reply.unwrap();
        MReply { ok: r.ok, body: r.body, refused: r.refused, node: String::new(), sig: String::new() }
    });
}

/// One browser frame: handled by this node, or forwarded to the node it names (the door adds nothing).
fn door(n: &Arc<Node>, op: &str, body: Value) -> Value {
    let o = n.input(Input::Frame { from: "browser".into(), api: "fe.door@1".into(), op: op.into(), body, now: now() });
    let r = o.reply.clone().unwrap();
    match o.forward {
        None => json!({"ok": r.ok, "body": r.body, "refused": r.refused, "node": n.me}),
        Some(f) => {
            let t = std::time::Instant::now();
            match n.mesh.call(&f.to, &f.api, &f.op, f.body, Duration::from_millis(1500)) {
                Ok(rep) => json!({"ok": rep.ok, "body": rep.body, "refused": rep.refused, "node": rep.node, "sig": rep.sig, "ms": t.elapsed().as_secs_f64() * 1000.0}),
                Err(e) => json!({"ok": false, "body": null, "refused": null, "transport": e, "node": f.to}),
            }
        }
    }
}
