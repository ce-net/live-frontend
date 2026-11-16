//! `cargo cell <command>` (alias in .cargo/config.toml).
mod check;
mod gen;
mod http;
mod run;
mod verify;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn root() -> PathBuf {
    let mut d = std::env::current_dir().unwrap();
    loop {
        if d.join("web/plan.json").exists() && d.join("Cargo.toml").exists() {
            return d;
        }
        if !d.pop() {
            eprintln!("run this inside the live-frontend checkout");
            std::process::exit(2);
        }
    }
}

const HELP: &str = "cargo cell <command>
  dev                 four nodes + the page on http://127.0.0.1:21200/; a change to a cell rebuilds only that cell
                      and the open page swaps only that panel, with its state
  sim                 the same without the watcher
  check               contract-break check against contracts/frozen, slot props, decl.lint on cells/*/ui/src
  freeze <api>|--all  writes contracts/frozen/<api>@<major>.json for the current release
  new <name>          a new cell pair and UI directory from the template
  gen                 TypeScript clients from the contracts (cells/*/ui/gen)
  build [--pages]     gen + every UI bundle + the page + the wasm node (web/dist)
  record              a real multi-process run driven by headless browsers -> proof/
  verify [dir]        signatures, hash chain, and re-execution of every logged input
  break-proof         three contract breaks that must fail: a renamed field (check and the TypeScript views),
                      a view that reads an op it does not declare the right for (the Rust build)
  vendor-check        sha256 of every vendored file against web/vendor/*/VENDOR.json";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = root();
    let code = match args.first().map(String::as_str) {
        Some("gen") => {
            for p in gen::run(&r) {
                println!("{p}");
            }
            0
        }
        Some("check") => {
            gen::run(&r);
            match check::run(&r) {
                Ok(s) => {
                    println!("ok: {s}");
                    0
                }
                Err(e) => {
                    for x in e {
                        eprintln!("error: {x}");
                    }
                    1
                }
            }
        }
        Some("freeze") => match check::freeze(&r, args.get(1).map(String::as_str).unwrap_or("--all")) {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Some("build") => build(&r, args.iter().any(|a| a == "--pages")),
        Some("vendor-check") => vendor_check(&r),
        Some("break-proof") => break_proof(&r),
        Some("new") => new_cell(&r, args.get(1).map(String::as_str).unwrap_or("")),
        Some("sim") => sim(&r, false, false),
        Some("dev") => sim(&r, true, false),
        Some("record") => sim(&r, false, true),
        Some("verify") => {
            let d = args.get(1).map(PathBuf::from).unwrap_or_else(|| r.join("proof"));
            match verify::run(&d) {
                Ok(s) => {
                    println!("{s}");
                    0
                }
                Err(e) => {
                    eprintln!("FAILED\n{e}");
                    1
                }
            }
        }
        _ => {
            println!("{HELP}");
            0
        }
    };
    std::process::exit(code);
}

fn sh(c: &mut Command) -> bool {
    c.status().map(|s| s.success()).unwrap_or(false)
}

fn build(r: &Path, pages: bool) -> i32 {
    gen::run(r);
    let mut node = Command::new("node");
    node.arg("build.mjs").current_dir(r.join("web"));
    if pages {
        node.arg("--pages");
    }
    if !sh(&mut node) {
        return 1;
    }
    let ok = sh(Command::new("cargo").args(["build", "-q", "--profile", "wasm", "--target", "wasm32-unknown-unknown", "-p", "sim-wasm"]).current_dir(r));
    if ok {
        let _ = std::fs::copy(r.join("target/wasm32-unknown-unknown/wasm/sim_wasm.wasm"), r.join("web/dist/sim.wasm"));
        println!("web/dist/sim.wasm");
    } else {
        eprintln!("wasm build failed (rustup target add wasm32-unknown-unknown); the page needs it only for the in-browser sim");
    }
    0
}

fn vendor_check(r: &Path) -> i32 {
    let mut bad = 0;
    let mut n = 0;
    for d in std::fs::read_dir(r.join("web/vendor")).into_iter().flatten().flatten() {
        let Ok(t) = std::fs::read_to_string(d.path().join("VENDOR.json")) else { continue };
        let v: Value = serde_json::from_str(&t).unwrap();
        for (f, h) in v["sha256"].as_object().unwrap() {
            n += 1;
            let got = std::fs::read(d.path().join(f)).map(|b| fe_chain::sha256_hex(&b)).unwrap_or_default();
            if got != h.as_str().unwrap() {
                eprintln!("changed: {}/{f}", d.path().display());
                bad += 1;
            }
        }
    }
    println!("{} of {n} vendored files match their pinned sha256", n - bad);
    (bad > 0) as i32
}

fn sim(r: &Path, watch: bool, record: bool) -> i32 {
    let seed = std::env::var("LF_SEED").unwrap_or_else(|_| if record { format!("record-{}", sim_mesh::now_ms() / 1000) } else { "dev".into() });
    let net = match run::Net::start(r, &seed, record) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    net.serve_ctl();
    if net.wait_page().is_err() {
        eprintln!("the door did not come up");
        return 1;
    }
    println!("four nodes up (a, b, c serve cells; d is the door). Open http://127.0.0.1:{}/  (controls: :{})", run::DOOR, run::CTL);
    if record {
        return record_run(r, &net);
    }
    if watch {
        watch_loop(r, &net);
    }
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn mtimes(dir: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
    let mut v = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "gen") {
                    stack.push(p);
                }
            } else if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                v.push((p, m));
            }
        }
    }
    v.sort();
    v
}

/// The dev loop: a change under cells/<name>/ui rebuilds that cell's bundles and re-installs them where they run
/// (the open page swaps that one panel); a change under cells/<name>/{api,cell} rebuilds the node binary and
/// restarts the nodes one at a time (replicas carry the rows over).
fn watch_loop(r: &Path, net: &std::sync::Arc<run::Net>) {
    let mut last: std::collections::BTreeMap<String, Vec<(PathBuf, std::time::SystemTime)>> = Default::default();
    loop {
        for d in std::fs::read_dir(r.join("cells")).into_iter().flatten().flatten() {
            let name = d.file_name().to_string_lossy().to_string();
            for part in ["ui", "api", "cell"] {
                let k = format!("{name}/{part}");
                let now = mtimes(&d.path().join(part));
                let prev = last.insert(k.clone(), now.clone());
                if prev.is_none() || prev.as_ref() == Some(&now) {
                    continue;
                }
                println!("changed: cells/{k}");
                if part == "ui" {
                    if !sh(Command::new("node").args(["build.mjs", "--only", &name]).current_dir(r.join("web"))) {
                        continue;
                    }
                    let bundles: Value = serde_json::from_str(&std::fs::read_to_string(r.join("web/dist/bundles.json")).unwrap()).unwrap();
                    let installed = net.installed.lock().unwrap().clone();
                    for (node, list) in installed {
                        for s in list.iter().filter(|s| s["dir"] == name.as_str()) {
                            let k = format!("{}@{}", s["view"].as_str().unwrap(), s["version"].as_str().unwrap());
                            let b64 = std::fs::read(r.join("web/dist").join(bundles[&k]["file"].as_str().unwrap())).map(|b| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b)).unwrap();
                            let mut body = s.clone();
                            body["bundle_b64"] = json!(b64);
                            let ok = net.ctl.ctl(&node, "Install", body).map(|x| x.ok).unwrap_or(false);
                            println!("  {k} re-announced on {node}: {}", if ok { "ok" } else { "FAILED" });
                        }
                    }
                } else if sh(Command::new("cargo").args(["build", "-q", "--release", "-p", "fe-node"]).current_dir(r)) {
                    gen::run(r);
                    for n in ["a", "b", "c"] {
                        let _ = net.step(&json!({"op": "kill", "node": n}));
                        let _ = net.step(&json!({"op": "start", "node": n}));
                        std::thread::sleep(Duration::from_millis(1500));
                        println!("  node {n} restarted on the new build");
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(700));
    }
}

fn record_run(r: &Path, net: &std::sync::Arc<run::Net>) -> i32 {
    let out = r.join("proof");
    std::fs::create_dir_all(out.join("inputs")).unwrap();
    let browsers = std::env::var("LF_BROWSERS").unwrap_or_else(|_| "chromium".into());
    let obs = r.join("run/observations.json");
    let ok = sh(Command::new("node")
        .args(["tools/record.mjs", "--url", &format!("http://127.0.0.1:{}/", run::DOOR), "--ctl", &format!("http://127.0.0.1:{}", run::CTL), "--browsers", &browsers, "--out"])
        .arg(&obs)
        .arg("--shots")
        .arg(out.join("shots"))
        .current_dir(r.join("web")));
    std::thread::sleep(Duration::from_millis(600));
    net.ctl.stop_all();
    if !ok {
        eprintln!("the recorder failed; observations so far: {}", obs.display());
    }
    let Ok(t) = std::fs::read_to_string(&obs) else { return 1 };
    let o: Value = serde_json::from_str(&t).unwrap();
    let mut log = live_proof::Log::default();
    let k = sim_mesh::key_from_hex(&net.recorder.seed_hex());
    for s in o["steps"].as_array().unwrap() {
        let req = s["request"].clone();
        let resp = s["response"].clone();
        let step = live_proof::Step {
            n: 0, step: s["n"].as_u64().unwrap_or(0) as u32, cell: s["cell"].as_str().unwrap_or("").into(), op: s["op"].as_str().unwrap_or("").into(),
            node: s["browser"].as_str().unwrap_or("").into(), to: s["to"].as_str().unwrap_or("").into(),
            request_ref: format!("ce-blob:sha256:{}", fe_chain::sha256_hex(req.to_string().as_bytes())),
            response_sha256: fe_chain::sha256_hex(resp.to_string().as_bytes()), ms: s["ms"].as_f64().unwrap_or(0.0),
            verdict: s["verdict"].as_str().unwrap_or("").into(), request: req, response: resp, note: s["note"].as_str().unwrap_or("").into(),
            prev: String::new(), key: String::new(), sig: String::new(),
        };
        log.push(step, &k);
    }
    std::fs::write(out.join("run.jsonl"), log.jsonl()).unwrap();
    for n in ["a", "b", "c", "d"] {
        let _ = std::fs::copy(r.join(format!("run/inputs/{n}.jsonl")), out.join(format!("inputs/{n}.jsonl")));
    }
    let _ = std::fs::copy(r.join("run/configs.json"), out.join("configs.json"));
    let recorder = format!("ed25519:{}", net.recorder.public_hex());
    let cfgs: Value = serde_json::from_str(&std::fs::read_to_string(r.join("run/configs.json")).unwrap()).unwrap();
    let nodes: Vec<Value> = cfgs[0]["nodes"].as_array().unwrap().iter().map(|m| json!({"name": m["name"], "class": m["class"], "key": m["key"], "fp": fe_chain::fingerprint(m["key"].as_str().unwrap())})).collect();
    let roster = json!({"recorder": recorder, "seed": net.seed, "root": cfgs[0]["root"], "nodes": nodes, "t0": net.t0,
        "note": "sim keys: every key of this run is derived from the seed above, so anyone can re-create the nodes and re-execute the inputs"});
    std::fs::write(out.join("roster.json"), serde_json::to_string_pretty(&roster).unwrap()).unwrap();
    let mut summary = o["summary"].clone();
    summary["recorded"] = json!(o["recorded"]);
    match verify::run(&out) {
        Ok(s) => {
            println!("{s}");
            summary["verify"] = json!(s);
        }
        Err(e) => {
            eprintln!("verify FAILED\n{e}");
            summary["verify"] = json!(format!("FAILED: {e}"));
        }
    }
    std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&summary).unwrap()).unwrap();
    println!("proof written to {}", out.display());
    (!ok) as i32
}

fn new_cell(r: &Path, name: &str) -> i32 {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase()) {
        eprintln!("cargo cell new <name>: lowercase letters only");
        return 2;
    }
    let base = r.join("cells").join(name);
    if base.exists() {
        eprintln!("cells/{name} exists");
        return 1;
    }
    let ty = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
    let files = [
        ("api/Cargo.toml", format!("[package]\nname = \"{name}-api\"\ndescription = \"The typed contract of the {name} cell\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\nauthors.workspace = true\n\n[dependencies]\nwire-lite.workspace = true\nlf-contract.workspace = true\nserde_json.workspace = true\n")),
        ("api/src/lib.rs", format!("//! {name}: a new cell.\nlf_contract::contract! {{\n    api {ty} = \"{name}\", version 1, minor 0;\n    types {{ pub struct Hello {{ pub text: String }} }}\n    ops {{ 1 => Read(()) -> Hello, needs \"{name}:read\"; }}\n    ui {{\n        view \"{name}.panel\" place \"main\" {{\n            needs [\"{name}:read\"]; data [Read]; tries []; streams [];\n            props \"()\"; state \"()\"; slots []; speaks []; hears [];\n        }}\n    }}\n}}\n")),
        ("cell/Cargo.toml", format!("[package]\nname = \"{name}-cell\"\ndescription = \"The backend core of the {name} cell\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\nauthors.workspace = true\n\n[dependencies]\n{name}-api = {{ path = \"../api\" }}\nfe-runtime.workspace = true\nserde_json.workspace = true\n")),
        ("cell/src/lib.rs", format!("use fe_runtime::{{to, Cell, Cx}};\nuse serde_json::Value;\n\n#[derive(Default)]\npub struct {ty};\n\nimpl Cell for {ty} {{\n    fn name(&self) -> &'static str {{\n        \"{name}\"\n    }}\n    fn serve(&mut self, _: u32, _: u32) {{}}\n    fn handle(&mut self, _major: u32, op: &str, _body: Value, cx: &mut Cx) -> Result<Value, String> {{\n        match op {{\n            \"Read\" => Ok(to(&{name}_api::Hello {{ text: format!(\"hello from {{}}\", cx.node) }})),\n            _ => Err(format!(\"{name} has no op {{op}}\")),\n        }}\n    }}\n}}\n")),
        ("ui/src/panel.ts", format!("import {{ cell }} from '../../../../web/cell-rt';\nimport {{ h }} from '../../../../web/vendor/ui-cells/kit/dom';\nimport {{ client }} from '../gen/{name}_1_0';\n\ncell(async (cx) => {{\n  const api = client(cx);\n  const r = await api.Read();\n  cx.root.append(h('h2', {{ text: '{name}' }}), h('p', {{ text: r.text }}));\n}});\n")),
    ];
    for (p, t) in files {
        let f = base.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, t).unwrap();
    }
    println!("cells/{name}: api, cell, ui. To serve it: add `{name}-api`/`{name}-cell` to crates/fe-host (registry.rs: descriptor and make_cell, at the `cell-new` markers), a bundle and an install to web/plan.json; under `cargo cell dev` the node restarts on the new build and the panel appears.");
    0
}

/// Each change below must make the named step fail; the file is restored afterwards.
fn break_proof(r: &Path) -> i32 {
    let f = r.join("cells/notes/api/src/lib.rs");
    let orig = std::fs::read_to_string(&f).unwrap();
    let mut ok = true;
    let mut run = |what: &str, edited: String, cmds: &[(&str, &[&str], &str)]| {
        std::fs::write(&f, edited).unwrap();
        for (prog, args, dir) in cmds {
            let out = Command::new(prog).args(*args).current_dir(r.join(dir)).output().unwrap();
            let failed = !out.status.success();
            let text = String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
            let first = text.lines().find(|l| l.contains("error")).unwrap_or("").trim().to_string();
            println!("{} {what}: `{prog} {}` {}  {}", if failed { "PASS" } else { "FAIL" }, args.join(" "), if failed { "failed as it must" } else { "succeeded (it must not)" }, &first[..first.len().min(160)]);
            ok &= failed;
        }
        std::fs::write(&f, &orig).unwrap();
    };
    // 1. Delta.seq renamed to head in notes 1.1 (a backend that never names the type still builds): the frozen
    //    contract no longer matches, naming every dependent view, and the views no longer compile
    let renamed = orig.replacen("pub struct Delta { pub seq: u64, pub rows: Vec<Note> }\n            pub struct Rep { pub op_id: String, pub row: serde_json::Value }\n            pub struct NoteAdded { pub id: String }\n        }\n        ops {\n            1 => List(()) -> Vec<Note>, needs \"notes:read\";\n            2 => Put(PutReq) -> Note, needs \"notes:write\", counted;\n            3 => Since(From) -> Delta, needs \"notes:read\";\n            9 => Purge(()) -> u32, needs \"notes:admin\";\n            10 => Replicate(Rep) -> bool, needs \"notes:replicate\";\n            11 => Dump(()) -> Vec<Rep>, needs \"notes:replicate\";\n        }\n        ui {\n            view \"notes.board\" place \"main\" {\n                needs [\"notes:read\", \"notes:write\"]; data [List, Put]; tries [Purge]; streams [Since];\n                props \"()\"; state \"Draft\"; slots [trend: \"trend.chart\" (\"TrendProps\")]; speaks [\"NoteAdded\"]; hears [];\n            }\n        }\n    }\n}\n\npub mod v2_0", "pub struct Delta { pub head: u64, pub rows: Vec<Note> }\n            pub struct Rep { pub op_id: String, pub row: serde_json::Value }\n            pub struct NoteAdded { pub id: String }\n        }\n        ops {\n            1 => List(()) -> Vec<Note>, needs \"notes:read\";\n            2 => Put(PutReq) -> Note, needs \"notes:write\", counted;\n            3 => Since(From) -> Delta, needs \"notes:read\";\n            9 => Purge(()) -> u32, needs \"notes:admin\";\n            10 => Replicate(Rep) -> bool, needs \"notes:replicate\";\n            11 => Dump(()) -> Vec<Rep>, needs \"notes:replicate\";\n        }\n        ui {\n            view \"notes.board\" place \"main\" {\n                needs [\"notes:read\", \"notes:write\"]; data [List, Put]; tries [Purge]; streams [Since];\n                props \"()\"; state \"Draft\"; slots [trend: \"trend.chart\" (\"TrendProps\")]; speaks [\"NoteAdded\"]; hears [];\n            }\n        }\n    }\n}\n\npub mod v2_0", 1);
    assert_ne!(renamed, orig);
    run("Delta.seq renamed to head in notes 1.1", renamed, &[("cargo", &["run", "-q", "--release", "-p", "lf", "--", "check"], "."), ("npx", &["tsc", "--noEmit", "-p", "."], "web")]);
    // 2. notes.board reads Purge as data although its needs do not hold notes:admin: the contract does not compile
    let wider = orig.replacen("data [List, Put]; tries [Purge];", "data [List, Put, Purge]; tries [];", 1);
    assert_ne!(wider, orig);
    run("notes.board reads Purge without notes:admin in its needs", wider, &[("cargo", &["build", "-q", "-p", "notes-api"], ".")]);
    // rebuild the real contract
    let _ = Command::new("cargo").args(["build", "-q", "--release", "-p", "lf"]).current_dir(r).status();
    gen::run(r);
    if ok {
        println!("break-proof: every break failed where it must");
        0
    } else {
        1
    }
}
