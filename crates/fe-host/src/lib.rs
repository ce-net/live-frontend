//! One node. `Host::input` takes one input (a frame that reached this node's door, a tick, or the answer to a
//! message it sent) and returns the reply, the messages to send and, on the door node, a visitor call to forward.
//! There is no I/O and no clock or randomness inside, so the same input log gives the same output bytes on a
//! native process and in a browser's wasm.
pub mod registry;
pub mod sim;

use base64::Engine;
use fe_chain::{fingerprint, holds, Identity, Link};
use fe_gate::{Gate, Refused, VisitorCall};
use fe_runtime::api::compose::{EntryBody, Signed, SlotDecl};
use fe_runtime::api::explain::{LinkView, MyChain, ViewVerdict};
use fe_runtime::api::http::{NodeInfo, Routed, Welcome};
use fe_runtime::api::render::{Bundle, ByHash, Html, HtmlReq, ViewReq};
use fe_runtime::compose::{entry_bytes, Compose};
use fe_runtime::history::History;
use fe_runtime::visitor::{Policy, Visitor};
use fe_runtime::{parse, to, Caller, Cell, Cx, Out};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    pub class: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub name: String,
    pub seed: String,
    pub root: String,
    pub nodes: Vec<Member>,
    pub door: String,
    /// On the door node: the issuer's seed and its grant from the root.
    #[serde(default)]
    pub issuer: Option<(String, Link)>,
    #[serde(default)]
    pub commit: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub pow_zeros: Option<usize>,
}
fn default_mode() -> String {
    "sim".into()
}

/// Rights a node (not a visitor) may use on another node's cells.
pub const NODE_RIGHTS: &[&str] = &["notes:replicate", "trend:push", "fe.history:append", "page.compose:announce", "page.compose:all"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Reply {
    pub ok: bool,
    pub body: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}
impl Reply {
    fn ok(v: Value) -> Reply {
        Reply { ok: true, body: v, refused: None }
    }
    fn no(why: impl Into<String>) -> Reply {
        Reply { ok: false, body: Value::Null, refused: Some(why.into()) }
    }
    fn gate(r: Refused) -> Reply {
        Reply { ok: false, refused: Some(r.to_string()), body: json!({"Refused": r}) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Forward {
    pub to: String,
    pub api: String,
    pub op: String,
    pub body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Output {
    pub reply: Option<Reply>,
    pub out: Vec<Out>,
    pub forward: Option<Forward>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Input {
    Frame { from: String, api: String, op: String, body: Value, now: u64 },
    Tick { now: u64 },
    Reply { tag: String, ok: bool, body: Value, now: u64 },
}

struct Install {
    body: EntryBody,
    peers: Vec<String>,
    bytes: Option<Vec<u8>>,
    /// Other files of the view by sha256 (only the shell has them: styles, worker, wasm node, recorded run).
    parts: BTreeMap<String, Option<Vec<u8>>>,
}

pub struct Host {
    pub cfg: Config,
    id: Identity,
    pub gate: Gate,
    cells: BTreeMap<String, Box<dyn Cell>>,
    installs: BTreeMap<(String, String, String), Install>,
    tampered: BTreeSet<String>,
    pub calls: u64,
    last_beat: u64,
    last_announce: u64,
    pub compose: Option<Compose>,
    history: Option<History>,
    visitor: Option<Visitor>,
    nodes: Vec<String>,
}

fn split_api(api: &str) -> (String, u32) {
    match api.rsplit_once('@') {
        Some((n, m)) => (n.to_string(), m.parse().unwrap_or(0)),
        None => (api.to_string(), 0),
    }
}

impl Host {
    pub fn new(cfg: Config) -> Result<Host, String> {
        let id = Identity::from_seed_hex(&cfg.seed).ok_or("bad seed")?;
        let me = cfg.nodes.iter().find(|m| m.name == cfg.name).ok_or("this node is not in the roster")?;
        if me.key != id.public_hex() {
            return Err("seed does not match this node's key in the roster".into());
        }
        let gate = Gate::new(&fingerprint(&id.public_hex()), &cfg.root);
        let nodes = cfg.nodes.iter().map(|m| m.name.clone()).collect();
        let mut h = Host {
            id, gate, cells: BTreeMap::new(), installs: BTreeMap::new(), tampered: BTreeSet::new(), calls: 0,
            last_beat: 0, last_announce: 0, compose: None, history: None, visitor: None, nodes, cfg,
        };
        if h.cfg.name == h.cfg.door {
            let keys = h.cfg.nodes.iter().map(|m| (m.name.clone(), m.key.clone())).collect();
            h.compose = Some(Compose::new(keys));
            h.history = Some(History::default());
            if let Some((seed, grant)) = h.cfg.issuer.clone() {
                let issuer = Identity::from_seed_hex(&seed).ok_or("bad issuer seed")?;
                let mut p = Policy::default();
                if let Some(z) = h.cfg.pow_zeros {
                    p.pow_zeros = z;
                }
                h.visitor = Some(Visitor::new(issuer, grant, p));
            }
        }
        Ok(h)
    }

    pub fn public_key(&self) -> String {
        self.id.public_hex()
    }

    /// The JSON form used by the wasm node and the input log.
    pub fn input_json(&mut self, inp: &str) -> String {
        match serde_json::from_str::<Input>(inp) {
            Ok(i) => serde_json::to_string(&self.input(i)).unwrap(),
            Err(e) => serde_json::to_string(&Output { reply: Some(Reply::no(format!("not an input: {e}"))), ..Default::default() }).unwrap(),
        }
    }

    pub fn input(&mut self, inp: Input) -> Output {
        let mut out = vec![];
        let mut o = Output::default();
        let now = match &inp {
            Input::Frame { now, .. } | Input::Tick { now } | Input::Reply { now, .. } => *now,
        };
        match inp {
            Input::Frame { from, api, op, body, now } => {
                let (r, f) = self.frame(&from, &api, &op, body, now, &mut out);
                o.reply = Some(r);
                o.forward = f;
            }
            Input::Tick { now } => self.tick(now, &mut out),
            Input::Reply { tag, ok, body, now } => {
                let name = tag.split('#').next().unwrap_or("").to_string();
                if let Some(mut c) = self.cells.remove(&name) {
                    let (peers, nodes, door) = (self.cx_parts(&name).0, self.nodes.clone(), self.cfg.door.clone());
                    let caller = Caller::Node(self.cfg.name.clone());
                    c.reply(&tag, ok, body, &mut Cx { now, node: &self.cfg.name.clone(), caller: &caller, peers: &peers, nodes: &nodes, door: &door, out: &mut out });
                    self.cells.insert(name, c);
                }
            }
        }
        // messages to this node itself are delivered now, in order
        let mut guard = 0;
        while let Some(i) = out.iter().position(|m| m.to == self.cfg.name) {
            let m = out.remove(i);
            let mut more = vec![];
            let _ = self.frame(&self.cfg.name.clone(), &m.api, &m.op, m.body, now, &mut more);
            out.extend(more);
            guard += 1;
            if guard > 256 {
                break;
            }
        }
        o.out = out;
        o
    }

    fn cx_parts(&self, api_name: &str) -> (Vec<String>, Vec<String>) {
        let peers: Vec<String> = self
            .installs
            .iter()
            .filter(|((_, _, a), _)| split_api(a).0 == api_name)
            .flat_map(|(_, i)| i.peers.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|p| *p != self.cfg.name)
            .collect();
        let nodes = self.nodes.iter().filter(|n| **n != self.cfg.name).cloned().collect();
        (peers, nodes)
    }

    fn dispatch(&mut self, api: &str, op: &str, body: Value, caller: &Caller, now: u64, out: &mut Vec<Out>) -> Result<Value, String> {
        let (name, major) = split_api(api);
        let (peers, nodes) = self.cx_parts(&name);
        let node = self.cfg.name.clone();
        let door = self.cfg.door.clone();
        let mut cx = Cx { now, node: &node, caller, peers: &peers, nodes: &nodes, door: &door, out };
        match name.as_str() {
            "page.compose" => self.compose.as_mut().ok_or("page.compose is not on this node")?.handle(major, op, body, &mut cx),
            "fe.history" => self.history.as_mut().ok_or("fe.history is not on this node")?.handle(major, op, body, &mut cx),
            "fe.visitor" => self.visitor.as_mut().ok_or("fe.visitor is not on this node")?.handle(major, op, body, &mut cx),
            _ => {
                let c = self.cells.get_mut(&name).ok_or_else(|| format!("{name} is not installed on {node}"))?;
                c.handle(major, op, body, &mut cx)
            }
        }
    }

    fn event(&mut self, kind: &str, cell: &str, detail: Value, now: u64, out: &mut Vec<Out>) {
        let b = json!({"kind": kind, "node": self.cfg.name, "cell": cell, "detail": detail});
        let _ = now;
        out.push(Out { to: self.cfg.door.clone(), api: "fe.history@1".into(), op: "Append".into(), body: b, tag: None });
    }

    fn frame(&mut self, from: &str, api: &str, op: &str, body: Value, now: u64, out: &mut Vec<Out>) -> (Reply, Option<Forward>) {
        if api == "sim.ctl" {
            return (self.ctl(op, body, now, out), None);
        }
        if api == "http.serve@1" {
            return self.door(op, body, now, out);
        }
        if api == "fe.gate@1" && op == "Revoke" {
            if from != self.cfg.door {
                return (Reply::no("only the issuer's node revokes"), None);
            }
            if let Some(c) = body["chain"].as_str() {
                self.gate.revoked.insert(c.to_string());
            }
            return (Reply::ok(Value::Null), None);
        }
        if api == "fe.beat@1" {
            let calls = body["calls"].as_u64().unwrap_or(0);
            for c in self.cells.values_mut() {
                c.beat(from, calls, now);
            }
            return (Reply::ok(Value::Null), None);
        }
        if body.get("call").is_some() && body.get("chain").is_some() && body.get("hsig").is_some() {
            return match parse::<VisitorCall>(body) {
                Ok(vc) => (self.visitor_call(vc, now, out), None),
                Err(e) => (Reply::no(e), None),
            };
        }
        // a node of this mesh
        if !self.nodes.iter().any(|n| n == from) {
            return (Reply::no(format!("{from} is not a node of this mesh")), None);
        }
        let (name, major) = split_api(api);
        let need = registry::right(&name, major, op);
        match need {
            Some(n) if NODE_RIGHTS.contains(&n.as_str()) => {}
            Some(n) => return (Reply::no(format!("a node may not call {api} {op} (needs {n})")), None),
            None => return (Reply::no(format!("{api} has no op {op}")), None),
        }
        match self.dispatch(api, op, body, &Caller::Node(from.to_string()), now, out) {
            Ok(v) => (Reply::ok(v), None),
            Err(e) => (Reply::no(e), None),
        }
    }

    fn visitor_call(&mut self, vc: VisitorCall, now: u64, out: &mut Vec<Out>) -> Reply {
        let (name, major) = split_api(&vc.call.api);
        let op = vc.call.op.clone();
        if op.starts_with("Render.") {
            return self.render(&name, major, &vc, now, out);
        }
        let Some(need) = registry::right(&name, major, &op) else {
            return Reply::gate(Refused { need: "?".into(), chain: vc.chain.id(), node: self.gate.node.clone(), why: format!("{} has no op {op}", vc.call.api) });
        };
        let counted = registry::counted(&name, major, &op);
        let eff = match self.gate.check(&vc, &need, counted, now) {
            Ok(e) => e,
            Err(r) => {
                self.event("Refused", &vc.call.api, json!({"op": op, "need": r.need, "why": r.why, "chain": r.chain, "gate": r.node}), now, out);
                return Reply::gate(r);
            }
        };
        self.calls += 1;
        let body: Value = match serde_json::from_str(&vc.call.body) {
            Ok(b) => b,
            Err(e) => return Reply::no(format!("body is not JSON: {e}")),
        };
        let caller = Caller::Visitor(eff.clone());
        let r = match name.as_str() {
            "fe.explain" => self.explain(&vc, &eff, now),
            _ => match self.dispatch(&vc.call.api, &op, body, &caller, now, out) {
                Ok(v) => Reply::ok(v),
                Err(e) => Reply::no(e),
            },
        };
        if counted && r.ok {
            let n = vc.ticket.as_ref().map(|t| t.n).unwrap_or(0);
            self.event("Allowed", &vc.call.api, json!({"op": op, "need": need, "chain": eff.id, "gate": self.gate.node, "use": n}), now, out);
        }
        r
    }

    /// The installs of `api_name` that answer for `view` (any instance when `instance` is None).
    fn installs_of(&self, api_name: &str, major: u32, view: &str, instance: Option<&str>) -> Vec<&Install> {
        let api = format!("{api_name}@{major}");
        self.installs.values().filter(|i| i.body.api == api && i.body.view == view && instance.is_none_or(|x| x == i.body.instance)).collect()
    }

    /// `Render.Entry`, `Render.Bundle`, `Render.Html`: the view as ops of its own backend cell, under the one right
    /// `<api>:render:<view>`. A view a chain may not render is refused here by the ordinary gate, with the same
    /// refusal text as any other op; there is no separate fetch rule.
    fn render(&mut self, name: &str, major: u32, vc: &VisitorCall, now: u64, out: &mut Vec<Out>) -> Reply {
        let op = vc.call.op.as_str();
        let body: Value = serde_json::from_str(&vc.call.body).unwrap_or(Value::Null);
        // the right this call needs, from the install it names (a view, or a bundle by hash)
        let target: Option<(EntryBody, Option<Vec<u8>>)> = match op {
            "Render.Entry" | "Render.Html" => {
                let v: Option<ViewReq> = serde_json::from_value(body.clone()).ok();
                v.and_then(|v| self.installs_of(name, major, &v.view, v.instance.as_deref()).into_iter().max_by_key(|i| i.body.version.clone()).map(|i| (i.body.clone(), None)))
            }
            "Render.Bundle" => {
                let h: Option<ByHash> = serde_json::from_value(body.clone()).ok();
                let api = format!("{name}@{major}");
                h.and_then(|h| {
                    self.installs.values().filter(|i| i.body.api == api).find_map(|i| {
                        if i.body.bundle == h.sha256 {
                            return Some((i.body.clone(), i.bytes.clone()));
                        }
                        i.parts.get(&h.sha256).map(|b| (i.body.clone(), b.clone()))
                    })
                })
            }
            _ => None,
        };
        let need = match &target {
            Some((b, _)) => b.render.clone(),
            None => format!("{name}:render:?"),
        };
        let eff = match self.gate.check(vc, &need, false, now) {
            Ok(e) => e,
            Err(r) => {
                self.event("Refused", &vc.call.api, json!({"op": op, "need": r.need, "why": r.why, "chain": r.chain, "gate": r.node}), now, out);
                return Reply::gate(r);
            }
        };
        let _ = eff;
        let Some((entry, bytes)) = target else {
            let why = match op {
                "Render.Bundle" => "no bundle with that hash here",
                "Render.Entry" | "Render.Html" => "no such view served here",
                _ => "not a render op",
            };
            return Reply::gate(Refused { need, chain: vc.chain.id(), node: self.gate.node.clone(), why: why.into() });
        };
        self.calls += 1;
        match op {
            "Render.Entry" => Reply::ok(to(&Signed { body: entry.clone(), node: self.cfg.name.clone(), sig: self.id.sign(&entry_bytes(&entry)) })),
            "Render.Bundle" => {
                // installed by hash only (the shell's files): the gate has decided; the node attaches the bytes from
                // its content store, by this hash, outside the host (so the input log stays small and replayable)
                let Some(mut b) = bytes else {
                    let sha = serde_json::from_value::<ByHash>(body).map(|h| h.sha256).unwrap_or_default();
                    return Reply::ok(json!({"sha256": sha, "b64": "", "store": true}));
                };
                let main = entry.bundle == serde_json::from_value::<ByHash>(body).map(|h| h.sha256).unwrap_or_default();
                if main && self.tampered.contains(&entry.view) && !b.is_empty() {
                    let k = b.len() / 2;
                    b[k] ^= 0x01;
                }
                let sha = fe_chain::sha256_hex(&b);
                let sha = if main && self.tampered.contains(&entry.view) { entry.bundle.clone() } else { sha };
                Reply::ok(to(&Bundle { sha256: sha, b64: base64::engine::general_purpose::STANDARD.encode(&b) }))
            }
            "Render.Html" => {
                let req: HtmlReq = serde_json::from_value(body).unwrap_or(HtmlReq { view: entry.view.clone(), instance: None, props: Value::Null });
                let inner = self.cells.get(name).and_then(|c| c.html(&entry.view, &entry.instance, &req.props));
                let html = html_fragment(&entry, inner.as_deref());
                Reply::ok(to(&Html { view: entry.view.clone(), sha256: fe_chain::sha256_hex(html.as_bytes()), html }))
            }
            _ => Reply::no(format!("{} has no op {op}", vc.call.api)),
        }
    }

    fn explain(&mut self, vc: &VisitorCall, eff: &fe_chain::Effective, now: u64) -> Reply {
        let Some(m) = &self.compose else { return Reply::no("fe.explain is on the door node") };
        let views = m
            .live(now)
            .into_iter()
            .map(|e| {
                let (held, missing) = std::iter::once(e.body.render.clone()).chain(e.body.needs.iter().cloned()).partition(|n| holds(&eff.rights, n));
                ViewVerdict { view: e.body.view, api: e.body.api, held, missing, holders: e.holders.into_iter().map(|h| h.node).collect() }
            })
            .collect();
        let st = self.visitor.as_ref().map(|v| v.state(&eff.id));
        let gates = self
            .history
            .as_ref()
            .map(|h| {
                h.events
                    .iter()
                    .rev()
                    .filter(|e| (e.kind == "Allowed" || e.kind == "Refused") && e.detail["chain"] == json!(eff.id))
                    .take(8)
                    .map(|e| (e.kind.clone(), e.node.clone(), e.detail["need"].as_str().unwrap_or("").to_string(), e.detail["why"].as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let links = vc
            .chain
            .0
            .iter()
            .map(|l| LinkView { issuer: fingerprint(&l.issuer), holder: fingerprint(&l.holder), rights: l.rights.clone(), until: l.caveats.until,
                count: l.caveats.count, rate: l.caveats.rate, rows: l.caveats.rows.clone(), mint: l.caveats.mint })
            .collect();
        let st = st.unwrap_or(fe_runtime::api::visitor::ChainState { id: eff.id.clone(), uses_left: 0, uses: 0, until: 0, revoked: false });
        Reply::ok(to(&MyChain { id: eff.id.clone(), links, uses_left: st.uses_left, uses: st.uses, until: st.until, revoked: st.revoked, views, gates }))
    }

    fn door(&mut self, op: &str, body: Value, now: u64, out: &mut Vec<Out>) -> (Reply, Option<Forward>) {
        match op {
            "Hello" => {
                let nodes = self.cfg.nodes.iter().map(|m| NodeInfo { name: m.name.clone(), class: m.class.clone(), key: m.key.clone(), fp: fingerprint(&m.key) }).collect();
                (Reply::ok(to(&Welcome { root: self.cfg.root.clone(), nodes, door: self.cfg.door.clone(), mode: self.cfg.mode.clone() })), None)
            }
            "Join" | "Anon" => {
                let node = self.cfg.name.clone();
                let door = self.cfg.door.clone();
                let nodes = self.nodes.clone();
                let caller = Caller::Node("browser".into());
                let Some(v) = self.visitor.as_mut() else { return (Reply::no("no issuer on this node"), None) };
                let mut cx = Cx { now, node: &node, caller: &caller, peers: &[], nodes: &nodes, door: &door, out };
                let r = match parse(body).and_then(|j| if op == "Join" { v.join(j, &mut cx) } else { v.anon(j, &mut cx) }) {
                    Ok(j) => Reply::ok(to(&j)),
                    Err(e) => Reply::no(e),
                };
                (r, None)
            }
            "Call" => {
                let mut r: Routed = match parse(body) {
                    Ok(r) => r,
                    Err(e) => return (Reply::no(e), None),
                };
                if r.to == self.cfg.name {
                    return (self.visitor_call(r.vc, now, out), None);
                }
                if !self.nodes.contains(&r.to) {
                    return (Reply::no(format!("{} is not a node of this mesh", r.to)), None);
                }
                let (name, major) = split_api(&r.vc.call.api);
                if registry::counted(&name, major, &r.vc.call.op) {
                    // one use of a counted chain: the issuer's counter row, checked against the holder's signature first
                    let id = r.vc.chain.id();
                    let held = r.vc.chain.verify(&self.cfg.root, now).and_then(|e| {
                        if fe_chain::verify_sig(&e.holder, &r.vc.call.digest(), &r.vc.hsig) { Ok(e) } else { Err("call not signed by the chain's holder".into()) }
                    });
                    match held {
                        Ok(e) if e.caveats.count.is_some() => {
                            let node = self.cfg.name.clone();
                            let door = self.cfg.door.clone();
                            let nodes = self.nodes.clone();
                            let caller = Caller::Visitor(e.clone());
                            let Some(v) = self.visitor.as_mut() else { return (Reply::no("no issuer on this node"), None) };
                            let mut cx = Cx { now, node: &node, caller: &caller, peers: &[], nodes: &nodes, door: &door, out };
                            match v.spend(&id, &r.vc.call.sha(), &mut cx) {
                                Ok(t) => r.vc.ticket = Some(t),
                                Err(why) => {
                                    let need = registry::right(&name, major, &r.vc.call.op).unwrap_or_default();
                                    let rf = Refused { need, chain: id.clone(), node: self.gate.node.clone(), why };
                                    self.event("Refused", &r.vc.call.api, json!({"op": r.vc.call.op, "need": rf.need, "why": rf.why, "chain": id, "gate": rf.node}), now, out);
                                    return (Reply::gate(rf), None);
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(why) => return (Reply::gate(Refused { need: "?".into(), chain: id, node: self.gate.node.clone(), why }), None),
                    }
                }
                let f = Forward { to: r.to.clone(), api: r.vc.call.api.clone(), op: r.vc.call.op.clone(), body: to(&r.vc) };
                (Reply::ok(json!({"forwarded": r.to})), Some(f))
            }
            _ => (Reply::no(format!("http.serve has no op {op}")), None),
        }
    }

    fn ctl(&mut self, op: &str, body: Value, now: u64, out: &mut Vec<Out>) -> Reply {
        match op {
            "Ping" => Reply::ok(json!({"node": self.cfg.name, "calls": self.calls})),
            "Install" => match self.install(body, now, out) {
                Ok(v) => Reply::ok(v),
                Err(e) => Reply::no(e),
            },
            "Uninstall" => {
                let view = body["view"].as_str().unwrap_or("").to_string();
                let api = body["api"].as_str().unwrap_or("").to_string();
                let before = self.installs.len();
                let removed: Vec<Vec<String>> = self.installs.iter().filter(|((v, _, a), _)| *v == view && (api.is_empty() || *a == api)).map(|(_, i)| i.body.needs.clone()).collect();
                self.installs.retain(|(v, _, a), _| !(*v == view && (api.is_empty() || *a == api)));
                let n = before - self.installs.len();
                let needs: Vec<String> = removed.first().cloned().unwrap_or_default();
                self.event("Uninstalled", &view, json!({"api": api, "n": n, "needs": needs}), now, out);
                Reply::ok(json!(n))
            }
            "Tamper" => {
                let view = body["view"].as_str().unwrap_or("").to_string();
                if body["on"].as_bool().unwrap_or(true) {
                    self.tampered.insert(view.clone());
                    let needs = self.installs.values().find(|i| i.body.view == view).map(|i| i.body.needs.clone()).unwrap_or_default();
                    self.event("Tampered", &view, json!({"note": "this node now serves the bundle with one byte changed", "needs": needs}), now, out);
                } else {
                    self.tampered.remove(&view);
                }
                Reply::ok(Value::Null)
            }
            "Note" => {
                let kind = body["kind"].as_str().unwrap_or("Note").to_string();
                let cell = body["cell"].as_str().unwrap_or("").to_string();
                self.event(&kind, &cell, body["detail"].clone(), now, out);
                Reply::ok(Value::Null)
            }
            _ => Reply::no(format!("sim.ctl has no op {op}")),
        }
    }

    fn install(&mut self, b: Value, now: u64, out: &mut Vec<Out>) -> Result<Value, String> {
        let s = |k: &str| b[k].as_str().unwrap_or("").to_string();
        let (view, api_name, version) = (s("view"), s("api"), s("version"));
        let instance = if s("instance").is_empty() { "main".to_string() } else { s("instance") };
        let (major, minor) = version.split_once('.').map(|(a, c)| (a.parse().unwrap_or(0), c.parse().unwrap_or(0))).ok_or("version is major.minor")?;
        let desc = registry::descriptor(&api_name, major, minor).ok_or_else(|| format!("no contract {api_name}@{version}"))?;
        let v = desc["views"].as_array().and_then(|vs| vs.iter().find(|x| x["name"] == json!(view))).cloned().ok_or_else(|| format!("{api_name}@{version} has no view {view}"))?;
        let (bytes, sha) = match b["bundle_b64"].as_str() {
            Some(x) => {
                let bytes = base64::engine::general_purpose::STANDARD.decode(x).map_err(|e| e.to_string())?;
                let sha = fe_chain::sha256_hex(&bytes);
                (Some(bytes), sha)
            }
            None => (None, s("bundle_sha")),
        };
        if sha.len() != 64 {
            return Err("a bundle or its sha256 is needed".into());
        }
        let mut parts = BTreeMap::new();
        let mut part_names = vec![];
        if let Some(m) = b["parts_b64"].as_object() {
            for (path, x) in m {
                let bytes = base64::engine::general_purpose::STANDARD.decode(x.as_str().unwrap_or("")).map_err(|e| e.to_string())?;
                let sha = fe_chain::sha256_hex(&bytes);
                part_names.push((path.clone(), sha.clone()));
                parts.insert(sha, Some(bytes));
            }
        }
        if let Some(m) = b["parts_sha"].as_object() {
            for (path, x) in m {
                let sha = x.as_str().unwrap_or("").to_string();
                if sha.len() != 64 {
                    return Err(format!("part {path}: not a sha256"));
                }
                part_names.push((path.clone(), sha.clone()));
                parts.insert(sha, None);
            }
        }
        part_names.sort();
        let commit = if self.cfg.commit.is_empty() { "0".repeat(40) } else { self.cfg.commit.clone() };
        let dir = s("dir");
        let path = if s("ref_path").is_empty() { format!("cells/{dir}/ui") } else { s("ref_path") };
        let bundle_ref = format!("repo:live-frontend/{path}@{commit} sha256:{sha}");
        ce_ref_id::Ref::parse(&bundle_ref).map_err(|e| format!("bundle ref {bundle_ref}: {e:?}"))?;
        let strs = |k: &str| -> Vec<String> { v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() };
        let props = v["props"].as_str().unwrap_or("()").to_string();
        let props_fields = lf_contract::parse_types(desc["types"].as_str().unwrap_or(""))
            .into_iter()
            .find_map(|t| match t {
                lf_contract::TypeDef::Struct { name, fields } if name == props => Some(fields),
                _ => None,
            })
            .unwrap_or_default();
        let api = format!("{api_name}@{major}");
        let body = EntryBody {
            view: view.clone(), instance: instance.clone(), api: api.clone(), version: version.clone(),
            schema: desc["schema"].as_str().unwrap_or("").into(), render: v["render"].as_str().unwrap_or("").into(),
            bundle: sha.clone(), bundle_ref, parts: part_names,
            needs: strs("needs"), place: v["place"].as_str().unwrap_or("main").into(), data: strs("data"), tries: strs("tries"),
            streams: strs("streams"), props, props_fields, state: v["state"].as_str().unwrap_or("()").into(),
            slots: v["slots"].as_array().map(|a| a.iter().map(|x| SlotDecl { name: x["name"].as_str().unwrap_or("").into(), render: x["render"].as_str().unwrap_or("").into(), props: x["props"].as_str().unwrap_or("").into() }).collect()).unwrap_or_default(),
            speaks: strs("speaks"), hears: strs("hears"),
            supersedes: b["supersedes"].as_str().map(String::from),
            until: b["until_ms"].as_u64().map(|d| now + d),
            label: s("label"),
        };
        if !api_name.starts_with("fe.") && registry::make_cell(&api_name).is_some() {
            let c = self.cells.entry(api_name.clone()).or_insert_with(|| registry::make_cell(&api_name).expect("a cell for every contract"));
            c.serve(major, minor);
        }
        let peers: Vec<String> = b["peers"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        self.installs.retain(|(v, i, a), _| !(*v == view && *i == instance && *a == api));
        self.installs.insert((view.clone(), instance, api), Install { body: body.clone(), peers, bytes, parts });
        self.event("Installed", &view, json!({"version": version, "bundle": &sha[..12], "needs": body.needs}), now, out);
        self.announce(&body, out);
        Ok(json!({"bundle": sha}))
    }

    fn announce(&self, body: &EntryBody, out: &mut Vec<Out>) {
        let a = Signed { body: body.clone(), node: self.cfg.name.clone(), sig: self.id.sign(&entry_bytes(body)) };
        out.push(Out { to: self.cfg.door.clone(), api: "page.compose@1".into(), op: "Announce".into(), body: to(&a), tag: None });
    }

    fn tick(&mut self, now: u64, out: &mut Vec<Out>) {
        if now >= self.last_beat + 1000 {
            self.last_beat = now;
            let me = self.cfg.name.clone();
            for c in self.cells.values_mut() {
                c.beat(&me, self.calls, now);
            }
            for n in self.nodes.iter().filter(|n| **n != me) {
                out.push(Out { to: n.clone(), api: "fe.beat@1".into(), op: "Beat".into(), body: json!({"calls": self.calls}), tag: None });
            }
        }
        if now >= self.last_announce + 2000 {
            self.last_announce = now;
            let bodies: Vec<EntryBody> = self.installs.values().map(|i| i.body.clone()).collect();
            for b in &bodies {
                if b.until.is_some_and(|u| u <= now) {
                    continue;
                }
                self.announce(b, out);
            }
        }
        let caller = Caller::Node(self.cfg.name.clone());
        let (node, door) = (self.cfg.name.clone(), self.cfg.door.clone());
        let nodes: Vec<String> = self.nodes.iter().filter(|n| **n != node).cloned().collect();
        if let Some(m) = self.compose.as_mut() {
            m.tick(&mut Cx { now, node: &node, caller: &caller, peers: &[], nodes: &nodes, door: &door, out });
        }
        let names: Vec<String> = self.cells.keys().cloned().collect();
        for name in names {
            let (peers, _) = self.cx_parts(&name);
            if let Some(c) = self.cells.get_mut(&name) {
                c.tick(&mut Cx { now, node: &node, caller: &caller, peers: &peers, nodes: &nodes, door: &door, out });
            }
        }
    }

    /// For `http.serve` and tests: the tree `page.compose` answers a chain with `rights`.
    pub fn tree(&self, rights: &[String], now: u64) -> Vec<fe_runtime::api::compose::Entry> {
        self.compose.as_ref().map(|c| c.tree(rights, now)).unwrap_or_default()
    }

    /// For tests and the page: the door node's history.
    pub fn history_len(&self) -> usize {
        self.history.as_ref().map(|h| h.events.len()).unwrap_or(0)
    }
}

/// The static fragment of a view for first paint and for readers without scripts: the panel frame the shell
/// would draw, with the cell's own text inside when it has one.
pub fn html_fragment(e: &EntryBody, inner: Option<&str>) -> String {
    let esc = |x: &str| x.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let api = e.api.split('@').next().unwrap_or("");
    let inst = if e.instance == "main" { String::new() } else { format!(" · {}", esc(&e.instance)) };
    format!(
        "<section class=\"cellpanel\" data-view=\"{v}\" data-render=\"{r}\"><div class=\"cellhead\">{v}{inst} · {api}@{ver} · sha256 {sha}</div><div class=\"cellbody\">{body}</div></section>",
        v = esc(&e.view), r = esc(&e.render), ver = esc(&e.version), sha = &e.bundle[..10.min(e.bundle.len())],
        body = inner.map(String::from).unwrap_or_else(|| format!("<p>{}</p>", esc(if e.label.is_empty() { &e.view } else { &e.label })))
    )
}
