use std::time::Duration;

use reqwest::Client;
use serde_json::Value;
use tracing::debug;

use crate::config::Cfg;

pub struct UpstreamClient {
    pub name: String,
    http: Client,
}

impl UpstreamClient {
    pub async fn new(name: String, cfg: &Cfg) -> anyhow::Result<Self> {
        // HTTP/1.1 only + keep-alive agressivo + timeouts curtos
        let http = Client::builder()
            .pool_max_idle_per_host(16)
            .pool_idle_timeout(Duration::from_secs(10))
            .tcp_nodelay(true)
            .http1_only()
            .use_rustls_tls()
            .connect_timeout(Duration::from_millis(50))
            .timeout(Duration::from_millis(cfg.request_timeout_ms))
            .build()?;
        Ok(Self { name, http })
    }

    pub async fn request(
        &self,
        cfg: &Cfg,
        body: &Value,
    ) -> Result<(String, Value), (String, http::StatusCode, String)> {
        let base = if self.name == "A" {
            &cfg.upstream_a
        } else {
            &cfg.upstream_b
        };
        let url = format!("{base}{}", cfg.pay_path);

        let mut req = self.http.post(&url).json(body);
        if let (Some(hn), Some(hv)) = (&cfg.auth_header_name, &cfg.auth_header_value) {
            req = req.header(hn, hv);
        }

        match req.send().await {
            Ok(resp) => {
                let sc = resp.status();
                if sc.is_success() {
                    match resp.json::<Value>().await {
                        Ok(js) => Ok((self.name.clone(), js)),
                        Err(e) => Err((
                            self.name.clone(),
                            http::StatusCode::BAD_GATEWAY,
                            format!("invalid json: {e}"),
                        )),
                    }
                } else {
                    Err((
                        self.name.clone(),
                        sc,
                        format!("upstream {} returned {}", self.name, sc),
                    ))
                }
            }
            Err(e) => Err((
                self.name.clone(),
                http::StatusCode::BAD_GATEWAY,
                format!("upstream {} error: {e}", self.name),
            )),
        }
    }
}
