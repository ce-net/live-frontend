//! The status core: the beats of every node, as one board. Any replica answers; `seq` is the second, so a
//! stream that moves to another replica continues from the next second.
use fe_runtime::{parse, to, Cell, Cx};
use serde_json::Value;
use status_api::{Board, From, NodeRow};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Status {
    pub beats: BTreeMap<String, (u64, u64)>,
}

impl Status {
    fn board(&self, cx: &Cx) -> Board {
        let mut nodes: Vec<NodeRow> = cx
            .nodes
            .iter()
            .chain(std::iter::once(&cx.node.to_string()))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|n| {
                let (t, calls) = self.beats.get(n).copied().unwrap_or((0, 0));
                let age = if n == cx.node { 0 } else if t == 0 { u64::MAX } else { cx.now.saturating_sub(t) };
                NodeRow { node: n.clone(), alive: age < 2_500, age_ms: age.min(9_999_999), calls }
            })
            .collect();
        nodes.sort_by(|a, b| a.node.cmp(&b.node));
        Board { holder: cx.node.into(), seq: cx.now / 1000, nodes }
    }
}

impl Cell for Status {
    fn name(&self) -> &'static str {
        "status"
    }
    fn serve(&mut self, _: u32, _: u32) {}
    fn beat(&mut self, from: &str, calls: u64, now: u64) {
        self.beats.insert(from.into(), (now, calls));
    }
    fn handle(&mut self, _major: u32, op: &str, body: Value, cx: &mut Cx) -> Result<Value, String> {
        match op {
            "Read" => Ok(to(&self.board(cx))),
            "Watch" => {
                let _f: From = parse(body)?;
                Ok(to(&self.board(cx)))
            }
            _ => Err(format!("status has no op {op}")),
        }
    }
}
