//! The faults core: counts the crashes a session asked for. The crash itself happens in the UI cell.
use faults_api::{ArmReq, Armed};
use fe_runtime::{parse, to, Cell, Cx};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Faults {
    pub armed: BTreeMap<String, u32>,
}

impl Cell for Faults {
    fn name(&self) -> &'static str {
        "faults"
    }
    fn serve(&mut self, _: u32, _: u32) {}
    fn handle(&mut self, _major: u32, op: &str, body: Value, cx: &mut Cx) -> Result<Value, String> {
        match op {
            "Arm" => {
                let a: ArmReq = parse(body)?;
                if a.mode != "throw" && a.mode != "hang" {
                    return Err("mode is throw or hang".into());
                }
                let s = cx.caller.chain();
                let n = self.armed.entry(s.clone()).or_default();
                *n += 1;
                cx.event("Crash requested", "faults.panel", serde_json::json!({"mode": a.mode, "n": *n}));
                Ok(to(&Armed { mode: a.mode, n: *n, session: s }))
            }
            _ => Err(format!("faults has no op {op}")),
        }
    }
}
