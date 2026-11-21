//! Four nodes in one process: composition per viewer, render ops behind the ordinary gate, counted uses,
//! failover, revocation, tamper, slots filled by capability, a minor swap, and that an input log replays to the
//! same outputs.
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

fn mesh(with_ops: bool) -> LocalMesh {
    let (_root, cfgs) = configs("test", T0, "");
    let mut m = LocalMesh::new(cfgs, T0);
    for n in ["a", "b", "c"] {
        install(&mut m, n, "notes.board", "notes", "1.0", "notes-1.0-bundle", &["a", "b", "c"]);
        install(&mut m, n, "status.panel", "status", "1.1", "status-bundle", &["a", "b", "c"]);
    }
    if with_ops {
        install(&mut m, "a", "ops.panel", "ops", "1.1", "ops-bundle", &["a"]);
    }
    install(&mut m, "d", "timeline.view", "fe.history", "1.1", "timeline-bundle", &["d"]);
    m.tick(1000);
    m
}

fn setup() -> (LocalMesh, Holder) {
    let mut m = mesh(true);
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
    let r = call(&mut m, &mut h, "d", "page.compose@1", "Resolve", Value::Null);
    assert!(r.ok, "{r:?}");
    let views: Vec<&str> = r.body["entries"].as_array().unwrap().iter().map(|e| e["body"]["view"].as_str().unwrap()).collect();
    assert!(views.contains(&"notes.board") && views.contains(&"status.panel") && views.contains(&"timeline.view"));
    assert!(!views.contains(&"ops.panel"), "ops.panel is never sent to a visitor");
    let notes = r.body["entries"].as_array().unwrap().iter().find(|e| e["body"]["view"] == "notes.board").unwrap();
    assert_eq!(notes["holders"].as_array().unwrap().len(), 3);
    let sha = notes["body"]["bundle"].as_str().unwrap().to_string();
    let f = call(&mut m, &mut h, "b", "notes@1", "Render.Bundle", json!({"sha256": sha}));
    assert!(f.ok, "{f:?}");
    let e = call(&mut m, &mut h, "b", "notes@1", "Render.Entry", json!({"view": "notes.board"}));
    assert!(e.ok && e.body["body"]["render"] == "notes:render:board" && e.body["node"] == "b", "{e:?}");
    let ops_sha = fe_chain::sha256_hex(b"ops-bundle");
    let f = call(&mut m, &mut h, "a", "ops@1", "Render.Bundle", json!({"sha256": ops_sha}));
    assert!(!f.ok && f.body["Refused"]["need"] == "ops:render:panel", "{f:?}");
    let f = call(&mut m, &mut h, "a", "ops@1", "Render.Entry", json!({"view": "ops.panel"}));
    assert!(!f.ok && f.body["Refused"]["need"] == "ops:render:panel", "{f:?}");
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
    let r = call(&mut m, &mut h, "d", "page.compose@1", "Resolve", Value::Null);
    let notes = r.body["entries"].as_array().unwrap().iter().find(|e| e["body"]["view"] == "notes.board").unwrap().clone();
    assert_eq!(notes["holders"].as_array().unwrap().len(), 2, "b's lease ran out");
    let r = call(&mut m, &mut h, "d", "fe.visitor@1", "Revoke", Value::Null);
    assert!(r.ok && r.body["revoked"] == true);
    let r = call(&mut m, &mut h, "a", "notes@1", "List", Value::Null);
    assert!(r.refused.unwrap().contains("revoked"));
    let r = call(&mut m, &mut h, "d", "page.compose@1", "Resolve", Value::Null);
    assert!(!r.ok);
}

#[test]
fn tampered_bundle_does_not_match() {
    let (mut m, mut h) = setup();
    let sha = fe_chain::sha256_hex(b"notes-1.0-bundle");
    m.ctl("c", "Tamper", json!({"view": "notes.board"}));
    let f = call(&mut m, &mut h, "c", "notes@1", "Render.Bundle", json!({"sha256": sha}));
    let got = base64::engine::general_purpose::STANDARD.decode(f.body["b64"].as_str().unwrap()).unwrap();
    assert_ne!(fe_chain::sha256_hex(&got), sha);
    let f = call(&mut m, &mut h, "a", "notes@1", "Render.Bundle", json!({"sha256": sha}));
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
    install(&mut m, "c", "trend.chart", "trend", "1.1", "trend-bundle", &["c"]);
    let r = call(&mut m, &mut h, "a", "notes@1", "Put", json!({"op_id": "t1", "note": {"id": "n", "text": "t"}}));
    assert!(r.ok);
    let s = call(&mut m, &mut h, "c", "trend@1", "Series", json!({"series": "notes"}));
    assert_eq!(s.body["lines"][0][1][0][1], json!(1.0), "{s:?}");
}

/// P14: a visitor's tree has no `ops` entry and is byte-identical to the tree of a mesh that never ran `ops`; the
/// visitor never called `ops:render:panel` (nothing in the history names it for this chain).
#[test]
fn p14_the_visitor_tree_equals_a_mesh_without_ops() {
    let (mut m, mut h) = setup();
    let mut w = mesh(false);
    let mut h2 = Holder::new(9);
    let pow = h2.pow(2);
    let j = w.door("Join", json!({"holder": h2.id.public_hex(), "pow": pow}));
    h2.chain = serde_json::from_value(j.body["chain"].clone()).unwrap();
    let a = call(&mut m, &mut h, "d", "page.compose@1", "Resolve", Value::Null);
    let b = call(&mut w, &mut h2, "d", "page.compose@1", "Resolve", Value::Null);
    let entries = |r: &fe_host::Reply| serde_json::to_string(&r.body["entries"]).unwrap();
    assert!(!entries(&a).contains("ops.panel"));
    assert_eq!(entries(&a), entries(&b), "same bytes with or without ops on the mesh");
    // an operator-class chain (all rights) does see it: the difference is the chain, not the page
    let op_tree = m.hosts.get("d").unwrap().tree(&["ops:render:panel".to_string(), "notes:render:board".to_string()], m.now);
    assert!(op_tree.iter().any(|e| e.body.view == "ops.panel"));
}

/// P15: a slot names a capability. A second cell offering `trend:render:chart` on another node fills the slot
/// when the first stops; the parent (`notes.board`) entry does not change.
#[test]
fn p15_a_slot_is_refilled_by_another_holder_of_the_capability() {
    let (mut m, mut h) = setup();
    install(&mut m, "c", "trend.chart", "trend", "1.1", "trend-bundle", &["c"]);
    m.tick(1000);
    let tree = |m: &mut LocalMesh, h: &mut Holder| call(m, h, "d", "page.compose@1", "Resolve", Value::Null).body["entries"].as_array().unwrap().clone();
    let t1 = tree(&mut m, &mut h);
    let board1 = t1.iter().find(|e| e["body"]["view"] == "notes.board").unwrap().clone();
    let slot = &board1["body"]["slots"][0];
    assert_eq!(slot["render"], "trend:render:chart");
    let fill = |t: &[Value]| t.iter().find(|e| e["body"]["render"] == "trend:render:chart").map(|e| e["holders"].as_array().unwrap().iter().map(|h| h["node"].as_str().unwrap().to_string()).collect::<Vec<_>>());
    assert_eq!(fill(&t1), Some(vec!["c".to_string()]));
    install(&mut m, "a", "trend.chart", "trend", "1.1", "trend-bundle", &["a"]);
    m.down.insert("c".into());
    for _ in 0..7 {
        m.tick(1000);
    }
    let t2 = tree(&mut m, &mut h);
    assert_eq!(fill(&t2), Some(vec!["a".to_string()]), "the slot's capability is now held by a");
    let board2 = t2.iter().find(|e| e["body"]["view"] == "notes.board").unwrap();
    assert_eq!(board1["body"], board2["body"], "the parent's entry is unchanged");
}

/// P16: `notes@1.2` (the release that carries the render ops) over 1.1 is a minor swap: one entry changes
/// version, the rows stay.
#[test]
fn p16_notes_1_2_over_1_1_is_a_minor_swap() {
    let (mut m, mut h) = setup();
    let r = call(&mut m, &mut h, "a", "notes@1", "Put", json!({"op_id": "k", "note": {"id": "n", "text": "kept"}}));
    assert!(r.ok);
    for n in ["a", "b", "c"] {
        install(&mut m, n, "notes.board", "notes", "1.1", "notes-1.1-bundle", &["a", "b", "c"]);
    }
    for n in ["a", "b", "c"] {
        install(&mut m, n, "notes.board", "notes", "1.2", "notes-1.2-bundle", &["a", "b", "c"]);
    }
    m.tick(1000);
    let t = call(&mut m, &mut h, "d", "page.compose@1", "Resolve", Value::Null);
    let boards: Vec<&Value> = t.body["entries"].as_array().unwrap().iter().filter(|e| e["body"]["view"] == "notes.board").collect();
    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0]["body"]["version"], "1.2");
    let l = call(&mut m, &mut h, "b", "notes@1", "List", Value::Null);
    assert_eq!(l.body[0]["text"], "kept");
}

/// Render.Html: a static fragment and its sha256, for first paint and readers without scripts.
#[test]
fn render_html_is_a_static_fragment() {
    let (mut m, mut h) = setup();
    let r = call(&mut m, &mut h, "a", "status@1", "Render.Html", json!({"view": "status.panel"}));
    assert!(r.ok, "{r:?}");
    let html = r.body["html"].as_str().unwrap();
    assert!(html.starts_with("<section class=\"cellpanel\" data-view=\"status.panel\"") && !html.contains("<script"));
    assert_eq!(r.body["sha256"], fe_chain::sha256_hex(html.as_bytes()));
    let r = call(&mut m, &mut h, "a", "ops@1", "Render.Html", json!({"view": "ops.panel"}));
    assert_eq!(r.body["Refused"]["need"], "ops:render:panel");
}
