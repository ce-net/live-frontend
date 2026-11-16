//! A sim network: keys and configs for four nodes from one seed, and an in-process mesh that delivers every
//! message in order (for tests and for replaying an input log). The native processes and the wasm workers use
//! the same configs.
use crate::{Config, Forward, Host, Input, Member, Output, Reply};
use fe_chain::{Caveats, Identity};
use fe_gate::{SignedCall, VisitorCall};
use fe_runtime::visitor::VISITOR_RIGHTS;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};

pub const NODES: [(&str, &str); 4] = [("a", "sim"), ("b", "sim"), ("c", "sim"), ("d", "sim")];

/// Configs for nodes a, b, c (serving) and d (door), all derived from `seed`. The issuer grant lasts 90 days
/// from `now`, carries exactly the visitor rights and a mint limit, like the grant section 7.3 asks for.
pub fn configs(seed: &str, now: u64, commit: &str) -> (Identity, Vec<Config>) {
    let root = Identity::from_seed(fe_chain::sha256_hex(format!("lf-root:{seed}").as_bytes()).as_bytes()[..32].try_into().unwrap()).derive("root");
    let ids: Vec<(String, String, Identity)> = NODES.iter().map(|(n, c)| (n.to_string(), c.to_string(), root.derive(&format!("node:{n}")))).collect();
    let issuer = root.derive("issuer:fe.visitor");
    let grant = root.grant(
        &issuer.public_hex(),
        VISITOR_RIGHTS.iter().map(|s| s.to_string()).collect(),
        Caveats { until: now + 90 * 86_400_000, mint: Some(500), ..Default::default() },
    );
    let members: Vec<Member> = ids.iter().map(|(n, c, i)| Member { name: n.clone(), class: c.clone(), key: i.public_hex() }).collect();
    let cfgs = ids
        .iter()
        .map(|(n, _, i)| Config {
            name: n.clone(), seed: i.seed_hex(), root: root.public_hex(), nodes: members.clone(), door: "d".into(),
            issuer: (n == "d").then(|| (issuer.seed_hex(), grant.clone())), commit: commit.into(), mode: "sim".into(), pow_zeros: Some(2),
        })
        .collect();
    (root, cfgs)
}

/// A browser-side holder in Rust (tests, the recorder): signs calls like the page does with WebCrypto.
pub struct Holder {
    pub id: Identity,
    pub chain: fe_chain::Chain,
    n: u64,
}

impl Holder {
    pub fn new(seed: u8) -> Holder {
        Holder { id: Identity::from_seed([seed; 32]), chain: fe_chain::Chain::default(), n: 0 }
    }
    pub fn pow(&self, zeros: usize) -> String {
        (0u64..).map(|i| i.to_string()).find(|p| fe_runtime::visitor::pow_ok(&self.id.public_hex(), p, zeros)).unwrap()
    }
    pub fn call(&mut self, to: &str, api: &str, op: &str, body: Value, now: u64) -> Value {
        self.n += 1;
        let call = SignedCall { api: api.into(), op: op.into(), body: body.to_string(), ts: now, nonce: format!("n{}", self.n) };
        let vc = VisitorCall { hsig: self.id.sign(&call.digest()), call, chain: self.chain.clone(), ticket: None };
        json!({"to": to, "vc": vc})
    }
}

pub struct LocalMesh {
    pub hosts: BTreeMap<String, Host>,
    pub down: std::collections::BTreeSet<String>,
    pub now: u64,
    /// Every input each host received, in order (what a recorded run stores).
    pub log: Vec<(String, Input)>,
}

impl LocalMesh {
    pub fn new(cfgs: Vec<Config>, now: u64) -> LocalMesh {
        let hosts = cfgs.into_iter().map(|c| (c.name.clone(), Host::new(c).unwrap())).collect();
        LocalMesh { hosts, down: Default::default(), now, log: vec![] }
    }

    fn feed(&mut self, node: &str, inp: Input) -> Option<Output> {
        if self.down.contains(node) {
            return None;
        }
        self.log.push((node.into(), inp.clone()));
        Some(self.hosts.get_mut(node)?.input(inp))
    }

    fn run(&mut self, from: &str, o: Output) {
        let mut q: VecDeque<(String, fe_runtime::Out)> = o.out.into_iter().map(|m| (from.to_string(), m)).collect();
        while let Some((src, m)) = q.pop_front() {
            let tag = m.tag.clone();
            if let Some(o) = self.feed(&m.to, Input::Frame { from: src.clone(), api: m.api, op: m.op, body: m.body, now: self.now }) {
                let to = m.to.clone();
                let r = o.reply.clone();
                q.extend(o.out.into_iter().map(|x| (to.clone(), x)));
                if let (Some(tag), Some(r)) = (tag, r) {
                    if let Some(o2) = self.feed(&src, Input::Reply { tag, ok: r.ok, body: r.body, now: self.now }) {
                        q.extend(o2.out.into_iter().map(|x| (src.clone(), x)));
                    }
                }
            }
        }
    }

    pub fn ctl(&mut self, node: &str, op: &str, body: Value) -> Reply {
        let o = self.feed(node, Input::Frame { from: "root".into(), api: "sim.ctl".into(), op: op.into(), body, now: self.now }).expect("node is up");
        let r = o.reply.clone().unwrap();
        self.run(node, o);
        r
    }

    pub fn tick(&mut self, ms: u64) {
        self.now += ms;
        let names: Vec<String> = self.hosts.keys().cloned().collect();
        for n in names {
            if let Some(o) = self.feed(&n, Input::Tick { now: self.now }) {
                self.run(&n, o);
            }
        }
    }

    /// A browser frame to the door, forwarded to its node when the door says so.
    pub fn door(&mut self, op: &str, body: Value) -> Reply {
        let o = self.feed("d", Input::Frame { from: "browser".into(), api: "fe.door@1".into(), op: op.into(), body, now: self.now }).unwrap();
        let r = o.reply.clone().unwrap();
        let f = o.forward.clone();
        self.run("d", o);
        match f {
            Some(Forward { to, api, op, body }) => match self.feed(&to, Input::Frame { from: "d".into(), api, op, body, now: self.now }) {
                Some(o) => {
                    let r = o.reply.clone().unwrap();
                    self.run(&to, o);
                    r
                }
                None => Reply { ok: false, body: Value::Null, refused: Some(format!("{to}: unreachable")) },
            },
            None => r,
        }
    }
}
