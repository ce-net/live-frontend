//! Every contract this node knows, by api, major and minor. A node can host any of these cells.
use fe_runtime::Cell;
use lf_contract::descriptor as d;
use serde_json::Value;
use std::sync::OnceLock;

pub fn all() -> &'static Vec<Value> {
    static R: OnceLock<Vec<Value>> = OnceLock::new();
    R.get_or_init(|| {
        use fe_runtime::api::*;
        vec![
            d::<notes_api::v1_0::Notes>(),
            d::<notes_api::v1_1::Notes>(),
            d::<notes_api::v2_0::Notes>(),
            d::<status_api::Status>(),
            d::<trend_api::Trend>(),
            d::<faults_api::Faults>(),
            d::<ops_api::Ops>(),
            d::<systems_api::Systems>(),
            d::<manifest::FeManifest>(),
            d::<serve::FeServe>(),
            d::<visitor::FeVisitor>(),
            d::<history::FeHistory>(),
            d::<explain::FeExplain>(),
            d::<door::FeDoor>(),
        ]
    })
}

pub fn descriptor(api: &str, major: u32, minor: u32) -> Option<&'static Value> {
    all().iter().find(|d| d["api"] == api && d["major"] == major && d["minor"] == minor)
}

/// The newest minor of a major.
pub fn latest(api: &str, major: u32) -> Option<&'static Value> {
    all().iter().filter(|d| d["api"] == api && d["major"] == major).max_by_key(|d| d["minor"].as_u64())
}

fn op<'a>(api: &str, major: u32, op: &str) -> Option<&'a Value> {
    latest(api, major)?["ops"].as_array()?.iter().find(|o| o["name"] == op)
}

pub fn right(api: &str, major: u32, name: &str) -> Option<String> {
    op(api, major, name).and_then(|o| o["right"].as_str().map(String::from))
}

pub fn counted(api: &str, major: u32, name: &str) -> bool {
    op(api, major, name).is_some_and(|o| o["kw"].as_array().is_some_and(|k| k.iter().any(|x| x == "counted")))
}

pub fn make_cell(api: &str) -> Option<Box<dyn Cell>> {
    Some(match api {
        "notes" => Box::new(notes_cell::Notes::default()),
        "status" => Box::new(status_cell::Status::default()),
        "trend" => Box::new(trend_cell::Trend::default()),
        "faults" => Box::new(faults_cell::Faults::default()),
        "ops" => Box::new(ops_cell::Ops),
        "systems.catalog" => Box::new(systems_cell::Systems),
        _ => return None,
    })
}
