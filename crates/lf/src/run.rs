//! `cargo cell sim | dev | record`: four real node processes on loopback (sim-mesh: own keys, signed frames),
//! the door and page on :21200, and the controller that carries out `web/plan.json` on :21202.
use base64::Engine;
use fe_chain::Identity;
use serde_json::{json, Value};
use sim_mesh::{Controller, Member, Mesh, Roster};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DOOR: u16 = 21200;
pub const CTL: u16 = 21202;
pub const MESH: u16 = 21210;

#[allow(dead_code)]
pub struct Net {
    pub root: PathBuf,
    pub dir: PathBuf,
    pub ctl: Controller,
    pub plan: Value,
    pub bundles: Value,
    pub installed: Mutex<BTreeMap<String, Vec<Value>>>,
    pub up: Mutex<BTreeMap<String, bool>>,
    pub recorder: Identity,
    pub seed: String,
    pub t0: u64,
}

fn http_get(port: u16, path: &str) -> Option<String> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    out.split("\r\n\r\n").nth(1).map(String::from)
}

impl Net {
    pub fn start(root: &Path, seed: &str, log: bool) -> Result<Arc<Net>, String> {
        let dir = root.join("run");
        let _ = std::fs::remove_dir_all(dir.join("inputs"));
        std::fs::create_dir_all(dir.join("inputs")).map_err(|e| e.to_string())?;
        let t0 = sim_mesh::now_ms();
        let commit = std::process::Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root).output().ok()
            .and_then(|o| String::from_utf8(o.stdout).ok()).map(|s| s.trim().to_string()).filter(|s| s.len() == 40).unwrap_or_default();
        let (sim_root, cfgs) = fe_host::sim::configs(seed, t0, &commit);
        let ctl_key = sim_root.derive("ctl");
        let mut roster = Roster { root: ctl_key.public_hex(), nodes: BTreeMap::new() };
        for (i, c) in cfgs.iter().enumerate() {
            let key = c.nodes.iter().find(|m| m.name == c.name).unwrap().key.clone();
            roster.nodes.insert(c.name.clone(), Member { port: MESH + i as u16, key });
        }
        std::fs::write(dir.join("mesh.json"), serde_json::to_string_pretty(&roster).unwrap()).unwrap();
        for c in &cfgs {
            std::fs::write(dir.join(format!("{}.json", c.name)), serde_json::to_string_pretty(c).unwrap()).unwrap();
        }
        std::fs::write(dir.join("configs.json"), serde_json::to_string_pretty(&cfgs).unwrap()).unwrap();
        let bin = std::env::current_exe().unwrap().parent().unwrap().join("fe-node");
        if !bin.exists() {
            return Err(format!("{} not built (cargo build --release -p fe-node)", bin.display()));
        }
        let (d2, r2) = (dir.clone(), root.to_path_buf());
        let spawn = Box::new(move |name: &str| {
            let mut c = std::process::Command::new(&bin);
            c.arg("--config").arg(d2.join(format!("{name}.json"))).arg("--mesh").arg(d2.join("mesh.json"));
            if log {
                c.arg("--log").arg(d2.join(format!("inputs/{name}.jsonl")));
            }
            if name == "d" {
                c.arg("--http").arg(format!("127.0.0.1:{DOOR}")).arg("--static").arg(r2.join("web/dist"));
            }
            c.spawn().expect("spawn fe-node")
        });
        let mesh = Mesh::new("ctl", sim_mesh::key_from_hex(&ctl_key.seed_hex()), roster);
        let ctl = Controller { mesh, children: Mutex::new(BTreeMap::new()), spawn };
        let plan: Value = serde_json::from_str(&std::fs::read_to_string(root.join("web/plan.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let bundles: Value = serde_json::from_str(&std::fs::read_to_string(root.join("web/dist/bundles.json")).map_err(|_| "web/dist/bundles.json missing: run `cargo cell build` first")?).unwrap();
        let net = Arc::new(Net {
            root: root.into(), dir, ctl, plan, bundles, installed: Default::default(), up: Default::default(),
            recorder: sim_root.derive("recorder"), seed: seed.into(), t0,
        });
        for n in ["a", "b", "c", "d"] {
            net.ctl.start(n);
        }
        for n in ["a", "b", "c", "d"] {
            net.ctl.wait_up(n, Duration::from_secs(10))?;
            net.up.lock().unwrap().insert(n.into(), true);
        }
        let base = net.plan["base"].as_array().unwrap().clone();
        for s in &base {
            net.step(s)?;
        }
        Ok(net)
    }

    pub fn bundle_b64(&self, view: &str, version: &str) -> Result<(String, String), String> {
        let k = format!("{view}@{version}");
        let b = &self.bundles[&k];
        let file = b["file"].as_str().ok_or(format!("no bundle {k}"))?;
        let bytes = std::fs::read(self.root.join("web/dist").join(file)).map_err(|e| format!("{file}: {e}"))?;
        Ok((base64::engine::general_purpose::STANDARD.encode(&bytes), fe_chain::sha256_hex(&bytes)))
    }

    /// One plan step: install, uninstall, tamper, untamper, kill, start, wait, note.
    pub fn step(&self, s: &Value) -> Result<Value, String> {
        let st = |k: &str| s[k].as_str().unwrap_or("").to_string();
        let node = st("node");
        match st("op").as_str() {
            "install" => {
                let (b64, _) = self.bundle_b64(&st("view"), &st("version"))?;
                let mut body = s.clone();
                body["bundle_b64"] = json!(b64);
                let r = self.ctl.ctl(&node, "Install", body)?;
                if !r.ok {
                    return Err(format!("install on {node}: {:?}", r.refused));
                }
                let mut i = self.installed.lock().unwrap();
                let list = i.entry(node).or_default();
                list.retain(|x| !(x["view"] == s["view"] && x["api"] == s["api"] && x["instance"] == s["instance"]));
                list.push(s.clone());
                Ok(r.body)
            }
            "uninstall" => {
                let r = self.ctl.ctl(&node, "Uninstall", s.clone())?;
                self.installed.lock().unwrap().entry(node).or_default().retain(|x| x["view"] != s["view"]);
                Ok(r.body)
            }
            "tamper" | "untamper" => {
                let r = self.ctl.ctl(&node, "Tamper", json!({"view": st("view"), "on": st("op") == "tamper"}))?;
                Ok(r.body)
            }
            "kill" => {
                self.ctl.kill(&node);
                self.up.lock().unwrap().insert(node.clone(), false);
                let _ = self.ctl.ctl("d", "Note", json!({"kind": "Killed", "cell": "", "detail": {"node": node}}));
                Ok(json!({"killed": node}))
            }
            "start" => {
                self.ctl.start(&node);
                let t = self.ctl.wait_up(&node, Duration::from_secs(10))?;
                self.up.lock().unwrap().insert(node.clone(), true);
                let list = self.installed.lock().unwrap().get(&node).cloned().unwrap_or_default();
                for x in list {
                    self.step(&x)?;
                }
                let _ = self.ctl.ctl("d", "Note", json!({"kind": "Started", "cell": "", "detail": {"node": node}}));
                Ok(json!({"started": node, "ms": t.as_millis() as u64}))
            }
            "wait" => {
                std::thread::sleep(Duration::from_millis(s["ms"].as_u64().unwrap_or(1000)));
                Ok(Value::Null)
            }
            "note" => Ok(self.ctl.ctl("d", "Note", s["body"].clone())?.body),
            o => Err(format!("unknown step {o}")),
        }
    }

    pub fn action(&self, name: &str) -> Result<Value, String> {
        let steps = self.plan["actions"][name].as_array().ok_or(format!("no action {name}"))?.clone();
        let t = Instant::now();
        let mut out = vec![];
        for s in &steps {
            out.push(self.step(s)?);
        }
        Ok(json!({"action": name, "ms": t.elapsed().as_millis() as u64, "results": out}))
    }

    pub fn state(&self) -> Value {
        json!({"up": *self.up.lock().unwrap(), "installed": *self.installed.lock().unwrap(), "mode": "local"})
    }

    pub fn wait_page(&self) -> Result<(), String> {
        for _ in 0..50 {
            if http_get(DOOR, "/door.json").is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("door did not answer on :21200".into())
    }
}

/// The net the controls act on (a record run starts one per browser).
pub static CURRENT: std::sync::OnceLock<Mutex<Option<Arc<Net>>>> = std::sync::OnceLock::new();

/// The controller's HTTP endpoint for the page's controls (and the recorder). Started once per process.
pub fn serve_ctl(net: &Arc<Net>) {
    let slot = CURRENT.get_or_init(|| Mutex::new(None));
    let first = slot.lock().unwrap().replace(net.clone()).is_none();
    if !first {
        return;
    }
    std::thread::spawn(move || {
        crate::http::serve(&format!("127.0.0.1:{CTL}"), move |req| {
            let Some(me) = CURRENT.get().and_then(|s| s.lock().unwrap().clone()) else { return crate::http::Resp::status(503, "no net") };
            if req.path == "/ctl/state" {
                return crate::http::Resp::json(&me.state());
            }
            if req.method == "POST" && req.path == "/ctl" {
                let v: Value = serde_json::from_slice(&req.body).unwrap_or_default();
                let r = match v["action"].as_str() {
                    Some(a) => me.action(a),
                    None => me.step(&v["step"]),
                };
                return crate::http::Resp::json(&match r {
                    Ok(v) => json!({"ok": true, "result": v}),
                    Err(e) => json!({"ok": false, "error": e}),
                });
            }
            crate::http::Resp::status(404, "no")
        })
    });
}
