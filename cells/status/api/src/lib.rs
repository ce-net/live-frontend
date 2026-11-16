//! status: which nodes are alive, from the beats every node sends. Served by three replicas.
lf_contract::contract! {
    api Status = "status", version 1, minor 0;
    types {
        pub struct NodeRow { pub node: String, pub alive: bool, pub age_ms: u64, pub calls: u64 }
        pub struct From { pub seq: u64 }
        pub struct Board { pub holder: String, pub seq: u64, pub nodes: Vec<NodeRow> }
    }
    ops {
        1 => Read(()) -> Board, needs "status:read";
        2 => Watch(From) -> Board, needs "status:read";
    }
    ui {
        view "status.panel" place "main" {
            needs ["status:read"]; data [Read]; tries []; streams [Watch];
            props "()"; state "()"; slots [trend: "trend.chart" ("TrendProps")]; speaks []; hears [];
        }
    }
}
