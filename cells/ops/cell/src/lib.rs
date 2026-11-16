//! The ops core. Only a chain with `ops:read` reaches it, and no visitor chain has that.
use fe_runtime::{to, Cell, Cx};
use ops_api::OpsBoard;
use serde_json::Value;

#[derive(Default)]
pub struct Ops;

impl Cell for Ops {
    fn name(&self) -> &'static str {
        "ops"
    }
    fn serve(&mut self, _: u32, _: u32) {}
    fn handle(&mut self, _major: u32, op: &str, _body: Value, cx: &mut Cx) -> Result<Value, String> {
        match op {
            "Read" => Ok(to(&OpsBoard { nodes: cx.nodes.to_vec(), note: "operator view".into() })),
            _ => Err(format!("ops has no op {op}")),
        }
    }
}
