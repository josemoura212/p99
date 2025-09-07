use anyhow::Context;

#[allow(unused)]
#[derive(Clone, Debug)]
pub struct Cfg {
    pub port: u16,
    pub upstream_a: String,
    pub upstream_b: String,
    pub pay_path: String,
    pub auth_header_name: Option<String>,
    pub auth_header_value: Option<String>,
    pub request_timeout_ms: u64,
    pub hedge_delay_ms: u64,
    pub concurrency_limit: usize,
    pub cb_fail_rate: f64,
    pub cb_min_samples: usize,
    pub cb_open_secs: u64,
}

impl Cfg {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            port: std::env::var("PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(9999),
            upstream_a: std::env::var("UPSTREAM_A_URL").context("UPSTREAM_A_URL missing")?,
            upstream_b: std::env::var("UPSTREAM_B_URL").context("UPSTREAM_B_URL missing")?,
            pay_path: std::env::var("UPSTREAM_PAY_PATH").unwrap_or_else(|_| "/api/pay".into()),
            auth_header_name: std::env::var("AUTH_HEADER_NAME").ok(),
            auth_header_value: std::env::var("AUTH_HEADER_VALUE").ok(),
            request_timeout_ms: std::env::var("REQUEST_TIMEOUT_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(120),
            hedge_delay_ms: std::env::var("HEDGE_DELAY_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(40),
            concurrency_limit: std::env::var("CONCURRENCY_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1024),
            cb_fail_rate: std::env::var("CB_FAIL_RATE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.25),
            cb_min_samples: std::env::var("CB_MIN_SAMPLES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50),
            cb_open_secs: std::env::var("CB_OPEN_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2),
        })
    }

    pub fn redacted(&self) -> Self {
        let mut c = self.clone();
        c.auth_header_value = c.auth_header_value.as_ref().map(|_| "***".into());
        c
    }
}
