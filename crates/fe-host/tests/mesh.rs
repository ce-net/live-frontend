//! Four nodes in one process: discovery per viewer, may_fetch, the gate's refusals, counted uses, failover,
//! revocation, tamper, and that an input log replays to the same outputs.
use base64::Engine;
use fe_host::sim::{configs, Holder, LocalMesh};
use fe_host::Host;
use serde_json::{json, Value};

const T0: u64 = 1_763_251_200_000;

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

fn install(m: &mut LocalMesh, node: &str, view: &str, api: &str, version: &str, bundle: &str, peers: &[&str]) -> String {
    let r = m.ctl(node, "Install", json!({"view": view, "api": api, "version": version, "dir": api, "bundle_b64": b64(bundle), "peers": peers}));
    assert!(r.ok, "{r:?}");
    r.body["bundle"].as_str().unwrap().to_string()
}

fn setup() -> (LocalMesh, Holder) {
    let (_root, cfgs) = configs("test", T0, "");
    let mut m = LocalMesh::new(cfgs, T0);
    for n in ["a", "b", "c"] {
        install(&mut m, n, "notes.board", "notes", "1.0", "notes-1.0-bundle", &["a", "b", "c"]);
        install(&mut m, n, "status.panel", "status", "1.0", "status-bundle", &["a", "b", "c"]);
    }
    install(&mut m, "a", "ops.panel", "ops", "1.0", "ops-bundle", &["a"]);
    install(&mut m, "d", "timeline.view", "fe.history", "1.0", "timeline-bundle", &["d"]);
    m.tick(1000);
    let mut h = Holder::new(9);
    let pow = h.pow(2);
    let j = m.door("Join", json!({"holder": h.id.public_hex(), "pow": pow}));
    assert!(j.ok, "{j:?}");
    h.chain = serde_json::from_value(j.body["chain"].clone()).unwrap();
    (m, h)
}

fn call(m: &mut LocalMesh, h: &mut Holder, to: &str, api: &str, op: &str, body: Value) -> fe_host::Reply {
    let now = m.now;
    let c = h.call(to, api, op, body, now);
    m.door("Call", c)
}

#[test]
fn a_viewer_sees_only_what_its_chain_holds() {
    let (mut m, mut h) = setup();
    let r = call(&mut m, &mut h, "d", "fe.manifest@1", "Resolve", Value::Null);
    assert!(r.ok, "{r:?}");
    let views: Vec<&str> = r.body["entries"].as_array().unwrap().iter().map(|e| e["body"]["view"].as_str().unwrap()).collect();
    assert!(views.contains(&"notes.board") && views.contains(&"status.panel") && views.contains(&"timeline.view"));
    assert!(!views.contains(&"ops.panel"), "ops.panel is never sent to a visitor");
    let notes = r.body["entries"].as_array().unwrap().iter().find(|e| e["body"]["view"] == "notes.board").unwrap();
    assert_eq!(notes["holders"].as_array().unwrap().len(), 3);
    let sha = notes["body"]["bundle"].as_str().unwrap().to_string();
    let f = call(&mut m, &mut h, "b", "fe.serve@1", "Fetch", json!({"sha256": sha}));
    assert!(f.ok);
    let ops_sha = fe_chain::sha256_hex(b"ops-bundle");
    let f = call(&mut m, &mut h, "a", "fe.serve@1", "Fetch", json!({"sha256": ops_sha}));
    assert!(!f.ok && f.body["Refused"]["need"] == "ops:read", "{f:?}");
    let o = call(&mut m, &mut h, "a", "ops@1", "Read", Value::Null);
    assert_eq!(o.body["Refused"]["need"], "ops:read");
}

#[test]
fn writes_allowed_purge_refused_count_runs_out() {
    let (mut m, mut h) = setup();
    let put = |i: u32| json!({"op_id": format!("op{i}"), "note": {"id": "n1", "text": format!("hello {i}")}});
    let r = call(&mut m, &mut h, "a", "notes@1", "Put", put(0));
    assert!(r.ok, "{r:?}");
    let l = call(&mut m, &mut h, "c", "notes@1", "List", Value::Null);
    assert_eq!(l.body[0]["text"], "hello 0", "replicated to c");
    let p = call(&mut m, &mut h, "a", "notes@1", "Purge", Value::Null);
    assert!(p.refused.as_ref().unwrap().starts_with("Refused{need: \"notes:admin\""), "{p:?}");
    let mut last = None;
    for i in 1..60 {
        m.tick(100);
        let r = call(&mut m, &mut h, "b", "notes@1", "Put", put(i));
        if !r.ok {
            last = Some((i, r));
            break;
        }
    }
    let (i, r) = last.expect("the count runs out");
    assert_eq!(i, 40, "40 uses: the 41st write is refused");
    assert!(r.refused.unwrap().contains("uses exhausted"));
}

#[test]
fn failover_and_revoke() {
    let (mut m, mut h) = setup();
    m.down.insert("b".into());
    let r = call(&mut m, &mut h, "b", "status@1", "Read", Value::Null);
    assert!(!r.ok && r.refused.unwrap().contains("unreachable"));
    let r = call(&mut m, &mut h, "c", "status@1", "Read", Value::Null);
    assert!(r.ok && r.body["holder"] == "c");
    for _ in 0..7 {
        m.tick(1000);
    }
    let r = call(&mut m, &mut h, "d", "fe.manifest@1", "Resolve", Value::Null);
    let notes = r.body["entries"].as_array().unwrap().iter().find(|e| e["body"]["view"] == "notes.board").unwrap().clone();
    assert_eq!(notes["holders"].as_array().unwrap().len(), 2, "b's lease ran out");
    let r = call(&mut m, &mut h, "d", "fe.visitor@1", "Revoke", Value::Null);
    assert!(r.ok && r.body["revoked"] == true);
    let r = call(&mut m, &mut h, "a", "notes@1", "List", Value::Null);
    assert!(r.refused.unwrap().contains("revoked"));
    let r = call(&mut m, &mut h, "d", "fe.manifest@1", "Resolve", Value::Null);
    assert!(!r.ok);
}

#[test]
fn tampered_bundle_does_not_match() {
    let (mut m, mut h) = setup();
    let sha = fe_chain::sha256_hex(b"notes-1.0-bundle");
    m.ctl("c", "Tamper", json!({"view": "notes.board"}));
    let f = call(&mut m, &mut h, "c", "fe.serve@1", "Fetch", json!({"sha256": sha}));
    let got = base64::engine::general_purpose::STANDARD.decode(f.body["b64"].as_str().unwrap()).unwrap();
    assert_ne!(fe_chain::sha256_hex(&got), sha);
    let f = call(&mut m, &mut h, "a", "fe.serve@1", "Fetch", json!({"sha256": sha}));
    let got = base64::engine::general_purpose::STANDARD.decode(f.body["b64"].as_str().unwrap()).unwrap();
    assert_eq!(fe_chain::sha256_hex(&got), sha);
}

#[test]
fn an_input_log_replays_to_the_same_outputs() {
    let (mut m, mut h) = setup();
    call(&mut m, &mut h, "a", "notes@1", "Put", json!({"op_id": "x", "note": {"id": "n", "text": "t"}}));
    m.tick(3000);
    let (_r, cfgs) = configs("test", T0, "");
    let mut fresh: std::collections::BTreeMap<String, Host> = cfgs.into_iter().map(|c| (c.name.clone(), Host::new(c).unwrap())).collect();
    let mut orig: std::collections::BTreeMap<String, Host> = configs("test", T0, "").1.into_iter().map(|c| (c.name.clone(), Host::new(c).unwrap())).collect();
    for (node, inp) in &m.log {
        let a = serde_json::to_string(&orig.get_mut(node).unwrap().input(inp.clone())).unwrap();
        let b = fresh.get_mut(node).unwrap().input_json(&serde_json::to_string(inp).unwrap());
        assert_eq!(a, b);
    }
    assert!(m.log.len() > 50);
}

#[test]
fn notes_writes_reach_the_trend_cell() {
    let (mut m, mut h) = setup();
    install(&mut m, "c", "trend.chart", "trend", "1.0", "trend-bundle", &["c"]);
    let r = call(&mut m, &mut h, "a", "notes@1", "Put", json!({"op_id": "t1", "note": {"id": "n", "text": "t"}}));
    assert!(r.ok);
    let s = call(&mut m, &mut h, "c", "trend@1", "Series", json!({"series": "notes"}));
    assert_eq!(s.body["lines"][0][1][0][1], json!(1.0), "{s:?}");
}
