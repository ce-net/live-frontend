//! `cargo cell check`: the contract-break check across cells, then the four decl.lint rules on every view.
use lf_contract::{parse_types, TypeDef};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

fn frozen_path(root: &Path, api: &str, major: u64) -> std::path::PathBuf {
    root.join(format!("contracts/frozen/{api}@{major}.json"))
}

/// The latest minor of every (api, major).
pub fn latest(all: &[Value]) -> BTreeMap<(String, u64), Value> {
    let mut m: BTreeMap<(String, u64), Value> = BTreeMap::new();
    for d in all {
        let k = (d["api"].as_str().unwrap().to_string(), d["major"].as_u64().unwrap());
        if m.get(&k).is_none_or(|x| x["minor"].as_u64() < d["minor"].as_u64()) {
            m.insert(k, d.clone());
        }
    }
    m
}

fn fields(d: &Value) -> BTreeMap<String, BTreeMap<String, String>> {
    parse_types(d["types"].as_str().unwrap_or(""))
        .into_iter()
        .map(|t| match t {
            TypeDef::Struct { name, fields } => (name, fields.into_iter().collect()),
            TypeDef::Enum { name, variants } => (name, variants.into_iter().map(|(v, f, i)| (v, format!("{f:?}{i:?}"))).collect()),
        })
        .collect()
}

/// Who depends on `api`: its own views, and every view (in any contract) with a slot filled by one of them.
pub fn dependents(api: &str, cur: &Value, all: &[Value]) -> Vec<String> {
    let own: Vec<String> = cur["views"].as_array().unwrap().iter().map(|v| v["name"].as_str().unwrap().to_string()).collect();
    let mut out: Vec<String> = own.iter().map(|v| format!("{v} (view of {api})")).collect();
    for d in all {
        for v in d["views"].as_array().unwrap() {
            for s in v["slots"].as_array().unwrap() {
                if own.iter().any(|o| s["view"] == o.as_str()) {
                    out.push(format!("{} (slot {} of {}@{})", v["name"].as_str().unwrap(), s["name"].as_str().unwrap(), d["api"].as_str().unwrap(), d["major"]));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Compares one contract with its frozen descriptor. Errors name what broke and every dependent.
pub fn compare(frozen: &Value, cur: &Value, all: &[Value]) -> Vec<String> {
    let api = format!("{}@{}", cur["api"].as_str().unwrap(), cur["major"]);
    let mut breaks = vec![];
    let mut adds = vec![];
    let ops = |d: &Value| -> BTreeMap<String, Value> { d["ops"].as_array().unwrap().iter().map(|o| (o["name"].as_str().unwrap().to_string(), o.clone())).collect() };
    let (fo, co) = (ops(frozen), ops(cur));
    for (n, o) in &fo {
        match co.get(n) {
            None => breaks.push(format!("op {n} removed")),
            Some(c) => {
                for k in ["id", "req", "resp", "right"] {
                    if o[k] != c[k] {
                        breaks.push(format!("op {n}: {k} {} -> {}", o[k], c[k]));
                    }
                }
            }
        }
    }
    adds.extend(co.keys().filter(|n| !fo.contains_key(*n)).map(|n| format!("op {n} added")));
    let (ft, ct) = (fields(frozen), fields(cur));
    for (t, f) in &ft {
        let Some(c) = ct.get(t) else {
            breaks.push(format!("type {t} removed"));
            continue;
        };
        for (k, ty) in f {
            match c.get(k) {
                None => breaks.push(format!("{t}.{k} removed")),
                Some(cty) if cty != ty => breaks.push(format!("{t}.{k}: {ty} -> {cty}")),
                _ => {}
            }
        }
        for k in c.keys().filter(|k| !f.contains_key(*k)) {
            if c[k].starts_with("Option<") {
                adds.push(format!("{t}.{k} added (optional)"));
            } else {
                breaks.push(format!("{t}.{k} added and required"));
            }
        }
    }
    let views = |d: &Value| -> BTreeMap<String, Value> { d["views"].as_array().unwrap().iter().map(|v| (v["name"].as_str().unwrap().to_string(), v.clone())).collect() };
    let (fv, cv) = (views(frozen), views(cur));
    for (n, v) in &fv {
        let Some(c) = cv.get(n) else {
            breaks.push(format!("view {n} removed"));
            continue;
        };
        for k in ["needs", "props", "state", "speaks", "slots"] {
            if v[k] != c[k] {
                breaks.push(format!("view {n}: {k} {} -> {}", v[k], c[k]));
            }
        }
    }
    let mut errs = vec![];
    if !breaks.is_empty() {
        errs.push(format!(
            "{api}: incompatible with the frozen {api}.{} (a change like this needs a new major served beside this one):\n    {}\n  depends on it: {}",
            frozen["minor"], breaks.join("\n    "), dependents(cur["api"].as_str().unwrap(), cur, all).join(", ")
        ));
    }
    if !adds.is_empty() && cur["minor"] == frozen["minor"] {
        errs.push(format!("{api}: {} without a minor bump (frozen {}.{}, still {}.{})", adds.join(", "), frozen["major"], frozen["minor"], cur["major"], cur["minor"]));
    }
    errs
}

/// The four decl.lint rules of FULLSTACK-CELLS 4.4 on one view source.
pub fn lint(path: &str, src: &str) -> Vec<String> {
    let mut errs = vec![];
    for (i, line) in src.lines().enumerate() {
        let l = line.trim();
        if l.starts_with("//") || l.starts_with('*') || l.starts_with("/*") {
            continue;
        }
        let at = format!("{path}:{}", i + 1);
        let lower = l.to_ascii_lowercase();
        let hides = lower.contains("hidden") || lower.contains("display") && lower.contains("none");
        let about_rights = ["right", "role", "canwrite", "isadmin", "admin", "may("].iter().any(|w| lower.contains(w));
        if hides && about_rights {
            errs.push(format!("{at}: hiding by role or right in the client (the node decides; a view only sees absence)"));
        }
        if (lower.contains("role ==") || lower.contains("role===") || lower.contains("role ===")) || has_right_literal_compare(l) {
            errs.push(format!("{at}: string compare on a role or right"));
        }
        if l.contains("cx.call(") || l.contains("postMessage(") || l.contains(".port") {
            errs.push(format!("{at}: a call that is not through ui/gen"));
        }
        if l.contains("fetch(") || l.contains("XMLHttpRequest") || l.contains("WebSocket") || l.contains("import(") || l.contains("sendBeacon") {
            errs.push(format!("{at}: a fetch of a URL not from the manifest"));
        }
    }
    errs
}

fn has_right_literal_compare(l: &str) -> bool {
    for op in ["===", "!==", "=="] {
        if let Some(i) = l.find(op) {
            let rest = l[i + op.len()..].trim_start();
            if let Some(q) = rest.strip_prefix('\'').or_else(|| rest.strip_prefix('"')) {
                let lit: String = q.chars().take_while(|c| *c != '\'' && *c != '"').collect();
                if lit.contains(':') && lit.chars().all(|c| c.is_ascii_lowercase() || c == ':' || c == '.' || c == '-' || c == '*') {
                    return true;
                }
            }
        }
    }
    false
}

pub fn run(root: &Path) -> Result<String, Vec<String>> {
    let all = fe_host::registry::all();
    let mut errs = vec![];
    let mut checked = 0;
    for ((api, major), cur) in latest(all) {
        let p = frozen_path(root, &api, major);
        match std::fs::read_to_string(&p) {
            Ok(t) => {
                let frozen: Value = serde_json::from_str(&t).map_err(|e| vec![format!("{}: {e}", p.display())])?;
                errs.extend(compare(&frozen, &cur, all));
                checked += 1;
            }
            Err(_) => errs.push(format!("{api}@{major}: no frozen descriptor (run `cargo cell freeze {api}@{major}`)")),
        }
    }
    // slot props: the type a parent names must be the child's props type
    for d in all {
        for v in d["views"].as_array().unwrap() {
            for s in v["slots"].as_array().unwrap() {
                let child = all.iter().flat_map(|c| c["views"].as_array().unwrap().iter()).find(|cv| cv["name"] == s["view"]);
                match child {
                    None => errs.push(format!("{}: slot {} names view {} which no contract declares", v["name"], s["name"], s["view"])),
                    Some(c) if c["props"] != s["props"] => errs.push(format!("{}: slot {} passes {} but {} takes {}", v["name"], s["name"], s["props"], s["view"], c["props"])),
                    _ => {}
                }
            }
        }
    }
    let mut linted = 0;
    if let Ok(dirs) = std::fs::read_dir(root.join("cells")) {
        for d in dirs.flatten() {
            let src = d.path().join("ui/src");
            for f in std::fs::read_dir(&src).into_iter().flatten().flatten() {
                let p = f.path();
                if p.extension().is_some_and(|e| e == "ts") {
                    let rel = p.strip_prefix(root).unwrap().display().to_string();
                    errs.extend(lint(&rel, &std::fs::read_to_string(&p).unwrap()));
                    linted += 1;
                }
            }
        }
    }
    if errs.is_empty() {
        Ok(format!("{checked} contracts match their frozen descriptors; slot props agree; {linted} view sources pass the four decl.lint rules"))
    } else {
        Err(errs)
    }
}

pub fn freeze(root: &Path, which: &str) -> Result<String, String> {
    let all = fe_host::registry::all();
    let mut n = 0;
    for ((api, major), cur) in latest(all) {
        if which != "--all" && which != api && which != format!("{api}@{major}") {
            continue;
        }
        let p = frozen_path(root, &api, major);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, serde_json::to_string_pretty(&cur).unwrap() + "\n").unwrap();
        n += 1;
    }
    if n == 0 {
        return Err(format!("no contract {which}"));
    }
    Ok(format!("froze {n}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn notes() -> Value {
        fe_host::registry::descriptor("notes", 1, 1).unwrap().clone()
    }
    #[test]
    fn a_removed_field_breaks_and_names_dependents() {
        let all = fe_host::registry::all();
        let frozen = notes();
        let mut cur = notes();
        cur["types"] = Value::String(cur["types"].as_str().unwrap().replacen("pub text: String,", "", 1).replacen("pub text : String,", "", 1));
        let e = compare(&frozen, &cur, all);
        assert!(e.len() == 1 && e[0].contains("Note.text removed") && e[0].contains("notes.board (view of notes)"), "{e:?}");
    }
    #[test]
    fn a_retyped_op_and_a_changed_slot_prop_break() {
        let all = fe_host::registry::all();
        let frozen = fe_host::registry::descriptor("trend", 1, 0).unwrap().clone();
        let mut cur = frozen.clone();
        cur["ops"][0]["resp"] = Value::String("Vec<u32>".into());
        let e = compare(&frozen, &cur, all);
        assert!(e[0].contains("op Series: resp") && e[0].contains("notes.board (slot trend of notes@1)") && e[0].contains("status.panel (slot trend of status@1)"), "{e:?}");
    }
    #[test]
    fn an_addition_needs_a_minor_bump() {
        let all = fe_host::registry::all();
        let frozen = fe_host::registry::descriptor("notes", 1, 0).unwrap().clone();
        let mut cur = fe_host::registry::descriptor("notes", 1, 1).unwrap().clone();
        assert!(compare(&frozen, &cur, all).is_empty(), "1.0 -> 1.1 adds an optional field with a minor bump");
        cur["minor"] = Value::from(0);
        assert!(compare(&frozen, &cur, all)[0].contains("without a minor bump"));
    }
    #[test]
    fn lint_rules() {
        assert_eq!(lint("x", "if (rights.includes('notes:admin')) el.hidden = true;").len(), 1);
        assert_eq!(lint("x", "if (r === 'notes:admin') go();").len(), 1);
        assert_eq!(lint("x", "cx.call('Purge', null)").len(), 1);
        assert_eq!(lint("x", "fetch('https://example.com')").len(), 1);
        assert!(lint("x", "const n = await api.List();").is_empty());
    }
}
