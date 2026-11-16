//! The systems.catalog core: the live systems of ce-net, one card each.
use fe_runtime::{Cell, Cx};
use serde_json::Value;

pub const SYSTEMS: &str = include_str!("../systems.json");

#[derive(Default)]
pub struct Systems;

impl Cell for Systems {
    fn name(&self) -> &'static str {
        "systems.catalog"
    }
    fn serve(&mut self, _: u32, _: u32) {}
    fn handle(&mut self, _major: u32, op: &str, _body: Value, _cx: &mut Cx) -> Result<Value, String> {
        match op {
            "List" => serde_json::from_str(SYSTEMS).map_err(|e| e.to_string()),
            _ => Err(format!("systems.catalog has no op {op}")),
        }
    }
}
