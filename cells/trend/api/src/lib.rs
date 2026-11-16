//! trend: small time series (calls per node, notes written), embedded by other views through a typed slot.
lf_contract::contract! {
    api Trend = "trend", version 1, minor 0;
    types {
        pub struct TrendProps { pub series: String }
        pub struct Points { pub series: String, pub lines: Vec<(String, Vec<(u64, f64)>)> }
        pub struct Sample { pub series: String, pub key: String, pub v: f64 }
    }
    ops {
        1 => Series(TrendProps) -> Points, needs "trend:read";
        2 => Push(Sample) -> (), needs "trend:push";
    }
    ui {
        view "trend.chart" place "slot" {
            needs ["trend:read"]; data [Series]; tries []; streams [];
            props "TrendProps"; state "()"; slots []; speaks []; hears [];
        }
    }
}
