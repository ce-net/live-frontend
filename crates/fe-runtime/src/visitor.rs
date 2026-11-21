//! fe.visitor: the issuer. A browser brings a fresh public key and a small proof of work and gets a chain
//! root -> issuer -> this key, narrowed to the visitor rights, 15 minutes, 40 counted uses, a rate, and a row
//! caveat that keeps its writes in its own sandbox. The issuer also keeps the one counter row per chain and
//! signs a spend ticket for each counted call.
use crate::api::visitor::*;
use crate::{parse, to, Caller, Cell, Cx};
use fe_chain::{fingerprint, Caveats, Chain, Identity, Link};
use fe_gate::Ticket;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Exactly the rights of a visitor chain. `ops:render:panel`, `ops:read`, `notes:admin`, every `:announce` and
/// every admin right are not here, and not in the issuer's grant either, so the issuer cannot mint them. A view is
/// seen only through its render right; `cargo cell check` fails if a render right here lacks its view's data rights.
pub const VISITOR_RIGHTS: &[&str] = &[
    "page.compose:resolve", "page.compose:report", "fe.shell:render:page",
    "fe.history:render:timeline", "fe.history:read", "fe.history:observe",
    "fe.explain:render:inspect", "fe.explain:mine", "fe.visitor:revoke-own", "fe.visitor:state",
    "systems.catalog:render:card", "systems.catalog:read",
    "notes:render:board", "notes:read", "notes:write",
    "status:render:panel", "status:read", "trend:render:chart", "trend:read",
    "faults:render:panel", "faults:run",
];

/// The anonymous chain's table (no join, no key): what `http.serve` uses for `GET /` and what a static export
/// shows. Read-only render and compose rights; no data op that writes, and nothing counted.
pub const ANONYMOUS_RIGHTS: &[&str] = &[
    "page.compose:resolve", "fe.shell:render:page", "systems.catalog:render:card", "systems.catalog:read",
    "status:render:panel", "status:read", "trend:render:chart", "trend:read", "fe.history:render:timeline", "fe.history:read",
];

pub struct Policy {
    pub ttl_ms: u64,
    pub count: u32,
    pub rate: u32,
    pub mint_per_day: u32,
    pub pow_zeros: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Policy { ttl_ms: 15 * 60_000, count: 40, rate: 20, mint_per_day: 500, pow_zeros: 3 }
    }
}

#[derive(Debug, Clone)]
pub struct St {
    pub holder: String,
    pub spent: u32,
    pub count: u32,
    pub until: u64,
    pub revoked: bool,
}

pub struct Visitor {
    pub issuer: Identity,
    pub grant: Link,
    pub policy: Policy,
    pub chains: BTreeMap<String, St>,
    day: (u64, u32),
}

pub fn pow_ok(holder: &str, pow: &str, zeros: usize) -> bool {
    fe_chain::sha256_hex(format!("{holder}:{pow}").as_bytes()).starts_with(&"0".repeat(zeros))
}

impl Visitor {
    pub fn new(issuer: Identity, grant: Link, policy: Policy) -> Visitor {
        Visitor { issuer, grant, policy, chains: BTreeMap::new(), day: (0, 0) }
    }

    pub fn join(&mut self, j: JoinReq, cx: &mut Cx) -> Result<Joined, String> {
        if hex_key(&j.holder).is_none() {
            return Err("holder is not an Ed25519 public key".into());
        }
        if !pow_ok(&j.holder, &j.pow, self.policy.pow_zeros) {
            return Err("proof of work does not check".into());
        }
        let day = cx.now / 86_400_000;
        if self.day.0 != day {
            self.day = (day, 0);
        }
        let mint = self.grant.caveats.mint.unwrap_or(self.policy.mint_per_day).min(self.policy.mint_per_day);
        if self.day.1 >= mint {
            return Err(format!("mint limit: {mint} chains today"));
        }
        self.day.1 += 1;
        let until = (cx.now + self.policy.ttl_ms).min(self.grant.caveats.until - 1);
        let fp = fingerprint(&j.holder);
        let link = self.issuer.grant(
            &j.holder,
            VISITOR_RIGHTS.iter().map(|s| s.to_string()).collect(),
            Caveats { until, count: Some(self.policy.count), rate: Some(self.policy.rate), rows: Some(("sandbox".into(), fp.clone())), mint: None },
        );
        let chain = Chain(vec![self.grant.clone(), link]);
        let id = chain.id();
        self.chains.insert(id.clone(), St { holder: j.holder.clone(), spent: 0, count: self.policy.count, until, revoked: false });
        cx.event("Joined", "fe.visitor", json!({"chain": id, "visitor": fp, "until": until, "uses": self.policy.count}));
        Ok(Joined { chain, id, until, uses: self.policy.count })
    }

    /// The anonymous chain: what `http.serve` composes `/` with, for a reader that has not joined. Read-only render
    /// and compose rights (`ANONYMOUS_RIGHTS`), no count (nothing it holds is counted), the same proof of work, the
    /// same mint limit, 15 minutes.
    pub fn anon(&mut self, j: JoinReq, cx: &mut Cx) -> Result<Joined, String> {
        if hex_key(&j.holder).is_none() {
            return Err("holder is not an Ed25519 public key".into());
        }
        if !pow_ok(&j.holder, &j.pow, self.policy.pow_zeros) {
            return Err("proof of work does not check".into());
        }
        let day = cx.now / 86_400_000;
        if self.day.0 != day {
            self.day = (day, 0);
        }
        let mint = self.grant.caveats.mint.unwrap_or(self.policy.mint_per_day).min(self.policy.mint_per_day);
        if self.day.1 >= mint {
            return Err(format!("mint limit: {mint} chains today"));
        }
        self.day.1 += 1;
        let until = (cx.now + self.policy.ttl_ms).min(self.grant.caveats.until - 1);
        let link = self.issuer.grant(
            &j.holder,
            ANONYMOUS_RIGHTS.iter().map(|s| s.to_string()).collect(),
            // one chain serves every anonymous reader of one http.serve cell, through its cache: a higher rate
            Caveats { until, count: None, rate: Some(self.policy.rate * 10), rows: None, mint: None },
        );
        let chain = Chain(vec![self.grant.clone(), link]);
        let id = chain.id();
        self.chains.insert(id.clone(), St { holder: j.holder.clone(), spent: 0, count: 0, until, revoked: false });
        cx.event("Joined", "fe.visitor", json!({"chain": id, "visitor": fingerprint(&j.holder), "until": until, "anonymous": true}));
        Ok(Joined { chain, id, until, uses: 0 })
    }

    /// One use of a counted chain: a ticket bound to this call, or why not.
    pub fn spend(&mut self, id: &str, call_sha: &str, cx: &mut Cx) -> Result<Ticket, String> {
        let s = self.chains.get_mut(id).ok_or("chain not issued here")?;
        if s.revoked {
            return Err("chain revoked".into());
        }
        if s.spent >= s.count {
            let c = s.count;
            cx.event("Exhausted", "fe.visitor", json!({"chain": id, "uses": c}));
            return Err(format!("uses exhausted ({c} of {c} spent)"));
        }
        s.spent += 1;
        let n = s.spent;
        Ok(Ticket { chain: id.into(), n, sig: self.issuer.sign(&fe_chain::ticket_digest(id, n, call_sha)) })
    }

    pub fn state(&self, id: &str) -> ChainState {
        match self.chains.get(id) {
            Some(s) => ChainState { id: id.into(), uses_left: s.count - s.spent, uses: s.count, until: s.until, revoked: s.revoked },
            None => ChainState { id: id.into(), uses_left: 0, uses: 0, until: 0, revoked: false },
        }
    }
}

fn hex_key(h: &str) -> Option<[u8; 32]> {
    hex::decode(h).ok()?.try_into().ok()
}

impl Cell for Visitor {
    fn name(&self) -> &'static str {
        "fe.visitor"
    }
    fn serve(&mut self, _: u32, _: u32) {}
    fn handle(&mut self, _major: u32, op: &str, body: Value, cx: &mut Cx) -> Result<Value, String> {
        match op {
            "Join" => Ok(to(&self.join(parse(body)?, cx)?)),
            "State" => Ok(to(&self.state(&cx.caller.chain()))),
            "Revoke" => {
                let id = cx.caller.chain();
                let Caller::Visitor(_) = cx.caller else { return Err("Revoke takes the caller's own chain".into()) };
                if let Some(s) = self.chains.get_mut(&id) {
                    s.revoked = true;
                }
                let mut all = cx.nodes.to_vec();
                all.push(cx.node.to_string());
                for n in all {
                    cx.send(&n, "fe.gate@1", "Revoke", json!({"chain": id}));
                }
                cx.event("Revoked", "fe.visitor", json!({"chain": id}));
                Ok(to(&self.state(&id)))
            }
            "Spend" => {
                let s: SpendReq = parse(body)?;
                Ok(to(&self.spend(&s.chain, &s.call_sha, cx)?))
            }
            _ => Err(format!("fe.visitor has no op {op}")),
        }
    }
}
