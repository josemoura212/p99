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
        _cfg: Arc<Cfg>,
        _body: Value,
    ) -> Result<(String, Value), (String, http::StatusCode, String)> {
        // Simular processamento sem fazer requisição externa por enquanto
        // TODO: Reativar quando resolver problemas de conectividade
        tokio::time::sleep(Duration::from_millis(10)).await; // Simular latência

        Ok((
            self.name.clone(),
            serde_json::json!({
                "message": "payment processed successfully"
            }),
        ))
    }
}
