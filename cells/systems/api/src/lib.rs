//! systems.catalog: the index of live systems; each card on the page is its own `systems.card` cell instance.
lf_contract::contract! {
    api Systems = "systems.catalog", version 1, minor 1;
    types {
        pub struct Open { pub url: String }
        pub struct System { pub slug: String, pub name: String, pub line: String, pub repo: String, pub page: String, pub status: String }
    }
    ops { 1 => List(()) -> Vec<System>, needs "systems.catalog:read"; }
    ui {
        view "systems.card" place "cards" render "systems.catalog:render:card" {
            needs ["systems.catalog:read"]; data [List]; tries []; streams [];
            props "()"; state "()"; slots []; speaks ["Open"]; hears [];
        }
    }
}
