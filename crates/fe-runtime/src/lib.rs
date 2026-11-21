//! The runtime cells and the `Cell` trait. A backend core is `handle(op, body, cx) -> reply` with no I/O of its
//! own: anything it wants sent elsewhere goes into `cx.out`, and the time is `cx.now`. The same cores run as
//! native processes and as wasm in the browser, and a recorded input log replays to the same bytes.
pub mod api;
pub mod history;
pub mod compose;
pub mod visitor;

use fe_chain::Effective;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Caller {
    /// A node of this mesh (its frames are signed by its node key; the sim-mesh door checked that).
    Node(String),
    /// A visitor, after the gate checked the chain.
    Visitor(Effective),
}

impl Caller {
    pub fn rows(&self) -> Option<(String, String)> {
        match self {
            Caller::Visitor(e) => e.caveats.rows.clone(),
            Caller::Node(_) => None,
        }
    }
    pub fn chain(&self) -> String {
        match self {
            Caller::Visitor(e) => e.id.clone(),
            Caller::Node(n) => format!("node:{n}"),
        }
    }
    pub fn rights(&self) -> Vec<String> {
        match self {
            Caller::Visitor(e) => e.rights.clone(),
            Caller::Node(_) => vec![],
        }
    }
}

/// A message a core wants sent. `tag`: the answer comes back to the core's `reply`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Out {
    pub to: String,
    pub api: String,
    pub op: String,
    pub body: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

pub struct Cx<'a> {
    pub now: u64,
    pub node: &'a str,
    pub caller: &'a Caller,
    /// The other nodes that hold this cell (from its install), and every node of the mesh.
    pub peers: &'a [String],
    pub nodes: &'a [String],
    /// The node that hosts the runtime cells (page.compose, fe.history, fe.visitor).
    pub door: &'a str,
    pub out: &'a mut Vec<Out>,
}

impl Cx<'_> {
    pub fn send(&mut self, to: &str, api: &str, op: &str, body: Value) {
        self.out.push(Out { to: to.into(), api: api.into(), op: op.into(), body, tag: None });
    }
    /// An event for `fe.history` (on the door node).
    pub fn event(&mut self, kind: &str, cell: &str, detail: Value) {
        let b = serde_json::json!({"kind": kind, "node": self.node, "cell": cell, "detail": detail});
        let d = self.door.to_string();
        self.send(&d, "fe.history@1", "Append", b);
    }
}

pub trait Cell: Send {
    /// The api name without a major, e.g. `notes`.
    fn name(&self) -> &'static str;
    /// Installs or upgrades the version this node serves (`1.0`, `1.1`, `2.0`).
    fn serve(&mut self, major: u32, minor: u32);
    fn handle(&mut self, major: u32, op: &str, body: Value, cx: &mut Cx) -> Result<Value, String>;
    /// The inner HTML of `Render.Html` for one view (escaped text only; no script). `None`: the generic panel.
    fn html(&self, _view: &str, _instance: &str, _props: &Value) -> Option<String> {
        None
    }
    /// Data an install step hands one instance (a `systems.card` gets the entry its system announced).
    fn install_data(&mut self, _instance: &str, _data: Value) {}
    fn tick(&mut self, _cx: &mut Cx) {}
    fn reply(&mut self, _tag: &str, _ok: bool, _body: Value, _cx: &mut Cx) {}
    /// A node's beat (every node sends one each second to every node).
    fn beat(&mut self, _from: &str, _calls: u64, _now: u64) {}
}

pub fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("body does not decode: {e}"))
}

pub fn to<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap()
}
