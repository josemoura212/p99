/// Configurações da aplicação load balancer
/// Todas as configurações são carregadas de variáveis de ambiente
/// Valores padrão são fornecidos para desenvolvimento
use anyhow::Context;

/// Estrutura principal de configurações da aplicação
/// Centraliza todas as opções de tuning e endpoints
#[allow(unused)]
#[derive(Clone, Debug)]
pub struct Cfg {
    /// Porta HTTP onde o servidor irá escutar
    pub port: u16,

    /// URL base do processador A (primário)
    pub upstream_a: String,

    /// URL base do processador B (secundário/fallback)
    pub upstream_b: String,

    /// Path da API de pagamento nos processadores upstream
    pub pay_path: String,

    /// Nome do header de autenticação (opcional)
    pub auth_header_name: Option<String>,

    /// Valor do header de autenticação (opcional)
    pub auth_header_value: Option<String>,

    /// Timeout total para requisições HTTP (milissegundos)
    pub request_timeout_ms: u64,

    /// Delay antes de iniciar hedging (milissegundos)
    pub hedge_delay_ms: u64,

    /// Limite máximo de conexões concorrentes
    pub concurrency_limit: usize,

    /// Taxa de falha limite para abrir circuit breaker (0.0-1.0)
    pub cb_fail_rate: f64,

    /// Número mínimo de amostras para calcular taxa de falha
    pub cb_min_samples: usize,

    /// Tempo que circuit breaker fica aberto (segundos)
    pub cb_open_secs: u64,
}

impl Cfg {
    /// Carrega configurações de variáveis de ambiente
    /// Fornece valores padrão para desenvolvimento
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            // ========== CONFIGURAÇÃO DO SERVIDOR ==========
            port: std::env::var("PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(9999), // Porta padrão para desenvolvimento

            // ========== ENDPOINTS DOS PROCESSADORES ==========
            upstream_a: std::env::var("UPSTREAM_A_URL").context("UPSTREAM_A_URL missing")?, // Obrigatório
            upstream_b: std::env::var("UPSTREAM_B_URL").context("UPSTREAM_B_URL missing")?, // Obrigatório
            pay_path: std::env::var("UPSTREAM_PAY_PATH").unwrap_or_else(|_| "/api/pay".into()), // Path padrão

            // ========== AUTENTICAÇÃO ==========
            auth_header_name: std::env::var("AUTH_HEADER_NAME").ok(),
            auth_header_value: std::env::var("AUTH_HEADER_VALUE").ok(),

            // ========== TIMEOUTS E PERFORMANCE ==========
            request_timeout_ms: std::env::var("REQUEST_TIMEOUT_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(120), // 120ms timeout padrão
            hedge_delay_ms: std::env::var("HEDGE_DELAY_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(40), // 40ms para hedging
            concurrency_limit: std::env::var("CONCURRENCY_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1024), // 1024 conexões concorrentes

            // ========== CIRCUIT BREAKER ==========
            cb_fail_rate: std::env::var("CB_FAIL_RATE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.25), // 25% de falha abre circuito
            cb_min_samples: std::env::var("CB_MIN_SAMPLES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50), // Mínimo 50 amostras
            cb_open_secs: std::env::var("CB_OPEN_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2), // 2 segundos aberto
        })
    }

    /// Retorna cópia da configuração com valores sensíveis mascarados
    /// Útil para logging sem expor secrets
    pub fn redacted(&self) -> Self {
        let mut c = self.clone();
        // Mascarar valor do header de autenticação
        c.auth_header_value = c.auth_header_value.as_ref().map(|_| "***".into());
        c
    }
}
