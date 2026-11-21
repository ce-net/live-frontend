//! The notes core. One store serves both majors: rows keep the colour by name; 2.0 callers see and write it as
//! an RGB number (migrated both ways through one palette). A write carries an op id, so a retried Put is
//! applied once on every replica. Rows follow the chain's row caveat (`sandbox == <visitor>`): a visitor reads
//! and writes only its own.
use fe_runtime::{parse, to, Caller, Cell, Cx};
use notes_api::{v1_1, v2_1};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const PALETTE: &[(&str, u32)] = &[("amber", 0xf5a524), ("green", 0x3f9d4a), ("blue", 0x3b6fd8), ("red", 0xd64545), ("grey", 0x8a8f87)];
pub const MAX_ROWS: usize = 20;
pub const MAX_TEXT: usize = 200;

pub fn name_to_rgb(n: &str) -> Option<u32> {
    PALETTE.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)
}
pub fn rgb_to_name(v: u32) -> String {
    let d = |a: u32, b: u32| -> i64 {
        let c = |s: u32| ((a >> s & 255) as i64 - (b >> s & 255) as i64).pow(2);
        c(16) + c(8) + c(0)
    };
    PALETTE.iter().min_by_key(|(_, p)| d(*p, v)).unwrap().0.to_string()
}

#[derive(Default)]
pub struct Notes {
    pub synced: bool,
    pub rows: BTreeMap<String, v1_1::Note>,
    pub row_op: BTreeMap<String, String>,
    pub ops: BTreeSet<String>,
    pub minor: BTreeMap<u32, u32>,
    pub seq: u64,
}

fn key(n: &v1_1::Note) -> String {
    format!("{}/{}", n.sandbox, n.id)
}

impl Notes {
    fn visible(&self, caller: &Caller) -> Vec<&v1_1::Note> {
        let rows: Vec<v1_1::Note> = vec![];
        let _ = rows;
        let all: Vec<&v1_1::Note> = self.rows.values().collect();
        match caller.rows() {
            Some((f, v)) if f == "sandbox" => all.into_iter().filter(|r| r.sandbox == v).collect(),
            Some(_) => vec![],
            None => all,
        }
    }
    fn out(&self, major: u32, n: &v1_1::Note) -> Value {
        match (major, self.minor.get(&1).copied().unwrap_or(0)) {
            (2, _) => to(&v2_1::Note { id: n.id.clone(), text: n.text.clone(), sandbox: n.sandbox.clone(), seq: n.seq, rgb: n.colour.as_deref().and_then(name_to_rgb) }),
            (_, 0) => json!({"id": n.id, "text": n.text, "sandbox": n.sandbox, "seq": n.seq}),
            _ => to(n),
        }
    }
    fn apply(&mut self, op_id: &str, row: v1_1::Note) -> bool {
        if !self.ops.insert(op_id.to_string()) {
            return false;
        }
        self.seq = self.seq.max(row.seq);
        self.row_op.insert(key(&row), op_id.to_string());
        self.rows.insert(key(&row), row);
        true
    }
}

impl Cell for Notes {
    /// A replica that starts (or restarts) asks its peers for their rows once.
    fn tick(&mut self, cx: &mut Cx) {
        if !self.synced {
            self.synced = true;
            for p in cx.peers.to_vec() {
                cx.out.push(fe_runtime::Out { to: p, api: "notes@1".into(), op: "Dump".into(), body: Value::Null, tag: Some("notes#sync".into()) });
            }
        }
    }
    fn reply(&mut self, _tag: &str, ok: bool, body: Value, _cx: &mut Cx) {
        if !ok {
            return;
        }
        if let Ok(reps) = serde_json::from_value::<Vec<v1_1::Rep>>(body) {
            for r in reps {
                if let Ok(row) = serde_json::from_value::<v1_1::Note>(r.row) {
                    self.apply(&r.op_id, row);
                }
            }
        }
    }
    fn name(&self) -> &'static str {
        "notes"
    }
    fn serve(&mut self, major: u32, minor: u32) {
        self.minor.insert(major, minor);
    }
    fn handle(&mut self, major: u32, op: &str, body: Value, cx: &mut Cx) -> Result<Value, String> {
        match op {
            "List" => Ok(Value::Array(self.visible(cx.caller).into_iter().map(|n| self.out(major, n)).collect())),
            "Since" => {
                let f: v1_1::From = parse(body)?;
                let rows: Vec<Value> = self.visible(cx.caller).into_iter().filter(|n| n.seq > f.seq).map(|n| self.out(major, n)).collect();
                Ok(json!({"seq": self.seq, "rows": rows}))
            }
            "Put" => {
                let (op_id, id, text, colour) = if major == 2 {
                    let p: v2_1::PutReq = parse(body)?;
                    (p.op_id, p.note.id, p.note.text, p.note.rgb.map(rgb_to_name))
                } else {
                    let p: v1_1::PutReq = parse(body)?;
                    // 1.0 has no colour: a 1.0 backend drops it; 1.1 keeps it.
                    let c = if self.minor.get(&1).copied().unwrap_or(0) >= 1 { p.note.colour } else { None };
                    (p.op_id, p.note.id, p.note.text, c)
                };
                let sandbox = match cx.caller.rows() {
                    Some((f, v)) if f == "sandbox" => v,
                    Some(_) => return Err("this chain may not write notes".into()),
                    None => "node".into(),
                };
                if text.len() > MAX_TEXT || id.is_empty() || id.len() > 40 {
                    return Err(format!("a note is at most {MAX_TEXT} bytes, with an id of 1 to 40"));
                }
                let k = format!("{sandbox}/{id}");
                if let Some(done) = self.rows.values().find(|r| self.ops.contains(&op_id) && key(r) == k) {
                    return Ok(self.out(major, done));
                }
                if !self.rows.contains_key(&k) && self.rows.values().filter(|r| r.sandbox == sandbox).count() >= MAX_ROWS {
                    return Err(format!("at most {MAX_ROWS} notes per sandbox"));
                }
                let row = v1_1::Note { id, text, sandbox, seq: self.seq + 1, colour };
                if self.apply(&op_id, row.clone()) {
                    for p in cx.peers.to_vec() {
                        cx.send(&p, "notes@1", "Replicate", to(&v1_1::Rep { op_id: op_id.clone(), row: to(&row) }));
                    }
                    let mut all = cx.nodes.to_vec();
                    all.push(cx.node.to_string());
                    for n in all {
                        cx.send(&n, "trend@1", "Push", json!({"series": "notes", "key": "written", "v": 1.0}));
                    }
                }
                Ok(self.out(major, self.rows.get(&k).unwrap()))
            }
            "Replicate" => {
                let Caller::Node(_) = cx.caller else { return Err("Replicate is for replicas".into()) };
                let r: v1_1::Rep = parse(body)?;
                let row: v1_1::Note = parse(r.row)?;
                Ok(json!(self.apply(&r.op_id, row)))
            }
            "Dump" => {
                let Caller::Node(_) = cx.caller else { return Err("Dump is for replicas".into()) };
                let reps: Vec<v1_1::Rep> = self.rows.iter().map(|(k, r)| v1_1::Rep { op_id: self.row_op.get(k).cloned().unwrap_or_default(), row: to(r) }).collect();
                Ok(to(&reps))
            }
            "Purge" => {
                let n = self.rows.len() as u32;
                self.rows.clear();
                Ok(json!(n))
            }
            _ => Err(format!("notes has no op {op}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fe_chain::{Caveats, Effective};
    fn visitor(sb: &str) -> Caller {
        Caller::Visitor(Effective { id: format!("c-{sb}"), issuer: "i".into(), holder: "h".into(), rights: vec![],
            caveats: Caveats { rows: Some(("sandbox".into(), sb.into())), ..Default::default() } })
    }
    fn cx<'a>(c: &'a Caller, out: &'a mut Vec<fe_runtime::Out>, peers: &'a [String]) -> Cx<'a> {
        Cx { now: 1, node: "a", caller: c, peers, nodes: &[], door: "d", out }
    }
    #[test]
    fn sandbox_retry_and_majors() {
        let mut n = Notes::default();
        n.serve(1, 1);
        n.serve(2, 0);
        let (v1, v2) = (visitor("v1"), visitor("v2"));
        let mut out = vec![];
        let peers = vec!["b".to_string()];
        let put = json!({"op_id": "op1", "note": {"id": "x", "text": "hello", "colour": "red"}});
        n.handle(1, "Put", put.clone(), &mut cx(&v1, &mut out, &peers)).unwrap();
        n.handle(1, "Put", put, &mut cx(&v1, &mut out, &peers)).unwrap();
        assert_eq!(out.iter().filter(|o| o.op == "Replicate").count(), 1, "a retried op id is applied once");
        assert_eq!(n.handle(1, "List", Value::Null, &mut cx(&v2, &mut out, &peers)).unwrap(), json!([]));
        let l2 = n.handle(2, "List", Value::Null, &mut cx(&v1, &mut out, &peers)).unwrap();
        assert_eq!(l2[0]["rgb"], json!(0xd64545));
        n.handle(2, "Put", json!({"op_id": "op2", "note": {"id": "y", "text": "t", "rgb": 0x3b6fd0}}), &mut cx(&v1, &mut out, &peers)).unwrap();
        let l1 = n.handle(1, "List", Value::Null, &mut cx(&v1, &mut out, &peers)).unwrap();
        assert_eq!(l1[1]["colour"], json!("blue"), "2.0 writes are read by 1.x callers");
    }
}
