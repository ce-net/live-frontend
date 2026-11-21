//! `cargo cell export [out]`: the Pages site is the output of the `http.serve` cell for the anonymous chain. The
//! base plan runs on four in-process nodes (the cells with their bundles, the shell on the door node), and
//! `http.serve` answers `/`, the shell's code by hash and the shell's files; each answer is written to a file,
//! and `served.json` names the render or compose step, cell, node and sha256 behind every file. A static host
//! cannot refuse, so the page says that refusal is shown by the sim nodes and by real gates in live mode.
use fe_host::sim::{configs, LocalMesh};
use http_serve_cell::{shell_install, HttpServe, Wire};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

pub struct LocalWire {
    pub mesh: Mutex<LocalMesh>,
    pub store: BTreeMap<String, Vec<u8>>,
}

impl Wire for LocalWire {
    fn frame(&self, op: &str, body: Value) -> Value {
        let mut m = self.mesh.lock().unwrap();
        m.tick(50);
        let r = m.door(op, body);
        json!({"ok": r.ok, "body": r.body, "refused": r.refused})
    }
    fn blob(&self, sha256: &str) -> Option<Vec<u8>> {
        self.store.get(sha256).cloned()
    }
    fn now(&self) -> u64 {
        self.mesh.lock().unwrap().now
    }
}

/// Four in-process nodes running the plan's base installs and the shell, as `cargo cell export` and the tests use.
pub fn mesh(root: &Path, now: u64) -> Result<LocalWire, String> {
    let dist = root.join("web/dist");
    let plan: Value = serde_json::from_str(&std::fs::read_to_string(root.join("web/plan.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let index: Value = serde_json::from_str(&std::fs::read_to_string(dist.join("bundles.json")).map_err(|e| format!("web/dist/bundles.json: {e} (cargo cell build first)"))?).map_err(|e| e.to_string())?;
    let (_root, cfgs) = configs("export", now, &crate::commit(root));
    let mut m = LocalMesh::new(cfgs, now);
    for s in plan["base"].as_array().into_iter().flatten() {
        if s["op"] != "install" {
            continue;
        }
        let key = format!("{}@{}", s["view"].as_str().unwrap_or(""), s["version"].as_str().unwrap_or(""));
        let b = &index[&key];
        let mut body = s.clone();
        match b["file"].as_str() {
            Some(f) => {
                use base64::Engine;
                let bytes = std::fs::read(dist.join(f)).map_err(|e| format!("{f}: {e}"))?;
                body["bundle_b64"] = Value::String(base64::engine::general_purpose::STANDARD.encode(bytes));
            }
            None => body["bundle_sha"] = b["sha"].clone(),
        }
        let node = s["node"].as_str().unwrap_or("").to_string();
        let r = m.ctl(&node, "Install", body);
        if !r.ok {
            return Err(format!("install {key} on {node}: {:?}", r.refused));
        }
    }
    let (body, store) = shell_install(&dist, &[]).map_err(|e| e.to_string())?;
    let r = m.ctl("d", "Install", body);
    if !r.ok {
        return Err(format!("install fe.shell on d: {:?}", r.refused));
    }
    m.tick(1000);
    Ok(LocalWire { mesh: Mutex::new(m), store })
}

pub fn run(root: &Path, out: &Path) -> Result<String, String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let cell = HttpServe::new(mesh(root, now)?, "/", [7; 32]);
    let _ = std::fs::remove_dir_all(out);
    let served = cell.export(out)?;
    let bytes: usize = served.iter().map(|s| s.bytes).sum();
    let steps: BTreeMap<&str, usize> = served.iter().fold(BTreeMap::new(), |mut m, s| {
        *m.entry(s.step.as_str()).or_insert(0) += 1;
        m
    });
    Ok(format!("{} files, {bytes} bytes, every one an answer of http.serve for the anonymous chain ({steps:?}) -> {}", served.len(), out.display()))
}
