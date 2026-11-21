//! The contracts of the runtime cells. `timeline.view` and `cap.inspect` are the UI halves of fe.history and
//! fe.explain.
/// The three ops every view expands to (`lf_contract::RENDER_OPS`), under the view's `<api>:render:<view>` right.
pub mod render {
    use serde::{Deserialize, Serialize};
    pub use super::compose::{EntryBody, Signed, SlotDecl};
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ViewReq { pub view: String, #[serde(default)] pub instance: Option<String> }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ByHash { pub sha256: String }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Bundle { pub sha256: String, pub b64: String }
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct HtmlReq { pub view: String, #[serde(default)] pub instance: Option<String>, #[serde(default)] pub props: serde_json::Value }
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct Html { pub view: String, pub html: String, pub sha256: String }
}

/// page.compose: the aggregator that replaces fe.manifest. Serving cells announce the signed answer of their own
/// `Render.Entry` (lease 5 s); `Resolve` answers, for the caller's chain, only the entries whose render right the
/// chain holds. It holds no right over any viewer's data and adds none.
pub mod compose {
    lf_contract::contract! {
        api PageCompose = "page.compose", version 1, minor 0;
        types {
            pub struct SlotDecl { pub name: String, pub render: String, pub props: String }
            pub struct EntryBody {
                pub view: String, pub instance: String, pub api: String, pub version: String, pub schema: String,
                pub render: String, pub bundle: String, pub bundle_ref: String, pub parts: Vec<(String, String)>,
                pub needs: Vec<String>, pub place: String,
                pub data: Vec<String>, pub tries: Vec<String>, pub streams: Vec<String>,
                pub props: String, pub props_fields: Vec<(String, String)>, pub state: String,
                pub slots: Vec<SlotDecl>, pub speaks: Vec<String>, pub hears: Vec<String>,
                pub supersedes: Option<String>, pub until: Option<u64>, pub label: String,
            }
            pub struct Signed { pub body: EntryBody, pub node: String, pub sig: String }
            pub struct Holder { pub node: String, pub sig: String, pub lease_until: u64 }
            pub struct Entry { pub body: EntryBody, pub holders: Vec<Holder> }
            pub struct Tree { pub seq: u64, pub entries: Vec<Entry> }
            pub struct Complaint { pub view: String, pub bundle: String, pub holder: String, pub why: String }
        }
        ops {
            1 => Announce(Signed) -> u64, needs "page.compose:announce";
            2 => Resolve(()) -> Tree, needs "page.compose:resolve";
            3 => Report(Complaint) -> (), needs "page.compose:report";
            4 => All(()) -> Vec<Entry>, needs "page.compose:all";
        }
        ui { }
    }
}

/// fe.shell: the empty page (loader, bus, sandbox) as a view like any other, served by hash through
/// `Render.Bundle` of `fe.shell:render:page`. Its parts are the page's other files (styles, worker, wasm node,
/// the recorded run), each by hash.
pub mod shell {
    lf_contract::contract! {
        api FeShell = "fe.shell", version 1, minor 0;
        types { }
        ops { }
        ui {
            view "shell.page" place "page" render "fe.shell:render:page" {
                needs []; data []; tries []; streams [];
                props "()"; state "()"; slots []; speaks []; hears [];
            }
        }
    }
}

pub mod visitor {
    lf_contract::contract! {
        api FeVisitor = "fe.visitor", version 1, minor 0;
        types {
            pub struct JoinReq { pub holder: String, pub pow: String }
            pub struct Joined { pub chain: fe_chain::Chain, pub id: String, pub until: u64, pub uses: u32 }
            pub struct SpendReq { pub chain: String, pub call_sha: String }
            pub struct ChainState { pub id: String, pub uses_left: u32, pub uses: u32, pub until: u64, pub revoked: bool }
        }
        ops {
            1 => Join(JoinReq) -> Joined, needs "fe.visitor:join";
            2 => Revoke(()) -> ChainState, needs "fe.visitor:revoke-own";
            3 => State(()) -> ChainState, needs "fe.visitor:state";
            4 => Spend(SpendReq) -> fe_gate::Ticket, needs "fe.visitor:spend";
        }
        ui { }
    }
}

pub mod history {
    lf_contract::contract! {
        api FeHistory = "fe.history", version 1, minor 1;
        types {
            pub struct NewEvent { pub kind: String, pub node: String, pub cell: String, pub detail: serde_json::Value }
            pub struct Observation { pub kind: String, pub cell: String, pub detail: serde_json::Value }
            pub struct Event { pub seq: u64, pub t: u64, pub kind: String, pub node: String, pub cell: String,
                pub detail: serde_json::Value, pub by: String, pub prev: String, pub hash: String }
            pub struct From { pub seq: u64 }
            pub struct Events { pub head: u64, pub events: Vec<Event> }
        }
        ops {
            1 => Append(NewEvent) -> u64, needs "fe.history:append";
            2 => Observe(Observation) -> u64, needs "fe.history:observe";
            3 => Since(From) -> Events, needs "fe.history:read";
        }
        ui {
            view "timeline.view" place "bottom" render "fe.history:render:timeline" {
                needs ["fe.history:read"]; data [Since]; tries []; streams [];
                props "()"; state "()"; slots []; speaks []; hears [];
            }
        }
    }
}

pub mod explain {
    lf_contract::contract! {
        api FeExplain = "fe.explain", version 1, minor 1;
        types {
            pub struct LinkView { pub issuer: String, pub holder: String, pub rights: Vec<String>, pub until: u64,
                pub count: Option<u32>, pub rate: Option<u32>, pub rows: Option<(String, String)>, pub mint: Option<u32> }
            pub struct ViewVerdict { pub view: String, pub api: String, pub held: Vec<String>, pub missing: Vec<String>, pub holders: Vec<String> }
            pub struct MyChain { pub id: String, pub links: Vec<LinkView>, pub uses_left: u32, pub uses: u32, pub until: u64,
                pub revoked: bool, pub views: Vec<ViewVerdict>, pub gates: Vec<(String, String, String, String)> }
        }
        ops { 1 => Mine(()) -> MyChain, needs "fe.explain:mine"; }
        ui {
            view "cap.inspect" place "side" render "fe.explain:render:inspect" {
                needs ["fe.explain:mine"]; data [Mine]; tries []; streams [];
                props "()"; state "()"; slots []; speaks []; hears [];
            }
        }
    }
}

/// http.serve: the one cell that speaks HTTP. It holds `http-serve` for its route (granted by the node) and no
/// right of its own over any cell: `/` is page.compose and Render.Html for the anonymous chain, `/_c/<sha256>` is
/// Render.Bundle of the owning cell, `/wire` forwards signed frames that every gate checks again.
pub mod http {
    lf_contract::contract! {
        api HttpServe = "http.serve", version 1, minor 0;
        types {
            pub struct NodeInfo { pub name: String, pub class: String, pub key: String, pub fp: String }
            pub struct Welcome { pub root: String, pub nodes: Vec<NodeInfo>, pub door: String, pub mode: String }
            pub struct Routed { pub to: String, pub vc: fe_gate::VisitorCall }
        }
        ops {
            1 => Hello(()) -> Welcome, needs "http.serve:hello";
            2 => Call(Routed) -> serde_json::Value, needs "http.serve:call";
        }
        ui { }
    }
}
