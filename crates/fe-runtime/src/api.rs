//! The contracts of the runtime cells. `timeline.view` and `cap.inspect` are the UI halves of fe.history and
//! fe.explain.
pub mod manifest {
    lf_contract::contract! {
        api FeManifest = "fe.manifest", version 1, minor 0;
        types {
            pub struct SlotDecl { pub name: String, pub view: String, pub props: String }
            pub struct EntryBody {
                pub view: String, pub instance: String, pub api: String, pub version: String, pub schema: String,
                pub bundle: String, pub bundle_ref: String, pub needs: Vec<String>, pub place: String,
                pub data: Vec<String>, pub tries: Vec<String>, pub streams: Vec<String>,
                pub props: String, pub props_fields: Vec<(String, String)>, pub state: String,
                pub slots: Vec<SlotDecl>, pub speaks: Vec<String>, pub hears: Vec<String>,
                pub supersedes: Option<String>, pub until: Option<u64>, pub label: String,
            }
            pub struct Signed { pub body: EntryBody, pub node: String, pub sig: String }
            pub struct Holder { pub node: String, pub sig: String, pub lease_until: u64 }
            pub struct Entry { pub body: EntryBody, pub holders: Vec<Holder> }
            pub struct Resolved { pub seq: u64, pub entries: Vec<Entry> }
            pub struct Complaint { pub view: String, pub bundle: String, pub holder: String, pub why: String }
        }
        ops {
            1 => Announce(Signed) -> u64, needs "fe.manifest:announce";
            2 => Resolve(()) -> Resolved, needs "fe.manifest:resolve";
            3 => Report(Complaint) -> (), needs "fe.manifest:report";
            4 => All(()) -> Vec<Entry>, needs "fe.manifest:all";
        }
        ui { }
    }
}

pub mod serve {
    lf_contract::contract! {
        api FeServe = "fe.serve", version 1, minor 0;
        types {
            pub struct ByHash { pub sha256: String }
            pub struct Bundle { pub sha256: String, pub b64: String }
        }
        ops { 1 => Fetch(ByHash) -> Bundle, needs "fe.serve:fetch"; }
        ui { }
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
        api FeHistory = "fe.history", version 1, minor 0;
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
            view "timeline.view" place "bottom" {
                needs ["fe.history:read"]; data [Since]; tries []; streams [];
                props "()"; state "()"; slots []; speaks []; hears [];
            }
        }
    }
}

pub mod explain {
    lf_contract::contract! {
        api FeExplain = "fe.explain", version 1, minor 0;
        types {
            pub struct LinkView { pub issuer: String, pub holder: String, pub rights: Vec<String>, pub until: u64,
                pub count: Option<u32>, pub rate: Option<u32>, pub rows: Option<(String, String)>, pub mint: Option<u32> }
            pub struct ViewVerdict { pub view: String, pub api: String, pub held: Vec<String>, pub missing: Vec<String>, pub holders: Vec<String> }
            pub struct MyChain { pub id: String, pub links: Vec<LinkView>, pub uses_left: u32, pub uses: u32, pub until: u64,
                pub revoked: bool, pub views: Vec<ViewVerdict>, pub gates: Vec<(String, String, String, String)> }
        }
        ops { 1 => Mine(()) -> MyChain, needs "fe.explain:mine"; }
        ui {
            view "cap.inspect" place "side" {
                needs ["fe.explain:mine"]; data [Mine]; tries []; streams [];
                props "()"; state "()"; slots []; speaks []; hears [];
            }
        }
    }
}

pub mod door {
    lf_contract::contract! {
        api FeDoor = "fe.door", version 1, minor 0;
        types {
            pub struct NodeInfo { pub name: String, pub class: String, pub key: String, pub fp: String }
            pub struct Welcome { pub root: String, pub nodes: Vec<NodeInfo>, pub door: String, pub mode: String }
            pub struct Routed { pub to: String, pub vc: fe_gate::VisitorCall }
        }
        ops {
            1 => Hello(()) -> Welcome, needs "fe.door:hello";
            2 => Call(Routed) -> serde_json::Value, needs "fe.door:call";
        }
        ui { }
    }
}
