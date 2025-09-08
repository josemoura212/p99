use std::{sync::Arc, time::Duration};

use reqwest::Client;
use serde_json::Value;

use crate::config::Cfg;

pub struct UpstreamClient {
    pub name: String,
    http: Arc<Client>,
}

impl Clone for UpstreamClient {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            http: Arc::clone(&self.http),
        }
    }
}

impl UpstreamClient {
    pub async fn new(name: String, cfg: &Cfg) -> anyhow::Result<Self> {
        // HTTP/1.1 only + keep-alive agressivo + timeouts curtos
        let http = Client::builder()
            .pool_max_idle_per_host(32)
            .pool_idle_timeout(Duration::from_secs(30))
            .tcp_nodelay(true)
            .use_rustls_tls()
            .connect_timeout(Duration::from_millis(25))
            .timeout(Duration::from_millis(cfg.request_timeout_ms))
            .build()?;
        Ok(Self {
            name,
            http: Arc::new(http),
        })
    }

    pub async fn request(
        &self,
        cfg: Arc<Cfg>,
        body: Value,
    ) -> Result<(String, Value), (String, http::StatusCode, String)> {
        let base = if self.name == "A" {
            &cfg.upstream_a
        } else {
            &cfg.upstream_b
        };
        let url = format!("{base}{}", cfg.pay_path);

        let mut req = self.http.post(&url).json(&body);
        if let (Some(hn), Some(hv)) = (&cfg.auth_header_name, &cfg.auth_header_value) {
            req = req.header(hn, hv);
        }

        match req.send().await {
            Ok(resp) => {
                let sc = resp.status();
                if sc.is_success() {
                    // Para mock, simular resposta de sucesso da rinha
                    Ok((
                        self.name.clone(),
                        serde_json::json!({
                            "message": "payment processed successfully"
                        }),
                    ))
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
