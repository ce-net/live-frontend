//! `cargo cell verify [proof]`: checks the signed steps (live-proof: every signature and the hash chain), then
//! re-executes every input each node process logged through a fresh `fe-host` with the same config and compares
//! the sha256 of every output. The page does the same in the browser with the wasm build.
use fe_host::{Config, Host, Input};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub fn replay(dir: &Path) -> Result<BTreeMap<String, (usize, usize)>, String> {
    let cfgs: Vec<Config> = serde_json::from_str(&std::fs::read_to_string(dir.join("configs.json")).map_err(|e| format!("configs.json: {e}"))?).map_err(|e| e.to_string())?;
    let mut out = BTreeMap::new();
    for c in cfgs {
        let name = c.name.clone();
        let Ok(log) = std::fs::read_to_string(dir.join(format!("inputs/{name}.jsonl"))) else { continue };
        let mut h = Host::new(c)?;
        let (mut n, mut same) = (0, 0);
        for line in log.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(line).map_err(|e| format!("{name}: {e}"))?;
            let inp: Input = serde_json::from_value(v["in"].clone()).map_err(|e| format!("{name}: {e}"))?;
            let o = serde_json::to_string(&h.input(inp)).unwrap();
            n += 1;
            if fe_chain::sha256_hex(o.as_bytes()) == v["out"].as_str().unwrap_or("") {
                same += 1;
            }
        }
        out.insert(name, (n, same));
    }
    Ok(out)
}

pub fn run(dir: &Path) -> Result<String, String> {
    let jsonl = std::fs::read_to_string(dir.join("run.jsonl")).map_err(|e| format!("run.jsonl: {e}"))?;
    let roster: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("roster.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let keys = vec![roster["recorder"].as_str().unwrap_or("").to_string()];
    let steps = live_proof::verify(&jsonl, &keys)?;
    let r = replay(dir)?;
    let mut lines = vec![format!("run.jsonl: {steps} signed steps, chain intact, all signed by the recorder {}", &keys[0][..20.min(keys[0].len())])];
    let mut bad = false;
    for (n, (all, same)) in &r {
        lines.push(format!("node {n}: {same} of {all} logged inputs re-executed to the same output bytes"));
        bad |= same != all;
    }
    if bad {
        return Err(lines.join("\n"));
    }
    Ok(lines.join("\n"))
}
