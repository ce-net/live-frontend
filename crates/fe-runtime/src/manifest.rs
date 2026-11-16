//! fe.manifest: signed entries with a five-second lease, answered per viewer. A cell that stops renewing leaves
//! the page by itself; a viewer is never sent an entry whose `needs` its chain does not hold.
use crate::api::manifest::*;
use crate::{parse, to, Caller, Cell, Cx};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const LEASE_MS: u64 = 5_000;

/// What a node signs for an entry: `lf-entry\n` and the entry as JSON with sorted keys (the form every reader,
/// including the browser, gets it in).
pub fn entry_bytes(b: &EntryBody) -> Vec<u8> {
    let mut v = b"lf-entry\n".to_vec();
    v.extend(serde_json::to_vec(&serde_json::to_value(b).unwrap()).unwrap());
    v
}

type Key = (String, String, String); // view, instance, api

#[derive(Default)]
pub struct Manifest {
    /// node name -> public key (hex), from the roster.
    pub keys: BTreeMap<String, String>,
    pub held: BTreeMap<Key, BTreeMap<String, (Signed, u64)>>,
    pub seq: u64,
}

fn version_key(v: &str) -> (u32, u32) {
    let mut p = v.split('.').map(|x| x.parse::<u32>().unwrap_or(0));
    (p.next().unwrap_or(0), p.next().unwrap_or(0))
}

impl Manifest {
    pub fn new(keys: BTreeMap<String, String>) -> Manifest {
        Manifest { keys, ..Default::default() }
    }

    /// Every live entry: per (view, instance, api) the newest version, with the holders that announce it.
    pub fn live(&self, now: u64) -> Vec<Entry> {
        let mut out = vec![];
        for by in self.held.values() {
            let live: Vec<&(Signed, u64)> = by.values().filter(|(_, l)| *l > now).collect();
            let Some(best) = live.iter().map(|(a, _)| &a.body).max_by_key(|b| (version_key(&b.version), b.bundle.clone())) else { continue };
            if best.until.is_some_and(|u| u <= now) {
                continue;
            }
            let holders = live
                .iter()
                .filter(|(a, _)| a.body == *best)
                .map(|(a, l)| Holder { node: a.node.clone(), sig: a.sig.clone(), lease_until: *l })
                .collect();
            out.push(Entry { body: best.clone(), holders });
        }
        out
    }

    fn expire(&mut self, cx: &mut Cx) {
        let now = cx.now;
        let mut gone = vec![];
        for (k, by) in self.held.iter_mut() {
            let dead: Vec<String> = by.iter().filter(|(_, (_, l))| *l <= now).map(|(n, _)| n.clone()).collect();
            for n in dead {
                let (a, _) = by.remove(&n).unwrap();
                gone.push((k.0.clone(), n, a.body.version.clone(), a.body.needs.clone()));
            }
        }
        self.held.retain(|_, by| !by.is_empty());
        for (view, node, version, needs) in gone {
            self.seq += 1;
            cx.event("Gone", &view, json!({"holder": node, "version": version, "needs": needs}));
        }
    }
}

impl Cell for Manifest {
    fn name(&self) -> &'static str {
        "fe.manifest"
    }
    fn serve(&mut self, _: u32, _: u32) {}
    fn tick(&mut self, cx: &mut Cx) {
        self.expire(cx);
    }
    fn handle(&mut self, _major: u32, op: &str, body: Value, cx: &mut Cx) -> Result<Value, String> {
        match op {
            "Announce" => {
                let a: Signed = parse(body)?;
                let Caller::Node(from) = cx.caller else { return Err("only a node announces".into()) };
                if &a.node != from {
                    return Err(format!("{from} announced for {}", a.node));
                }
                let key = self.keys.get(&a.node).ok_or("not a node of this mesh")?;
                if !fe_chain::verify_sig(key, &entry_bytes(&a.body), &a.sig) {
                    return Err("entry signature does not verify".into());
                }
                let k = (a.body.view.clone(), a.body.instance.clone(), a.body.api.clone());
                let by = self.held.entry(k).or_default();
                let prev = by.get(&a.node).map(|(p, l)| (p.body.clone(), *l > cx.now));
                let lease = cx.now + LEASE_MS;
                let (view, version, bundle, needs) = (a.body.view.clone(), a.body.version.clone(), a.body.bundle.clone(), a.body.needs.clone());
                by.insert(a.node.clone(), (a, lease));
                match prev {
                    Some((p, true)) if p.bundle == bundle && p.version == version => {}
                    Some((p, _)) => {
                        self.seq += 1;
                        cx.event("Changed", &view, json!({"holder": from, "from": p.version, "to": version, "bundle": &bundle[..12], "needs": needs}));
                    }
                    None => {
                        self.seq += 1;
                        cx.event("Announced", &view, json!({"holder": from, "version": version, "bundle": &bundle[..12], "needs": needs}));
                    }
                }
                Ok(json!(self.seq))
            }
            "Resolve" => {
                let rights = cx.caller.rights();
                let entries = self.live(cx.now).into_iter().filter(|e| e.body.needs.iter().all(|n| fe_chain::holds(&rights, n))).collect();
                Ok(to(&Resolved { seq: self.seq, entries }))
            }
            "All" => Ok(to(&self.live(cx.now))),
            "Report" => {
                let r: Complaint = parse(body)?;
                cx.event("Refused", &r.view, json!({"holder": r.holder, "bundle": r.bundle, "why": r.why, "reported_by": cx.caller.chain()}));
                Ok(Value::Null)
            }
            _ => Err(format!("fe.manifest has no op {op}")),
        }
    }
}
