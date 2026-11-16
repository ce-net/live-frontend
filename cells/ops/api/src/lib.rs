//! ops: an operator panel no visitor chain holds. It is never in a visitor's manifest and its bundle is never
//! sent to one.
lf_contract::contract! {
    api Ops = "ops", version 1, minor 0;
    types { pub struct OpsBoard { pub nodes: Vec<String>, pub note: String } }
    ops { 1 => Read(()) -> OpsBoard, needs "ops:read"; }
    ui {
        view "ops.panel" place "main" {
            needs ["ops:read"]; data [Read]; tries []; streams [];
            props "()"; state "()"; slots []; speaks []; hears [];
        }
    }
}
