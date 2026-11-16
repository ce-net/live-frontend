//! `contract!` takes the text of a `ce_wire::api!` contract (as `wire-lite` reads it) plus the `ui { }` block of
//! FULLSTACK-CELLS section 2. The first half is handed to `wire_lite::api!` unchanged, so the api name, op ids,
//! rights and schema hash are the ones the real crates compute. The second half becomes `VIEWS`, and the build
//! fails (a const assertion) when a view names an op that does not exist or reads data whose right the view does
//! not declare in `needs`. `tries` lists ops a view may attempt and must expect to be refused (shown as absent).
pub use serde_json;
pub use wire_lite;
use wire_lite::{Api, OpShape};

#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub name: &'static str,
    pub view: &'static str,
    pub props: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct View {
    pub name: &'static str,
    pub place: &'static str,
    pub needs: &'static [&'static str],
    pub data: &'static [&'static str],
    pub tries: &'static [&'static str],
    pub streams: &'static [&'static str],
    pub props: &'static str,
    pub state: &'static str,
    pub slots: &'static [Slot],
    pub speaks: &'static [&'static str],
    pub hears: &'static [&'static str],
}

pub trait Contract: Api {
    const MINOR: u32;
    const TYPES: &'static str;
    const VIEWS: &'static [View];
    /// Per op, the keywords after its right (`counted`: each call spends one use of a counted chain).
    const KW: &'static [(&'static str, &'static [&'static str])];
}

pub const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// `granted` covers `needed` when equal, or `granted` is `"<api>:*"` and `needed` starts with `"<api>:"`.
pub const fn covers(granted: &str, needed: &str) -> bool {
    if str_eq(granted, needed) {
        return true;
    }
    let (g, n) = (granted.as_bytes(), needed.as_bytes());
    if g.len() < 2 || g[g.len() - 1] != b'*' || g[g.len() - 2] != b':' || n.len() < g.len() - 1 {
        return false;
    }
    let mut i = 0;
    while i < g.len() - 1 {
        if g[i] != n[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn right_of(ops: &[OpShape], rights: &[&str], op: &str) -> Option<usize> {
    let mut i = 0;
    while i < ops.len() {
        if str_eq(ops[i].name, op) {
            let _ = rights;
            return Some(i);
        }
        i += 1;
    }
    None
}

const fn needs_cover(needs: &[&str], right: &str) -> bool {
    let mut i = 0;
    while i < needs.len() {
        if covers(needs[i], right) {
            return true;
        }
        i += 1;
    }
    false
}

/// The compile-time check of the `ui { }` block. Called in a `const _` by `contract!`.
pub const fn check_views(ops: &[OpShape], rights: &[&str], views: &[View]) {
    let mut v = 0;
    while v < views.len() {
        let view = &views[v];
        let mut k = 0;
        while k < view.data.len() + view.streams.len() {
            let op = if k < view.data.len() { view.data[k] } else { view.streams[k - view.data.len()] };
            match right_of(ops, rights, op) {
                None => panic!("contract!: a view's data or streams name an op that is not in ops"),
                Some(i) => {
                    if !needs_cover(view.needs, rights[i]) {
                        panic!("contract!: a view reads an op whose right is not in the view's needs");
                    }
                }
            }
            k += 1;
        }
        let mut t = 0;
        while t < view.tries.len() {
            if right_of(ops, rights, view.tries[t]).is_none() {
                panic!("contract!: a view's tries name an op that is not in ops");
            }
            t += 1;
        }
        v += 1;
    }
}

/// The machine-readable contract: what `cargo cell gen` turns into TypeScript and `cargo cell freeze` stores.
pub fn descriptor<C: Contract>() -> serde_json::Value {
    use serde_json::json;
    let ops: Vec<_> = C::OPS
        .iter()
        .zip(C::RIGHTS)
        .map(|(o, r)| {
            let kw = C::KW.iter().find(|(n, _)| *n == o.name).map(|(_, k)| k.to_vec()).unwrap_or_default();
            json!({"id": o.id, "name": o.name, "req": o.req.replace(' ', ""), "resp": o.resp.replace(' ', ""), "right": r, "kw": kw})
        })
        .collect();
    let views: Vec<_> = C::VIEWS
        .iter()
        .map(|v| {
            json!({"name": v.name, "place": v.place, "needs": v.needs, "data": v.data, "tries": v.tries, "streams": v.streams,
                "props": v.props, "state": v.state,
                "slots": v.slots.iter().map(|s| json!({"name": s.name, "view": s.view, "props": s.props})).collect::<Vec<_>>(),
                "speaks": v.speaks, "hears": v.hears})
        })
        .collect();
    json!({"api": C::NAME, "major": C::VERSION, "minor": C::MINOR, "schema": format!("{:016x}", C::SCHEMA),
        "types": C::TYPES, "ops": ops, "views": views})
}

#[macro_export]
macro_rules! contract {
    (
        api $api:ident = $name:literal, version $ver:literal, minor $minor:literal ;
        types { $($ty:item)* }
        ops { $( $id:literal => $op:ident ( $req:ty ) -> $resp:ty , needs $right:literal $(, $kw:ident)* ; )* }
        ui { $(
            view $vname:literal place $place:literal {
                needs [$($need:literal),* $(,)?];
                data [$($data:ident),* $(,)?];
                tries [$($try:ident),* $(,)?];
                streams [$($stream:ident),* $(,)?];
                props $props:literal;
                state $state:literal;
                slots [$($sname:ident : $sview:literal ($sprops:literal)),* $(,)?];
                speaks [$($speak:literal),* $(,)?];
                hears [$($hear:literal),* $(,)?];
            }
        )* }
    ) => {
        $crate::wire_lite::api! {
            api $api = $name, version $ver;
            types { $($ty)* }
            ops { $( $id => $op ( $req ) -> $resp , needs $right $(, $kw)* ; )* }
        }
        impl $crate::Contract for $api {
            const MINOR: u32 = $minor;
            const TYPES: &'static str = stringify!($($ty)*);
            const VIEWS: &'static [$crate::View] = &[$(
                $crate::View {
                    name: $vname, place: $place,
                    needs: &[$($need),*],
                    data: &[$(stringify!($data)),*],
                    tries: &[$(stringify!($try)),*],
                    streams: &[$(stringify!($stream)),*],
                    props: $props, state: $state,
                    slots: &[$($crate::Slot { name: stringify!($sname), view: $sview, props: $sprops }),*],
                    speaks: &[$($speak),*],
                    hears: &[$($hear),*],
                }
            ),*];
            const KW: &'static [(&'static str, &'static [&'static str])] = &[$( (stringify!($op), &[$(stringify!($kw)),*]) ),*];
        }
        const _: () = $crate::check_views(
            <$api as $crate::wire_lite::Api>::OPS,
            <$api as $crate::wire_lite::Api>::RIGHTS,
            <$api as $crate::Contract>::VIEWS,
        );
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn covers_wildcards() {
        assert!(covers("notes:*", "notes:read"));
        assert!(covers("notes:read", "notes:read"));
        assert!(!covers("notes:*", "notesx:read"));
        assert!(!covers("notes:read", "notes:write"));
    }
}

/// A type declared in a contract's `types { }`, read back from its token text (for TypeScript, frozen
/// descriptors and slot prop checks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDef {
    Struct { name: String, fields: Vec<(String, String)> },
    Enum { name: String, variants: Vec<(String, Option<Vec<(String, String)>>, Option<String>)> },
}

impl TypeDef {
    pub fn name(&self) -> &str {
        match self {
            TypeDef::Struct { name, .. } | TypeDef::Enum { name, .. } => name,
        }
    }
}

fn tokens(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            if !c.is_whitespace() {
                out.push(c.to_string());
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Parses the `stringify!` text of `types { }` (structs with named fields, enums with unit, tuple(1) or struct variants).
pub fn parse_types(text: &str) -> Vec<TypeDef> {
    let t = tokens(text);
    let mut i = 0;
    let mut out = vec![];
    // reads a type until `,` / `}` / `)` at depth 0
    fn ty(t: &[String], i: &mut usize) -> String {
        let mut d = 0i32;
        let mut s = String::new();
        while *i < t.len() {
            let x = t[*i].as_str();
            if d == 0 && (x == "," || x == "}" || x == ")") {
                break;
            }
            if x == "<" || x == "(" || x == "[" {
                d += 1;
            }
            if x == ">" || x == ")" || x == "]" {
                d -= 1;
            }
            s.push_str(x);
            *i += 1;
        }
        s
    }
    fn fields(t: &[String], i: &mut usize) -> Vec<(String, String)> {
        let mut f = vec![];
        *i += 1; // {
        while *i < t.len() && t[*i] != "}" {
            if t[*i] == "#" {
                skip_attr(t, i);
                continue;
            }
            if t[*i] == "pub" {
                *i += 1;
            }
            let name = t[*i].clone();
            *i += 2; // name :
            let ty = ty(t, i);
            f.push((name, ty));
            if *i < t.len() && t[*i] == "," {
                *i += 1;
            }
        }
        *i += 1; // }
        f
    }
    fn skip_attr(t: &[String], i: &mut usize) {
        *i += 1; // #
        let mut d = 0;
        while *i < t.len() {
            if t[*i] == "[" {
                d += 1;
            }
            if t[*i] == "]" {
                d -= 1;
                if d == 0 {
                    *i += 1;
                    return;
                }
            }
            *i += 1;
        }
    }
    while i < t.len() {
        match t[i].as_str() {
            "#" => skip_attr(&t, &mut i),
            "pub" => i += 1,
            "struct" => {
                let name = t[i + 1].clone();
                i += 2;
                if t[i] == "{" {
                    out.push(TypeDef::Struct { name, fields: fields(&t, &mut i) });
                } else {
                    while i < t.len() && t[i] != ";" {
                        i += 1;
                    }
                    i += 1;
                    out.push(TypeDef::Struct { name, fields: vec![] });
                }
            }
            "enum" => {
                let name = t[i + 1].clone();
                i += 3; // enum Name {
                let mut variants = vec![];
                while i < t.len() && t[i] != "}" {
                    if t[i] == "#" {
                        skip_attr(&t, &mut i);
                        continue;
                    }
                    let v = t[i].clone();
                    i += 1;
                    if t[i] == "{" {
                        variants.push((v, Some(fields(&t, &mut i)), None));
                    } else if t[i] == "(" {
                        i += 1;
                        let inner = ty(&t, &mut i);
                        i += 1;
                        variants.push((v, None, Some(inner)));
                    } else {
                        variants.push((v, None, None));
                    }
                    if i < t.len() && t[i] == "," {
                        i += 1;
                    }
                }
                i += 1;
                out.push(TypeDef::Enum { name, variants });
            }
            _ => i += 1,
        }
    }
    out
}

/// The TypeScript type for a Rust type as serde_json encodes it.
pub fn ts_type(rust: &str) -> String {
    let r = rust.replace(' ', "");
    if let Some(inner) = r.strip_prefix("Option<").and_then(|x| x.strip_suffix('>')) {
        return format!("{} | null", ts_type(inner));
    }
    if let Some(inner) = r.strip_prefix("Vec<").and_then(|x| x.strip_suffix('>')) {
        let t = ts_type(inner);
        return if t.contains(" | ") { format!("({t})[]") } else { format!("{t}[]") };
    }
    if let Some(inner) = r.strip_prefix("BTreeMap<String,").and_then(|x| x.strip_suffix('>')) {
        return format!("Record<string, {}>", ts_type(inner));
    }
    if r.starts_with('(') && r.ends_with(')') && r.len() > 2 {
        let parts: Vec<String> = split_top(&r[1..r.len() - 1]).iter().map(|p| ts_type(p)).collect();
        return format!("[{}]", parts.join(", "));
    }
    match r.as_str() {
        "String" | "str" => "string".into(),
        "u8" | "u16" | "u32" | "u64" | "i32" | "i64" | "f32" | "f64" | "usize" => "number".into(),
        "bool" => "boolean".into(),
        "()" => "null".into(),
        "Value" | "serde_json::Value" => "unknown".into(),
        other => other.to_string(),
    }
}

fn split_top(s: &str) -> Vec<String> {
    let (mut d, mut cur, mut out) = (0, String::new(), vec![]);
    for c in s.chars() {
        match c {
            '<' | '(' => d += 1,
            '>' | ')' => d -= 1,
            _ => {}
        }
        if c == ',' && d == 0 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod parse_tests {
    use super::*;
    #[test]
    fn reads_structs_and_enums() {
        let t = parse_types("pub struct Note { pub id : String, pub colour : Option < String >, pub pts: Vec<(u64, f64)>, } pub enum D { Reset, Put(Note), Del { id: String } }");
        assert_eq!(t[0], TypeDef::Struct { name: "Note".into(), fields: vec![("id".into(), "String".into()), ("colour".into(), "Option<String>".into()), ("pts".into(), "Vec<(u64,f64)>".into())] });
        assert_eq!(ts_type("Vec<(u64,f64)>"), "[number, number][]");
        assert_eq!(ts_type("Option<Vec<String>>"), "string[] | null");
        match &t[1] {
            TypeDef::Enum { variants, .. } => assert_eq!(variants.len(), 3),
            _ => panic!(),
        }
    }
}
