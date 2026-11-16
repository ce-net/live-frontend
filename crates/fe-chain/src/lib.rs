//! Capability chains, the cap v3 shape (the slice in live-shop's `shop-chain`, extended).
//!
//! A chain is a list of links. The first is signed by the root key; every later link is signed by the holder of
//! the link before it and may only narrow it: fewer rights, the same or more caveats, an earlier expiry. A call
//! carries the chain and a signature by the leaf holder over the exact call bytes (`call_digest`), so a chain
//! copied off the wire is useless without the holder's key. Caveats: `until` (expiry), `count` (counted uses,
//! spent through tickets the chain's issuer signs), `rate` (calls per second at each gate), `rows` (a `where`
//! caveat: only rows whose field equals the value), `mint` (how many chains a day an issuer may sign).
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Caveats {
    /// Unix ms after which the link is void.
    pub until: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<u32>,
    /// `where field == value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mint: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub issuer: String,
    pub holder: String,
    pub rights: Vec<String>,
    pub caveats: Caveats,
    pub sig: String,
}

#[derive(Serialize)]
struct LinkBody<'a> {
    issuer: &'a str,
    holder: &'a str,
    rights: &'a [String],
    caveats: &'a Caveats,
}

impl Link {
    pub fn body_bytes(&self) -> Vec<u8> {
        let b = LinkBody { issuer: &self.issuer, holder: &self.holder, rights: &self.rights, caveats: &self.caveats };
        let mut v = b"lf-link\n".to_vec();
        v.extend(serde_json::to_vec(&b).unwrap());
        v
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Chain(pub Vec<Link>);

/// What a verified chain grants its leaf holder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effective {
    pub id: String,
    pub issuer: String,
    pub holder: String,
    pub rights: Vec<String>,
    pub caveats: Caveats,
}

#[derive(Clone)]
pub struct Identity(SigningKey);

impl Identity {
    pub fn from_seed(seed: [u8; 32]) -> Identity {
        Identity(SigningKey::from_bytes(&seed))
    }
    /// Deterministic child key: `sha256(seed || label)`.
    pub fn derive(&self, label: &str) -> Identity {
        let mut h = Sha256::new();
        h.update(self.0.to_bytes());
        h.update(label.as_bytes());
        Identity::from_seed(h.finalize().into())
    }
    pub fn seed_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }
    pub fn from_seed_hex(s: &str) -> Option<Identity> {
        let b: [u8; 32] = hex::decode(s.trim()).ok()?.try_into().ok()?;
        Some(Identity::from_seed(b))
    }
    pub fn public_hex(&self) -> String {
        hex::encode(self.0.verifying_key().to_bytes())
    }
    pub fn sign(&self, msg: &[u8]) -> String {
        hex::encode(self.0.sign(msg).to_bytes())
    }
    pub fn key(&self) -> &SigningKey {
        &self.0
    }
    /// Signs a link from this identity to `holder`.
    pub fn grant(&self, holder: &str, rights: Vec<String>, caveats: Caveats) -> Link {
        let mut l = Link { issuer: self.public_hex(), holder: holder.into(), rights, caveats, sig: String::new() };
        l.sig = self.sign(&l.body_bytes());
        l
    }
}

/// The short form a page shows: the first 12 hex digits of sha256 of the public key.
pub fn fingerprint(pub_hex: &str) -> String {
    hex::encode(Sha256::digest(pub_hex.as_bytes()))[..12].to_string()
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

pub fn verify_sig(pub_hex: &str, msg: &[u8], sig_hex: &str) -> bool {
    let Some(k) = hex::decode(pub_hex).ok().and_then(|v| <[u8; 32]>::try_from(v).ok()) else { return false };
    let Ok(vk) = VerifyingKey::from_bytes(&k) else { return false };
    let Some(s) = hex::decode(sig_hex).ok().and_then(|v| <[u8; 64]>::try_from(v).ok()) else { return false };
    vk.verify(msg, &Signature::from_bytes(&s)).is_ok()
}

/// `granted` covers `needed` when equal, or `granted` is `"<api>:*"` and `needed` starts with `"<api>:"`.
pub fn covers(granted: &str, needed: &str) -> bool {
    granted == needed || granted.strip_suffix('*').is_some_and(|p| p.ends_with(':') && needed.starts_with(p))
}

pub fn holds(rights: &[String], needed: &str) -> bool {
    rights.iter().any(|g| covers(g, needed))
}

/// Why a link may not follow its parent.
pub fn narrower(child: &Link, parent: &Link) -> Result<(), String> {
    for r in &child.rights {
        if !holds(&parent.rights, r) {
            return Err(format!("right {r} widened"));
        }
    }
    let (c, p) = (&child.caveats, &parent.caveats);
    if c.until > p.until {
        return Err("expiry widened".into());
    }
    fn keep<T: PartialEq + PartialOrd>(name: &str, c: &Option<T>, p: &Option<T>, le: bool) -> Result<(), String> {
        match (c, p) {
            (_, None) => Ok(()),
            (Some(c), Some(p)) if (le && c <= p) || c == p => Ok(()),
            _ => Err(format!("{name} widened")),
        }
    }
    keep("count", &c.count, &p.count, true)?;
    keep("rate", &c.rate, &p.rate, true)?;
    keep("rows", &c.rows, &p.rows, false)?;
    Ok(())
}

impl Chain {
    /// The chain id: the first 16 hex digits of sha256 over the leaf's signature.
    pub fn id(&self) -> String {
        self.0.last().map(|l| sha256_hex(l.sig.as_bytes())[..16].to_string()).unwrap_or_default()
    }

    /// Checks every signature, that each link narrows its parent, that the root is `root`, and expiry at `now`.
    pub fn verify(&self, root: &str, now: u64) -> Result<Effective, String> {
        let links = &self.0;
        let first = links.first().ok_or("empty chain")?;
        if first.issuer != root {
            return Err("chain is not rooted at this network's root".into());
        }
        for (i, l) in links.iter().enumerate() {
            if !verify_sig(&l.issuer, &l.body_bytes(), &l.sig) {
                return Err(format!("link {i}: signature does not verify"));
            }
            if i > 0 {
                let p = &links[i - 1];
                if l.issuer != p.holder {
                    return Err(format!("link {i}: not signed by the holder of link {}", i - 1));
                }
                narrower(l, p).map_err(|e| format!("link {i}: {e}"))?;
            }
            if l.caveats.until <= now {
                return Err(format!("link {i}: expired"));
            }
        }
        let leaf = links.last().unwrap();
        Ok(Effective {
            id: self.id(),
            issuer: leaf.issuer.clone(),
            holder: leaf.holder.clone(),
            rights: leaf.rights.clone(),
            caveats: leaf.caveats.clone(),
        })
    }
}

/// The bytes a holder signs for one call. The browser signs the same string with WebCrypto Ed25519.
pub fn call_digest(api: &str, op: &str, body: &str, ts: u64, nonce: &str) -> Vec<u8> {
    format!("lf-call\n{api}\n{op}\n{body}\n{ts}\n{nonce}").into_bytes()
}

/// The bytes an issuer signs to let one counted call through: chain id, use number, and the call it is for.
pub fn ticket_digest(chain: &str, n: u32, call_sha: &str) -> Vec<u8> {
    format!("lf-spend\n{chain}\n{n}\n{call_sha}").into_bytes()
}

/// `layer_view`: the rows a `rows` caveat lets through. `None` means every row.
pub fn layer_view<'a, T>(rows: &'a [T], caveat: &Option<(String, String)>, field: impl Fn(&T, &str) -> Option<String>) -> Vec<&'a T> {
    match caveat {
        None => rows.iter().collect(),
        Some((f, v)) => rows.iter().filter(|r| field(r, f).as_deref() == Some(v.as_str())).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(b: u8) -> Identity {
        Identity::from_seed([b; 32])
    }
    fn cv(until: u64) -> Caveats {
        Caveats { until, ..Default::default() }
    }
    #[test]
    fn root_issuer_visitor_verifies() {
        let (root, iss, vis) = (id(1), id(2), id(3));
        let l0 = root.grant(&iss.public_hex(), vec!["notes:*".into(), "status:read".into()], Caveats { mint: Some(500), ..cv(1000) });
        let l1 = iss.grant(&vis.public_hex(), vec!["notes:read".into()], Caveats { count: Some(40), rows: Some(("sandbox".into(), "v1".into())), ..cv(900) });
        let c = Chain(vec![l0, l1]);
        let e = c.verify(&root.public_hex(), 10).unwrap();
        assert_eq!(e.holder, vis.public_hex());
        assert_eq!(e.caveats.count, Some(40));
        assert!(c.verify(&root.public_hex(), 950).unwrap_err().contains("expired"));
        assert!(c.verify(&iss.public_hex(), 10).is_err());
    }
    #[test]
    fn widening_is_refused() {
        let (root, iss, vis) = (id(1), id(2), id(3));
        let l0 = root.grant(&iss.public_hex(), vec!["notes:read".into()], Caveats { count: Some(5), ..cv(1000) });
        for bad in [
            iss.grant(&vis.public_hex(), vec!["notes:admin".into()], Caveats { count: Some(5), ..cv(900) }),
            iss.grant(&vis.public_hex(), vec!["notes:read".into()], Caveats { count: Some(6), ..cv(900) }),
            iss.grant(&vis.public_hex(), vec!["notes:read".into()], cv(900)),
            iss.grant(&vis.public_hex(), vec!["notes:read".into()], Caveats { count: Some(5), ..cv(2000) }),
        ] {
            assert!(Chain(vec![l0.clone(), bad]).verify(&root.public_hex(), 10).is_err());
        }
        let mut forged = iss.grant(&vis.public_hex(), vec!["notes:read".into()], Caveats { count: Some(5), ..cv(900) });
        forged.rights.push("notes:admin".into());
        assert!(Chain(vec![l0, forged]).verify(&root.public_hex(), 10).unwrap_err().contains("signature"));
    }
    #[test]
    fn layer_view_filters_rows() {
        let rows = vec![("a", "v1"), ("b", "v2"), ("c", "v1")];
        let f = |r: &(&str, &str), k: &str| (k == "sandbox").then(|| r.1.to_string());
        assert_eq!(layer_view(&rows, &Some(("sandbox".into(), "v1".into())), f).len(), 2);
        assert_eq!(layer_view(&rows, &None, f).len(), 3);
    }
}
