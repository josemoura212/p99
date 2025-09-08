use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    routing::{get, post},
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpListener;
use tracing::info;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// ========== MÓDULOS PRÓPRIOS ==========
mod breaker;
mod config;
mod strategy;
mod upstream;

// ========== IMPORTS DOS MÓDULOS ==========
use breaker::Breaker;
use config::Cfg;
use moka::sync::Cache;
use strategy::RouteStrategy;
use upstream::UpstreamClient;

/// Estado global da aplicação - compartilhado entre todas as threads
/// Usa Arc (Atomic Reference Counting) para compartilhamento seguro entre threads
#[derive(Clone)]
struct AppState {
    cfg: Arc<Cfg>,                   // Configuração da aplicação
    up_a: Arc<UpstreamClient>,       // Cliente para Payment Processor A
    up_b: Arc<UpstreamClient>,       // Cliente para Payment Processor B
    breaker_a: Arc<Breaker>,         // Circuit Breaker para serviço A
    breaker_b: Arc<Breaker>,         // Circuit Breaker para serviço B
    strategy: Arc<RouteStrategy>,    // Estratégia de roteamento
    idem: Cache<String, ()>,         // Cache de idempotência (correlationId -> ())
    stats: Arc<Mutex<PaymentStats>>, // Estatísticas globais (protegidas por Mutex)
}

/// Estatísticas globais de processamento de pagamentos
/// Separadas por processador (default/fallback)
#[derive(Default)]
struct PaymentStats {
    default: ProcessorStats,  // Estatísticas do Payment Processor A
    fallback: ProcessorStats, // Estatísticas do Payment Processor B
}

/// Estatísticas por processador individual
#[derive(Default)]
struct ProcessorStats {
    total_requests: u64, // Total de requests processados
    total_amount: f64,   // Valor total processado
}

#[derive(Deserialize)]
struct PayIn {
    #[serde(rename = "correlationId")]
    correlation_id: String, // Campo da rinha
    amount: f64,
}

#[derive(Deserialize)]
struct TransacaoIn {
    valor: i64,
    tipo: String,
    descricao: String,
}

#[derive(Serialize)]
struct TransacaoOut {
    limite: i64,
    saldo: i64,
}

#[derive(Serialize)]
struct PayOut {
    message: String, // Ajustado para rinha
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PaymentSummary {
    default: ProcessorSummary,
    fallback: ProcessorSummary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessorSummary {
    total_requests: u64,
    total_amount: f64,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct PaymentsSummaryQuery {
    from: Option<String>,
    to: Option<String>,
}

/// Função principal da aplicação
/// Inicializa todos os componentes e inicia o servidor HTTP
#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    // ========== CONFIGURAÇÃO DE LOGGING ==========
    // Logging enxuto focado apenas no necessário
    // - p99=info: logs da nossa aplicação
    // - axum=warn: reduz logs do framework
    // - tower_http=warn: reduz logs de middleware
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("p99=info".parse().unwrap())
                .add_directive("axum=warn".parse().unwrap())
                .add_directive("tower_http=warn".parse().unwrap()),
        )
        .with_target(false) // Remove target dos logs
        .compact() // Formato compacto
        .init();

    // ========== CONFIGURAÇÃO DE MÉTRICAS ==========
    // Prometheus para métricas de observabilidade
    // Permite monitorar performance, throughput, erros
    let prom_handle: PrometheusHandle = PrometheusBuilder::new()
        .install_recorder()
        .expect("install recorder");

    // ========== CARREGAMENTO DE CONFIGURAÇÃO ==========
    // Carrega configuração de variáveis de ambiente
    // Usa Arc para compartilhamento seguro entre threads
    let cfg = Arc::new(Cfg::from_env()?);
    info!("cfg: {:?}", cfg.redacted()); // Log sem dados sensíveis

    // ========== INICIALIZAÇÃO DOS UPSTREAM CLIENTS ==========
    // Cria clientes HTTP para os Payment Processors
    // Usa connection pooling e timeouts otimizados
    let up_a = Arc::new(UpstreamClient::new("A".into(), &cfg).await?);
    let up_b = Arc::new(UpstreamClient::new("B".into(), &cfg).await?);

    // ========== CIRCUIT BREAKERS ==========
    // Protege contra cascata de falhas
    // Abre automaticamente se taxa de erro for alta
    let breaker_a = Arc::new(Breaker::new(
        cfg.cb_min_samples,                    // Mínimo de amostras para avaliar
        cfg.cb_fail_rate,                      // Taxa de falha para abrir
        Duration::from_secs(cfg.cb_open_secs), // Tempo aberto
    ));
    let breaker_b = Arc::new(Breaker::new(
        cfg.cb_min_samples,
        cfg.cb_fail_rate,
        Duration::from_secs(cfg.cb_open_secs),
    ));

    // ========== ESTRATÉGIA DE ROTEAMENTO ==========
    // Define como distribuir carga entre os processadores
    let strategy = Arc::new(RouteStrategy::new());

    // ========== CACHE DE IDEMPOTÊNCIA ==========
    // Previne processamento duplicado de requests
    // TTL curto para liberar memória rapidamente
    let idem = Cache::builder()
        .max_capacity(500_000) // Capacidade otimizada
        .time_to_live(Duration::from_secs(30)) // TTL de 30s
        .build();

    // ========== ESTADO GLOBAL ==========
    // Tudo compartilhado entre threads via Arc
    let state = AppState {
        cfg,
        up_a,
        up_b,
        breaker_a,
        breaker_b,
        strategy,
        idem,
        stats: Arc::new(Mutex::new(PaymentStats::default())),
    };

    // ========== CONFIGURAÇÃO DAS ROTAS ==========
    // Router do Axum com todas as endpoints
    let prom_handle_route = prom_handle.clone();
    let app = Router::new()
        .route("/payments", post(pay)) // Processamento de pagamentos
        .route("/payments-summary", get(payments_summary)) // Estatísticas
        .route("/purge-payments", post(purge_payments)) // Reset de estatísticas
        .route("/clientes/{id}/transacoes", post(transacao)) // Transações da Rinha
        .route("/healthz", get(|| async { "ok" })) // Health check
        .route("/readyz", get(|| async { "ready" })) // Readiness check
        .route(
            "/metrics",
            get(move || {
                // Métricas Prometheus
                let h = prom_handle_route.clone();
                async move { h.render() }
            }),
        )
        .with_state(state.clone());

    // ========== INICIALIZAÇÃO DO SERVIDOR ==========
    // Bind na porta configurada
    let addr: SocketAddr = format!("0.0.0.0:{}", state.cfg.port).parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!("listening on {}", addr);

    // Inicia servidor com graceful shutdown
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

/// Handler principal para processamento de pagamentos
/// Implementa toda a lógica de load balancing, circuit breaker e hedging
async fn pay(
    State(st): State<AppState>, // Estado global da aplicação
    headers: HeaderMap,         // Headers HTTP da requisição
    Json(body): Json<PayIn>,    // Payload JSON da requisição
) -> Result<(StatusCode, Json<PayOut>), (StatusCode, String)> {
    // ========== AUTENTICAÇÃO ==========
    // Verifica token de autenticação nos headers
    // Compatível com o sistema de teste da Rinha
    if let Some(required) = st.cfg.auth_header_value.clone() {
        let name = st
            .cfg
            .auth_header_name
            .as_deref()
            .unwrap_or("Authorization");

        match headers.get(name) {
            Some(v) if v == HeaderValue::from_str(&required).unwrap() => {
                // Autenticação OK, continua
            }
            _ => return Err((StatusCode::UNAUTHORIZED, "unauthorized".into())),
        }
    }

    // ========== IDEMPOTÊNCIA ==========
    // Previne processamento duplicado do mesmo correlationId
    // Usa cache TTL para liberar memória automaticamente
    let key = body.correlation_id.as_str();
    if st.idem.get(key).is_some() {
        return Err((
            StatusCode::CONFLICT,
            "duplicate correlation_id (ttl)".into(),
        ));
    }

    // ========== SELEÇÃO DE PROCESSADOR ==========
    // Escolhe primário e secundário baseado na estratégia
    // Considera estado dos circuit breakers
    let (prim, sec, prim_brk) = if st.strategy.pick_a_first(&st.breaker_a, &st.breaker_b) {
        (&st.up_a, &st.up_b, &st.breaker_a) // A é primário
    } else {
        (&st.up_b, &st.up_a, &st.breaker_b) // B é primário
    };

    // ========== PREPARAÇÃO DO PAYLOAD ==========
    // Cria payload para o upstream no formato da Rinha
    // Gera novo correlationId para evitar conflitos
    let correlation_id = uuid::Uuid::new_v4().to_string();
    let requested_at = chrono::Utc::now().to_rfc3339();
    let req_body = serde_json::json!({
        "correlationId": correlation_id,
        "amount": body.amount,
        "requestedAt": requested_at,
    });

    // ========== MÉTRICA DE LATÊNCIA ==========
    let start = std::time::Instant::now();

    // ========== HEDGING OTIMIZADO ==========
    // Estratégia: tenta primary primeiro, só faz hedge se necessário
    let result = if prim_brk.is_open() {
        // Circuit breaker aberto - vai direto pro secundário
        st.strategy.note_skip_primary();
        sec.clone()
            .request(Arc::clone(&st.cfg), req_body.clone())
            .await
    } else {
        // Circuit breaker fechado - tenta primary com timeout
        let primary_timeout = Duration::from_millis(st.cfg.hedge_delay_ms);
        match tokio::time::timeout(
            primary_timeout,
            prim.clone().request(Arc::clone(&st.cfg), req_body.clone()),
        )
        .await
        {
            Ok(Ok(result)) => Ok(result), // Primary conseguiu dentro do timeout
            _ => {
                // Primary falhou ou demorou - tenta secondary
                sec.clone()
                    .request(Arc::clone(&st.cfg), req_body.clone())
                    .await
            }
        }
    };

    // ========== CÁLCULO DE LATÊNCIA ==========
    let elapsed = start.elapsed().as_millis() as u64;
    metrics::histogram!("payments_latency_ms").record(elapsed as f64);

    // ========== PROCESSAMENTO DO RESULTADO ==========
    match result {
        Ok((proc_name, _echo)) => {
            // ========== SUCESSO ==========
            // Registra no cache de idempotência
            st.idem.insert(key.to_string(), ());

            // Atualiza estatísticas globais
            {
                let mut stats = st.stats.lock().unwrap();
                if proc_name == "A" {
                    stats.default.total_requests += 1;
                    stats.default.total_amount += body.amount;
                    st.breaker_a.on_success(); // Notifica sucesso
                } else {
                    stats.fallback.total_requests += 1;
                    stats.fallback.total_amount += body.amount;
                    st.breaker_b.on_success(); // Notifica sucesso
                }
            }

            // Registra métrica de sucesso
            metrics::counter!("payments_ok").increment(1);

            Ok((
                StatusCode::OK,
                Json(PayOut {
                    message: "payment processed successfully".into(),
                }),
            ))
        }
        Err((proc_name, code, msg)) => {
            // ========== ERRO ==========
            // Notifica circuit breaker sobre falha
            if proc_name == "A" {
                st.breaker_a.on_failure();
            } else {
                st.breaker_b.on_failure();
            }

            // Registra métrica de erro com código HTTP
            metrics::counter!("payments_err", "code" => code.as_u16().to_string()).increment(1);

            Err((code, msg))
        }
    }
}

/// Handler para processamento de transações de clientes
/// Implementa a lógica de débito/crédito com validações da Rinha de Backend
/// Usa o mesmo mecanismo de load balancing e circuit breaker do pay()
async fn transacao(
    State(st): State<AppState>,     // Estado global da aplicação
    Path(cliente_id): Path<String>, // ID do cliente via URL path
    Json(body): Json<TransacaoIn>,  // Payload JSON da transação
) -> Result<(StatusCode, Json<TransacaoOut>), (StatusCode, String)> {
    // ========== VALIDAÇÃO DO CLIENTE ==========
    // Converte e valida o ID do cliente (1-5 conforme especificação da Rinha)
    let cliente_id_num: i64 = match cliente_id.parse() {
        Ok(id) if id >= 1 && id <= 5 => id,
        _ => return Err((StatusCode::NOT_FOUND, "cliente not found".into())),
    };

    // ========== VALIDAÇÕES DA RINHA ==========
    // Validações rigorosas conforme especificação do desafio
    if body.descricao.is_empty() || body.descricao.len() > 10 {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "invalid descricao".into()));
    }

    if body.tipo != "c" && body.tipo != "d" {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "invalid tipo".into()));
    }

    if body.valor <= 0 {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "invalid valor".into()));
    }

    // ========== DEFINIÇÃO DE LIMITES ==========
    // Limites pré-definidos por cliente (conforme especificação da Rinha)
    let limite = match cliente_id_num {
        1 => 100000,   // Cliente 1: R$ 1000,00
        2 => 80000,    // Cliente 2: R$ 800,00
        3 => 1000000,  // Cliente 3: R$ 10000,00
        4 => 10000000, // Cliente 4: R$ 100000,00
        5 => 500000,   // Cliente 5: R$ 5000,00
        _ => 0,
    };

    // ========== SIMULAÇÃO DE SALDO ==========
    // Em produção, isso viria do banco de dados
    // Para a Rinha, mantemos em memória por simplicidade
    let mut saldo = 0;

    // ========== APLICAÇÃO DA TRANSAÇÃO ==========
    if body.tipo == "d" {
        // Débito: subtrai do saldo
        saldo -= body.valor;
        // Verifica se não ultrapassa o limite
        if saldo < -limite {
            return Err((StatusCode::UNPROCESSABLE_ENTITY, "limite exceeded".into()));
        }
    } else {
        // Crédito: adiciona ao saldo
        saldo += body.valor;
    }

    // ========== INTEGRAÇÃO COM UPSTREAM ==========
    // Usa o mesmo mecanismo de load balancing do pay()
    // Gera correlationId único para rastreamento
    let correlation_id = uuid::Uuid::new_v4().to_string();
    let req_body = serde_json::json!({
        "correlationId": correlation_id,
        "amount": body.valor as f64,
        "requestedAt": chrono::Utc::now().to_rfc3339()
    });

    // ========== SELEÇÃO DE PROCESSADOR ==========
    // Mesmo algoritmo de escolha primário/secundário
    let (prim, sec, prim_brk) = if st.strategy.pick_a_first(&st.breaker_a, &st.breaker_b) {
        (&st.up_a, &st.up_b, &st.breaker_a)
    } else {
        (&st.up_b, &st.up_a, &st.breaker_b)
    };

    // ========== HEDGING COM TOKIO::SELECT ==========
    // Implementação mais sofisticada usando tokio::select para concorrência real
    let result = if prim_brk.is_open() {
        // Circuit breaker aberto - vai direto pro secundário
        st.strategy.note_skip_primary();
        sec.clone()
            .request(Arc::clone(&st.cfg), req_body.clone())
            .await
    } else {
        // ========== CONCORRÊNCIA REAL ==========
        // Spawna tarefa para o primary
        let cfg_clone = Arc::clone(&st.cfg);
        let req_body_clone = req_body.clone();
        let prim_clone = prim.clone();
        let mut p_handle =
            tokio::spawn(async move { prim_clone.request(cfg_clone, req_body_clone).await });

        // Usa tokio::select para implementar hedging real
        let res = tokio::select! {
            // Se primary responder primeiro, usa o resultado
            res = &mut p_handle => res,
            // Se passar o delay, inicia secondary paralelamente
            _ = tokio::time::sleep(Duration::from_millis(st.cfg.hedge_delay_ms)) => {
                let cfg_clone2 = Arc::clone(&st.cfg);
                let req_body_clone2 = req_body.clone();
                let sec_clone = sec.clone();
                let mut s_handle = tokio::spawn(async move { sec_clone.request(cfg_clone2, req_body_clone2).await });
                // Agora espera o primeiro que responder (primary ou secondary)
                tokio::select! {
                    res = &mut s_handle => res,
                    res = &mut p_handle => res,
                }
            }
        };

        // Trata panics das tarefas
        match res {
            Ok(r) => r,
            Err(_) => Err((
                "unknown".into(),
                StatusCode::INTERNAL_SERVER_ERROR,
                "task panicked".into(),
            )),
        }
    };

    // ========== PROCESSAMENTO DO RESULTADO ==========
    match result {
        Ok((proc_name, _echo)) => {
            // ========== SUCESSO ==========
            // Atualiza estatísticas globais
            {
                let mut stats = st.stats.lock().unwrap();
                if proc_name == "A" {
                    stats.default.total_requests += 1;
                    stats.default.total_amount += body.valor as f64;
                    st.breaker_a.on_success();
                } else {
                    stats.fallback.total_requests += 1;
                    stats.fallback.total_amount += body.valor as f64;
                    st.breaker_b.on_success();
                }
            }

            // Registra métrica de sucesso
            metrics::counter!("transacoes_ok").increment(1);

            Ok((StatusCode::OK, Json(TransacaoOut { limite, saldo })))
        }
        Err((proc_name, code, msg)) => {
            // ========== ERRO ==========
            // Notifica circuit breaker sobre falha
            if proc_name == "A" {
                st.breaker_a.on_failure();
            } else {
                st.breaker_b.on_failure();
            }

            // Registra métrica de erro
            metrics::counter!("transacoes_err", "code" => code.as_u16().to_string()).increment(1);

            Err((code, msg))
        }
    }
}

/// Handler para consulta de estatísticas de pagamentos
/// Retorna métricas agregadas de processamento por processador
async fn payments_summary(
    State(st): State<AppState>,          // Estado global da aplicação
    _query: Query<PaymentsSummaryQuery>, // Parâmetros de query (não utilizados)
) -> Result<Json<PaymentSummary>, (StatusCode, String)> {
    // ========== ACESSO ÀS ESTATÍSTICAS ==========
    // Bloqueia o mutex para acesso thread-safe às estatísticas globais
    let stats = st.stats.lock().unwrap();

    // ========== RETORNO DAS MÉTRICAS ==========
    // Retorna estatísticas separadas para processador primário e secundário
    Ok(Json(PaymentSummary {
        default: ProcessorSummary {
            // Processador A (primário)
            total_requests: stats.default.total_requests,
            total_amount: stats.default.total_amount,
        },
        fallback: ProcessorSummary {
            // Processador B (secundário)
            total_requests: stats.fallback.total_requests,
            total_amount: stats.fallback.total_amount,
        },
    }))
}

/// Handler para limpeza/reset das estatísticas de pagamentos
/// Zera todos os contadores de requisições e valores processados
async fn purge_payments(
    State(st): State<AppState>, // Estado global da aplicação
) -> Result<StatusCode, (StatusCode, String)> {
    // ========== RESET DAS ESTATÍSTICAS ==========
    // Bloqueia o mutex e zera todos os contadores
    let mut stats = st.stats.lock().unwrap();
    stats.default.total_requests = 0; // Zera contador do processador A
    stats.default.total_amount = 0.0; // Zera valor total do processador A
    stats.fallback.total_requests = 0; // Zera contador do processador B
    stats.fallback.total_amount = 0.0; // Zera valor total do processador B

    // ========== CONFIRMAÇÃO DE SUCESSO ==========
    // Retorna 200 OK indicando que o reset foi realizado
    Ok(StatusCode::OK)
}
