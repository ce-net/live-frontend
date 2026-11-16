//! fe.history: an append-only, hash-chained log of what happened to the system.
use crate::api::history::*;
use crate::{parse, to, Caller, Cell, Cx};
use serde_json::{json, Value};

#[derive(Default)]
pub struct History {
    pub events: Vec<Event>,
}

impl History {
    pub fn push(&mut self, t: u64, kind: &str, node: &str, cell: &str, detail: Value, by: &str) -> u64 {
        let prev = self.events.last().map(|e| e.hash.clone()).unwrap_or_else(|| "0".repeat(16));
        let seq = self.events.len() as u64 + 1;
        let body = json!([seq, t, kind, node, cell, detail, by, prev]).to_string();
        let hash = fe_chain::sha256_hex(body.as_bytes())[..16].to_string();
        self.events.push(Event { seq, t, kind: kind.into(), node: node.into(), cell: cell.into(), detail, by: by.into(), prev, hash });
        seq
    }
}

impl Cell for History {
    fn name(&self) -> &'static str {
        "fe.history"
    }
    fn serve(&mut self, _: u32, _: u32) {}
    fn handle(&mut self, _major: u32, op: &str, body: Value, cx: &mut Cx) -> Result<Value, String> {
        match op {
            "Append" => {
                let a: NewEvent = parse(body)?;
                let Caller::Node(from) = cx.caller else { return Err("Append is for nodes".into()) };
                let by = from.clone();
                Ok(json!(self.push(cx.now, &a.kind, &a.node, &a.cell, a.detail, &by)))
            }
            "Observe" => {
                let o: Observation = parse(body)?;
                let by = format!("viewer:{}", &cx.caller.chain()[..8.min(cx.caller.chain().len())]);
                Ok(json!(self.push(cx.now, &o.kind, "browser", &o.cell, o.detail, &by)))
            }
            "Since" => {
                let s: From = parse(body)?;
                let from = s.seq as usize;
                // a viewer never hears about a cell whose needs its chain does not hold (ops.panel stays unknown)
                let rights = cx.caller.rights();
                let visible = |e: &Event| match (&cx.caller, e.detail.get("needs").and_then(|n| n.as_array())) {
                    (Caller::Visitor(_), Some(needs)) => needs.iter().all(|n| n.as_str().is_some_and(|n| fe_chain::holds(&rights, n))),
                    _ => true,
                };
                let events: Vec<Event> = self.events.iter().skip(from).take(400).filter(|e| visible(e)).cloned().collect();
                Ok(to(&Events { head: self.events.len() as u64, events }))
            }
            _ => Err(format!("fe.history has no op {op}")),
        }
    }
}
