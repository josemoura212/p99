/// Cliente HTTP otimizado para comunicação com processadores upstream
/// Implementa connection pooling, timeouts e headers específicos da Rinha
use std::{sync::Arc, time::Duration};

use reqwest::Client;
use serde_json::Value;

use crate::config::Cfg;

/// Cliente HTTP para comunicação com processadores de pagamento
/// Mantém pool de conexões e configurações otimizadas para alta performance
pub struct UpstreamClient {
    /// Nome identificador do processador (A ou B)
    pub name: String,
    /// Cliente HTTP com pool de conexões compartilhado
    http: Arc<Client>,
}

impl Clone for UpstreamClient {
    /// Implementação customizada de Clone para compartilhar o pool HTTP
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            http: Arc::clone(&self.http),
        }
    }
}

impl UpstreamClient {
    /// Cria novo cliente upstream com configurações otimizadas
    /// # Arguments
    /// * `name` - Nome do processador (A ou B)
    /// * `cfg` - Configurações globais da aplicação
    pub async fn new(name: String, cfg: &Cfg) -> anyhow::Result<Self> {
        // ========== CONFIGURAÇÕES DE PERFORMANCE ==========
        // HTTP/1.1 only para compatibilidade com servidores legacy
        // Pool de conexões agressivo para reduzir latência
        let http = Client::builder()
            .pool_max_idle_per_host(32) // Pool grande para alta concorrência
            .pool_idle_timeout(Duration::from_secs(30)) // Keep-alive por 30s
            .tcp_nodelay(true) // Desabilita Nagle para baixa latência
            .use_rustls_tls() // TLS otimizado
            .connect_timeout(Duration::from_millis(25)) // Timeout de conexão curto
            .timeout(Duration::from_millis(cfg.request_timeout_ms)) // Timeout total da requisição
            .build()?;

        Ok(Self {
            name,
            http: Arc::new(http),
        })
    }

    /// Executa requisição HTTP para o processador upstream
    /// # Arguments
    /// * `cfg` - Configurações da aplicação (Arc para compartilhamento)
    /// * `body` - Payload JSON da requisição
    ///
    /// # Returns
    /// * `Ok((nome, resposta))` - Sucesso com nome do processador e resposta JSON
    /// * `Err((nome, status, mensagem))` - Erro com detalhes para circuit breaker
    pub async fn request(
        &self,
        cfg: Arc<Cfg>,
        body: Value,
    ) -> Result<(String, Value), (String, http::StatusCode, String)> {
        // ========== CONSTRUÇÃO DA URL ==========
        // Seleciona URL base baseado no nome do processador
        let base = if self.name == "A" {
            &cfg.upstream_a
        } else {
            &cfg.upstream_b
        };
        let url = format!("{base}{}", cfg.pay_path);

        // ========== PREPARAÇÃO DA REQUISIÇÃO ==========
        // POST com JSON body e headers específicos da Rinha
        let mut req = self.http.post(&url).json(&body);
        req = req.header("X-Rinha-Token", "123"); // Token obrigatório para processadores oficiais

        // ========== EXECUÇÃO DA REQUISIÇÃO ==========
        match req.send().await {
            Ok(resp) => {
                let sc = resp.status();

                // ========== TRATAMENTO DE SUCESSO ==========
                if sc.is_success() {
                    // Para mock, simula resposta de sucesso da Rinha
                    // Em produção, faria resp.json().await
                    Ok((
                        self.name.clone(),
                        serde_json::json!({
                            "message": "payment processed successfully"
                        }),
                    ))
                } else {
                    // ========== TRATAMENTO DE ERRO HTTP ==========
                    Err((
                        self.name.clone(),
                        sc,
                        format!("upstream {} returned {}", self.name, sc),
                    ))
                }
            }
            Err(e) => {
                // ========== TRATAMENTO DE ERRO DE REDE ==========
                // Connection timeout, DNS failure, etc.
                Err((
                    self.name.clone(),
                    http::StatusCode::BAD_GATEWAY,
                    format!("upstream {} error: {e}", self.name),
                ))
            }
        }
    }
}
