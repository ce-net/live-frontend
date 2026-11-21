//! The systems.catalog core: the live systems of ce-net, one card each.
//!
//! `systems.json` is the list built in (generated from the `ce-net/demos` index). A system that publishes its own
//! `ce-ui.json` is announced at install time instead: the installer checks that entry's sha256 and hands it to
//! this cell with the card instance, and it replaces the built-in line for that slug or adds a new card.
use fe_runtime::{Cell, Cx};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SYSTEMS: &str = include_str!("../systems.json");

#[derive(Default)]
pub struct Systems {
    announced: BTreeMap<String, Value>,
}

const FIELDS: [&str; 6] = ["slug", "name", "line", "repo", "page", "status"];

impl Cell for Systems {
    fn name(&self) -> &'static str {
        "systems.catalog"
    }
    fn serve(&mut self, _: u32, _: u32) {}
    fn install_data(&mut self, instance: &str, data: Value) {
        if FIELDS.iter().all(|k| data[*k].is_string()) && data["slug"] == instance {
            let card: serde_json::Map<String, Value> = FIELDS.iter().map(|k| (k.to_string(), data[*k].clone())).collect();
            self.announced.insert(instance.to_string(), Value::Object(card));
        }
    }
    fn handle(&mut self, _major: u32, op: &str, _body: Value, _cx: &mut Cx) -> Result<Value, String> {
        match op {
            "List" => {
                let mut all: Vec<Value> = serde_json::from_str(SYSTEMS).map_err(|e| e.to_string())?;
                for x in all.iter_mut() {
                    if let Some(a) = x["slug"].as_str().and_then(|s| self.announced.get(s)) {
                        *x = a.clone();
                    }
                }
                for (slug, a) in &self.announced {
                    if !all.iter().any(|x| x["slug"] == slug.as_str()) {
                        all.push(a.clone());
                    }
                }
                Ok(Value::Array(all))
            }
            _ => Err(format!("systems.catalog has no op {op}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn list(s: &mut Systems) -> Vec<Value> {
        let (caller, mut out) = (fe_runtime::Caller::Node("a".into()), vec![]);
        let mut cx = Cx { now: 1, node: "a", caller: &caller, peers: &[], nodes: &[], door: "d", out: &mut out };
        serde_json::from_value(s.handle(1, "List", Value::Null, &mut cx).unwrap()).unwrap()
    }

    #[test]
    fn an_announced_system_replaces_or_adds_a_card() {
        let mut s = Systems::default();
        let n = list(&mut s).len();
        let card = |slug: &str, line: &str| json!({"slug": slug, "name": "n", "line": line, "repo": "r", "page": "p", "status": "works"});
        s.install_data("live-frontend", card("live-frontend", "announced by itself"));
        s.install_data("new-system", card("new-system", "not in the built-in list"));
        s.install_data("other", card("mismatch", "slug differs from the instance: ignored"));
        let l = list(&mut s);
        assert_eq!(l.len(), n + 1);
        assert_eq!(l.iter().find(|x| x["slug"] == "live-frontend").unwrap()["line"], "announced by itself");
        assert!(l.iter().any(|x| x["slug"] == "new-system"));
        assert!(!l.iter().any(|x| x["slug"] == "mismatch"));
    }
}
