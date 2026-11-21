//! faults: a UI cell that crashes on request (throw, or stop answering), to show that one cell's crash
//! leaves every other cell running. Acts on the caller's own session only.
lf_contract::contract! {
    api Faults = "faults", version 1, minor 1;
    types {
        pub struct ArmReq { pub mode: String }
        pub struct Armed { pub mode: String, pub n: u32, pub session: String }
    }
    ops { 1 => Arm(ArmReq) -> Armed, needs "faults:run"; }
    ui {
        view "faults.panel" place "main" render "faults:render:panel" {
            needs ["faults:run"]; data [Arm]; tries []; streams [];
            props "()"; state "()"; slots []; speaks []; hears [];
        }
    }
}
