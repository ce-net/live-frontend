//! The gate. A node runs one in front of all its cells and checks every call from a visitor again, whatever the
//! browser's bus already allowed: the chain (signatures, narrowing, root, expiry), revocation, the holder's
//! signature over these exact call bytes, freshness and replay, the op's right, the rate caveat, and for a
//! counted op the issuer's spend ticket bound to this call. The refusal is one shape, printed verbatim on the page.
use fe_chain::{call_digest, holds, sha256_hex, ticket_digest, verify_sig, Chain, Effective};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedCall {
    pub api: String,
    pub op: String,
    /// The JSON body exactly as the holder signed it.
    pub body: String,
    pub ts: u64,
    pub nonce: String,
}

impl SignedCall {
    pub fn digest(&self) -> Vec<u8> {
        call_digest(&self.api, &self.op, &self.body, self.ts, &self.nonce)
    }
    pub fn sha(&self) -> String {
        sha256_hex(&self.digest())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub chain: String,
    pub n: u32,
    pub sig: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisitorCall {
    pub call: SignedCall,
    pub chain: Chain,
    pub hsig: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<Ticket>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refused {
    pub need: String,
    pub chain: String,
    pub node: String,
    pub why: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Refused{{need: \"{}\", chain: {}, node: {}}} ({})", self.need, self.chain, self.node, self.why)
    }
}

pub const FRESH_MS: u64 = 120_000;

#[derive(Default, Serialize, Deserialize)]
pub struct Gate {
    /// This node's fingerprint, written into every refusal.
    pub node: String,
    /// The network root's public key.
    pub root: String,
    pub revoked: BTreeSet<String>,
    nonces: BTreeMap<String, BTreeSet<String>>,
    rate: BTreeMap<String, (u64, u32)>,
    tickets: BTreeMap<String, BTreeSet<u32>>,
}

impl Gate {
    pub fn new(node: &str, root: &str) -> Gate {
        Gate { node: node.into(), root: root.into(), ..Default::default() }
    }

    fn no(&self, need: &str, chain: &str, why: impl Into<String>) -> Refused {
        Refused { need: need.into(), chain: chain.into(), node: self.node.clone(), why: why.into() }
    }

    /// Checks one visitor call against the right `need`. `counted`: the op spends one use of a counted chain.
    pub fn check(&mut self, vc: &VisitorCall, need: &str, counted: bool, now: u64) -> Result<Effective, Refused> {
        let id = vc.chain.id();
        let e = vc.chain.verify(&self.root, now).map_err(|w| self.no(need, &id, w))?;
        if self.revoked.contains(&id) {
            return Err(self.no(need, &id, "chain revoked"));
        }
        if !verify_sig(&e.holder, &vc.call.digest(), &vc.hsig) {
            return Err(self.no(need, &id, "call not signed by the chain's holder"));
        }
        if vc.call.ts + FRESH_MS < now || vc.call.ts > now + FRESH_MS {
            return Err(self.no(need, &id, "call too old or from the future"));
        }
        let seen = self.nonces.entry(id.clone()).or_default();
        if !seen.insert(vc.call.nonce.clone()) {
            return Err(self.no(need, &id, "replayed call"));
        }
        if seen.len() > 4096 {
            let first = seen.iter().next().cloned().unwrap();
            seen.remove(&first);
        }
        if !holds(&e.rights, need) {
            return Err(self.no(need, &id, "right not held"));
        }
        if let Some(max) = e.caveats.rate {
            let sec = now / 1000;
            let r = self.rate.entry(id.clone()).or_insert((sec, 0));
            if r.0 != sec {
                *r = (sec, 0);
            }
            r.1 += 1;
            if r.1 > max {
                return Err(self.no(need, &id, format!("rate: more than {max} calls this second")));
            }
        }
        if counted {
            if let Some(max) = e.caveats.count {
                let t = vc.ticket.as_ref().ok_or_else(|| self.no(need, &id, "counted op without a spend ticket"))?;
                if t.chain != id || !verify_sig(&e.issuer, &ticket_digest(&id, t.n, &vc.call.sha()), &t.sig) {
                    return Err(self.no(need, &id, "spend ticket not signed by the chain's issuer for this call"));
                }
                if t.n == 0 || t.n > max {
                    return Err(self.no(need, &id, format!("uses exhausted ({max} of {max} spent)")));
                }
                if !self.tickets.entry(id.clone()).or_default().insert(t.n) {
                    return Err(self.no(need, &id, "spend ticket used twice"));
                }
            }
        }
        Ok(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fe_chain::{Caveats, Identity};

    fn setup() -> (Identity, Identity, Identity, Chain) {
        let (root, iss, vis) = (Identity::from_seed([1; 32]), Identity::from_seed([2; 32]), Identity::from_seed([3; 32]));
        let l0 = root.grant(&iss.public_hex(), vec!["notes:read".into(), "notes:write".into()], Caveats { until: 10_000_000, ..Default::default() });
        let l1 = iss.grant(&vis.public_hex(), vec!["notes:read".into(), "notes:write".into()], Caveats { until: 9_000_000, count: Some(2), rate: Some(3), ..Default::default() });
        (root, iss, vis, Chain(vec![l0, l1]))
    }
    fn call(vis: &Identity, chain: &Chain, op: &str, nonce: &str, now: u64) -> VisitorCall {
        let c = SignedCall { api: "notes@1".into(), op: op.into(), body: "{}".into(), ts: now, nonce: nonce.into() };
        VisitorCall { hsig: vis.sign(&c.digest()), call: c, chain: chain.clone(), ticket: None }
    }

    #[test]
    fn allows_then_refuses_with_one_shape() {
        let (root, iss, vis, chain) = setup();
        let mut g = Gate::new("node-a", &root.public_hex());
        assert!(g.check(&call(&vis, &chain, "List", "1", 1000), "notes:read", false, 1000).is_ok());
        let r = g.check(&call(&vis, &chain, "Purge", "2", 1000), "notes:admin", false, 1000).unwrap_err();
        assert_eq!(r.need, "notes:admin");
        assert!(r.to_string().starts_with("Refused{need: \"notes:admin\", chain: "));
        // replay
        assert!(g.check(&call(&vis, &chain, "List", "1", 1000), "notes:read", false, 1000).unwrap_err().why.contains("replayed"));
        // a stranger's signature
        let mut vc = call(&vis, &chain, "List", "3", 1000);
        vc.hsig = iss.sign(&vc.call.digest());
        assert!(g.check(&vc, "notes:read", false, 1000).unwrap_err().why.contains("holder"));
        // rate: 3 per second
        for n in 0..3 {
            let _ = g.check(&call(&vis, &chain, "List", &format!("r{n}"), 2000), "notes:read", false, 2000);
        }
        assert!(g.check(&call(&vis, &chain, "List", "r9", 2000), "notes:read", false, 2000).unwrap_err().why.contains("rate"));
        // revoked
        g.revoked.insert(chain.id());
        assert!(g.check(&call(&vis, &chain, "List", "z", 5000), "notes:read", false, 5000).unwrap_err().why.contains("revoked"));
    }

    #[test]
    fn counted_needs_a_ticket_for_this_call() {
        let (root, iss, vis, chain) = setup();
        let mut g = Gate::new("node-a", &root.public_hex());
        let mut vc = call(&vis, &chain, "Put", "1", 1000);
        assert!(g.check(&vc, "notes:write", true, 1000).unwrap_err().why.contains("ticket"));
        let mut vc2 = call(&vis, &chain, "Put", "2", 1000);
        vc.ticket = Some(Ticket { chain: chain.id(), n: 1, sig: iss.sign(&ticket_digest(&chain.id(), 1, &vc.call.sha())) });
        vc2.ticket = Some(Ticket { chain: chain.id(), n: 2, sig: iss.sign(&ticket_digest(&chain.id(), 1, &vc.call.sha())) });
        assert!(g.check(&vc2, "notes:write", true, 1000).unwrap_err().why.contains("issuer"));
        vc.call.nonce = "1b".into();
        vc.hsig = vis.sign(&vc.call.digest());
        vc.ticket = Some(Ticket { chain: chain.id(), n: 1, sig: iss.sign(&ticket_digest(&chain.id(), 1, &vc.call.sha())) });
        assert!(g.check(&vc, "notes:write", true, 1000).is_ok());
        let mut vc3 = call(&vis, &chain, "Put", "3", 3000);
        vc3.ticket = Some(Ticket { chain: chain.id(), n: 3, sig: iss.sign(&ticket_digest(&chain.id(), 3, &vc3.call.sha())) });
        assert!(g.check(&vc3, "notes:write", true, 3000).unwrap_err().why.contains("exhausted"));
    }
}
