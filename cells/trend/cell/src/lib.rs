//! The trend core: per-second series from node beats (calls served) and from pushes (notes written).
use fe_runtime::{parse, to, Caller, Cell, Cx};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use trend_api::{Points, Sample, TrendProps};

pub const KEEP: usize = 60;

#[derive(Default)]
pub struct Trend {
    pub series: BTreeMap<String, BTreeMap<String, VecDeque<(u64, f64)>>>,
    last_calls: BTreeMap<String, u64>,
}

impl Trend {
    fn add(&mut self, series: &str, key: &str, t: u64, v: f64, sum: bool) {
        let q = self.series.entry(series.into()).or_default().entry(key.into()).or_default();
        let sec = t / 1000;
        match q.back_mut() {
            Some((s, x)) if *s == sec => {
                if sum {
                    *x += v
                } else {
                    *x = v
                }
            }
            _ => q.push_back((sec, v)),
        }
        while q.len() > KEEP {
            q.pop_front();
        }
    }
}

impl Cell for Trend {
    fn name(&self) -> &'static str {
        "trend"
    }
    fn serve(&mut self, _: u32, _: u32) {}
    fn beat(&mut self, from: &str, calls: u64, now: u64) {
        let prev = self.last_calls.insert(from.into(), calls).unwrap_or(calls);
        self.add("calls", from, now, calls.saturating_sub(prev) as f64, false);
    }
    fn handle(&mut self, _major: u32, op: &str, body: Value, cx: &mut Cx) -> Result<Value, String> {
        match op {
            "Series" => {
                let p: TrendProps = parse(body)?;
                let lines = self
                    .series
                    .get(&p.series)
                    .map(|m| m.iter().map(|(k, q)| (k.clone(), q.iter().copied().collect())).collect())
                    .unwrap_or_default();
                Ok(to(&Points { series: p.series, lines }))
            }
            "Push" => {
                let Caller::Node(_) = cx.caller else { return Err("Push is for nodes".into()) };
                let s: Sample = parse(body)?;
                self.add(&s.series, &s.key, cx.now, s.v, true);
                Ok(Value::Null)
            }
            _ => Err(format!("trend has no op {op}")),
        }
    }
}
