//! notes: a board of short notes, each visitor writing only in its own sandbox. Three releases of one contract:
//! 1.0, then 1.1 (adds an optional colour, minor: 1.1 answers 1.0 callers), then 2.0 (colour changes meaning
//! from a name to an RGB number, major: served beside 1 for a while, migrated both ways).
pub mod v1_0 {
    lf_contract::contract! {
        api Notes = "notes", version 1, minor 0;
        types {
            pub struct Note { pub id: String, pub text: String, pub sandbox: String, pub seq: u64 }
            pub struct Draft { pub id: String, pub text: String }
            pub struct PutReq { pub op_id: String, pub note: Draft }
            pub struct From { pub seq: u64 }
            pub struct Delta { pub seq: u64, pub rows: Vec<Note> }
            pub struct Rep { pub op_id: String, pub row: serde_json::Value }
            pub struct NoteAdded { pub id: String }
        }
        ops {
            1 => List(()) -> Vec<Note>, needs "notes:read";
            2 => Put(PutReq) -> Note, needs "notes:write", counted;
            3 => Since(From) -> Delta, needs "notes:read";
            9 => Purge(()) -> u32, needs "notes:admin";
            10 => Replicate(Rep) -> bool, needs "notes:replicate";
            11 => Dump(()) -> Vec<Rep>, needs "notes:replicate";
        }
        ui {
            view "notes.board" place "main" {
                needs ["notes:read", "notes:write"]; data [List, Put]; tries [Purge]; streams [Since];
                props "()"; state "Draft"; slots [trend: "trend.chart" ("TrendProps")]; speaks ["NoteAdded"]; hears [];
            }
        }
    }
}

pub mod v1_1 {
    lf_contract::contract! {
        api Notes = "notes", version 1, minor 1;
        types {
            pub struct Note { pub id: String, pub text: String, pub sandbox: String, pub seq: u64, pub colour: Option<String> }
            pub struct Draft { pub id: String, pub text: String, pub colour: Option<String> }
            pub struct PutReq { pub op_id: String, pub note: Draft }
            pub struct From { pub seq: u64 }
            pub struct Delta { pub seq: u64, pub rows: Vec<Note> }
            pub struct Rep { pub op_id: String, pub row: serde_json::Value }
            pub struct NoteAdded { pub id: String }
        }
        ops {
            1 => List(()) -> Vec<Note>, needs "notes:read";
            2 => Put(PutReq) -> Note, needs "notes:write", counted;
            3 => Since(From) -> Delta, needs "notes:read";
            9 => Purge(()) -> u32, needs "notes:admin";
            10 => Replicate(Rep) -> bool, needs "notes:replicate";
            11 => Dump(()) -> Vec<Rep>, needs "notes:replicate";
        }
        ui {
            view "notes.board" place "main" {
                needs ["notes:read", "notes:write"]; data [List, Put]; tries [Purge]; streams [Since];
                props "()"; state "Draft"; slots [trend: "trend.chart" ("TrendProps")]; speaks ["NoteAdded"]; hears [];
            }
        }
    }
}

pub mod v2_0 {
    lf_contract::contract! {
        api Notes = "notes", version 2, minor 0;
        types {
            pub struct Note { pub id: String, pub text: String, pub sandbox: String, pub seq: u64, pub rgb: Option<u32> }
            pub struct Draft { pub id: String, pub text: String, pub rgb: Option<u32> }
            pub struct PutReq { pub op_id: String, pub note: Draft }
            pub struct From { pub seq: u64 }
            pub struct Delta { pub seq: u64, pub rows: Vec<Note> }
            pub struct Rep { pub op_id: String, pub row: serde_json::Value }
            pub struct NoteAdded { pub id: String }
        }
        ops {
            1 => List(()) -> Vec<Note>, needs "notes:read";
            2 => Put(PutReq) -> Note, needs "notes:write", counted;
            3 => Since(From) -> Delta, needs "notes:read";
            9 => Purge(()) -> u32, needs "notes:admin";
            10 => Replicate(Rep) -> bool, needs "notes:replicate";
            11 => Dump(()) -> Vec<Rep>, needs "notes:replicate";
        }
        ui {
            view "notes.board" place "main" {
                needs ["notes:read", "notes:write"]; data [List, Put]; tries [Purge]; streams [Since];
                props "()"; state "Draft"; slots [trend: "trend.chart" ("TrendProps")]; speaks ["NoteAdded"]; hears [];
            }
        }
    }
}
