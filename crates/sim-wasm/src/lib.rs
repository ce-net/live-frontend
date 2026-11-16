//! A C-ABI wasm module with no bindings generator: the page passes JSON in linear memory and gets JSON back.
//! `lf_new(config)` makes this instance's node, `lf_input(input)` is `Host::input`, `lf_configs` derives the sim
//! network, `lf_verify` checks a recorded `run.jsonl` (signatures and hash chain) with the same `live-proof` code.
use fe_host::{Config, Host};
use std::cell::RefCell;

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn ret(s: String) -> u64 {
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        *o = s.into_bytes();
        ((o.as_ptr() as u64) << 32) | o.len() as u64
    })
}

unsafe fn arg(p: *mut u8, n: usize) -> String {
    let v = Vec::from_raw_parts(p, n, n);
    String::from_utf8(v).unwrap_or_default()
}

#[no_mangle]
pub extern "C" fn lf_alloc(n: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(n);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `p` must come from `lf_alloc(n)` and hold `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn lf_new(p: *mut u8, n: usize) -> u64 {
    let s = arg(p, n);
    let r = serde_json::from_str::<Config>(&s).map_err(|e| e.to_string()).and_then(Host::new);
    ret(match r {
        Ok(h) => {
            let k = h.public_key();
            HOST.with(|x| *x.borrow_mut() = Some(h));
            serde_json::json!({"ok": true, "key": k}).to_string()
        }
        Err(e) => serde_json::json!({"ok": false, "error": e}).to_string(),
    })
}

/// # Safety
/// As `lf_new`.
#[no_mangle]
pub unsafe extern "C" fn lf_input(p: *mut u8, n: usize) -> u64 {
    let s = arg(p, n);
    ret(HOST.with(|x| match x.borrow_mut().as_mut() {
        Some(h) => h.input_json(&s),
        None => "{\"reply\":{\"ok\":false,\"body\":null,\"refused\":\"no node\"},\"out\":[]}".into(),
    }))
}

/// # Safety
/// As `lf_new`. Input: `{"seed": "...", "now": ms, "commit": "..."}`.
#[no_mangle]
pub unsafe extern "C" fn lf_configs(p: *mut u8, n: usize) -> u64 {
    let v: serde_json::Value = serde_json::from_str(&arg(p, n)).unwrap_or_default();
    let (_root, cfgs) = fe_host::sim::configs(v["seed"].as_str().unwrap_or("page"), v["now"].as_u64().unwrap_or(0), v["commit"].as_str().unwrap_or(""));
    ret(serde_json::to_string(&cfgs).unwrap())
}

/// # Safety
/// As `lf_new`. Input: `{"jsonl": "...", "keys": [...]}`.
#[no_mangle]
pub unsafe extern "C" fn lf_verify(p: *mut u8, n: usize) -> u64 {
    let v: serde_json::Value = serde_json::from_str(&arg(p, n)).unwrap_or_default();
    let keys: Vec<String> = v["keys"].as_array().map(|a| a.iter().filter_map(|k| k.as_str().map(String::from)).collect()).unwrap_or_default();
    ret(match live_proof::verify(v["jsonl"].as_str().unwrap_or(""), &keys) {
        Ok(n) => serde_json::json!({"ok": true, "steps": n}).to_string(),
        Err(e) => serde_json::json!({"ok": false, "error": e}).to_string(),
    })
}
