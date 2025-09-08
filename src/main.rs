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

mod breaker;
mod config;
mod strategy;
mod upstream;

use breaker::Breaker;
use config::Cfg;
use moka::sync::Cache;
use strategy::RouteStrategy;
use upstream::UpstreamClient;

#[derive(Clone)]
struct AppState {
    cfg: Arc<Cfg>,
    up_a: Arc<UpstreamClient>,
    up_b: Arc<UpstreamClient>,
    breaker_a: Arc<Breaker>,
    breaker_b: Arc<Breaker>,
    strategy: Arc<RouteStrategy>,
    idem: Cache<String, ()>,
    stats: Arc<Mutex<PaymentStats>>,
}

#[derive(Default)]
struct PaymentStats {
    default: ProcessorStats,
    fallback: ProcessorStats,
}

#[derive(Default)]
struct ProcessorStats {
    total_requests: u64,
    total_amount: f64,
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

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    // tracing enxuto
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("p99=info".parse().unwrap())
                .add_directive("axum=warn".parse().unwrap())
                .add_directive("tower_http=warn".parse().unwrap()),
        )
        .with_target(false)
        .compact()
        .init();

    // metrics (metrics 0.24 + prometheus 0.17)
    let prom_handle: PrometheusHandle = PrometheusBuilder::new()
        .install_recorder()
        .expect("install recorder");

    // config/env
    let cfg = Arc::new(Cfg::from_env()?);
    info!("cfg: {:?}", cfg.redacted());

    // upstreams
    let up_a = Arc::new(UpstreamClient::new("A".into(), &cfg).await?);
    let up_b = Arc::new(UpstreamClient::new("B".into(), &cfg).await?);

    // circuit-breakers
    let breaker_a = Arc::new(Breaker::new(
        cfg.cb_min_samples,
        cfg.cb_fail_rate,
        Duration::from_secs(cfg.cb_open_secs),
    ));
    let breaker_b = Arc::new(Breaker::new(
        cfg.cb_min_samples,
        cfg.cb_fail_rate,
        Duration::from_secs(cfg.cb_open_secs),
    ));

    // estratégia de roteamento
    let strategy = Arc::new(RouteStrategy::new());

    // cache idempotente TTL curto
    let idem = Cache::builder()
        .max_capacity(1_000_000)
        .time_to_live(Duration::from_secs(60))
        .build();

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

    // router
    let prom_handle_route = prom_handle.clone();
    let app = Router::new()
        .route("/payments", post(pay))
        .route("/payments-summary", get(payments_summary))
        .route("/purge-payments", post(purge_payments))
        .route("/clientes/{id}/transacoes", post(transacao))
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(|| async { "ready" }))
        .route(
            "/metrics",
            get(move || {
                let h = prom_handle_route.clone();
                async move { h.render() }
            }),
        )
        .with_state(state.clone());

    // axum 0.8 -> TcpListener + axum::serve
    let addr: SocketAddr = format!("0.0.0.0:{}", state.cfg.port).parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!("listening on {}", addr);
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

async fn pay(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PayIn>,
) -> Result<(StatusCode, Json<PayOut>), (StatusCode, String)> {
    // auth por header (compatível com rinha-test)
    if let Some(required) = st.cfg.auth_header_value.clone() {
        let name = st
            .cfg
            .auth_header_name
            .as_deref()
            .unwrap_or("Authorization");
        match headers.get(name) {
            Some(v) if v == HeaderValue::from_str(&required).unwrap() => {}
            _ => return Err((StatusCode::UNAUTHORIZED, "unauthorized".into())),
        }
    }

    // idempotência básica
    let key = format!("idemp:{}", body.correlation_id);
    if st.idem.get(&key).is_some() {
        return Err((
            StatusCode::CONFLICT,
            "duplicate correlation_id (ttl)".into(),
        ));
    }

    // seleção prim/sec com breaker
    let (prim, sec, prim_brk) = if st.strategy.pick_a_first(&st.breaker_a, &st.breaker_b) {
        (&st.up_a, &st.up_b, &st.breaker_a)
    } else {
        (&st.up_b, &st.up_a, &st.breaker_b)
    };

    // payload para upstream (formato da rinha)
    let now = std::time::SystemTime::now();
    let requested_at = chrono::DateTime::<chrono::Utc>::from(now).to_rfc3339();
    let req_body = serde_json::json!({
        "correlationId": uuid::Uuid::new_v4().to_string(),
        "amount": body.amount,
        "requestedAt": requested_at,
    });

    let start = std::time::Instant::now();

    // Hedge simples: dispara sec depois de um delay se prim não retornou
    let hedge_delay = Duration::from_millis(st.cfg.hedge_delay_ms);

    let result = if prim_brk.is_open() {
        st.strategy.note_skip_primary();
        sec.clone()
            .request(Arc::clone(&st.cfg), req_body.clone())
            .await
    } else {
        let cfg_clone = Arc::clone(&st.cfg);
        let req_body_clone = req_body.clone();
        let prim_clone = prim.clone();
        let mut p_handle =
            tokio::spawn(async move { prim_clone.request(cfg_clone, req_body_clone).await });
        let res = tokio::select! {
            res = &mut p_handle => res,
            _ = tokio::time::sleep(hedge_delay) => {
                let cfg_clone2 = Arc::clone(&st.cfg);
                let req_body_clone2 = req_body.clone();
                let sec_clone = sec.clone();
                let mut s_handle = tokio::spawn(async move { sec_clone.request(cfg_clone2, req_body_clone2).await });
                tokio::select! {
                    res = &mut s_handle => res,
                    res = &mut p_handle => res,
                }
            }
        };
        match res {
            Ok(r) => r,
            Err(_) => Err((
                "unknown".into(),
                StatusCode::INTERNAL_SERVER_ERROR,
                "task panicked".into(),
            )),
        }
    };

    let elapsed = start.elapsed().as_millis() as u64;
    metrics::histogram!("payments_latency_ms").record(elapsed as f64);

    match result {
        Ok((proc_name, _echo)) => {
            st.idem.insert(key.clone(), ());

            // Atualizar estatísticas
            {
                let mut stats = st.stats.lock().unwrap();
                if proc_name == "A" {
                    stats.default.total_requests += 1;
                    stats.default.total_amount += body.amount;
                    st.breaker_a.on_success();
                } else {
                    stats.fallback.total_requests += 1;
                    stats.fallback.total_amount += body.amount;
                    st.breaker_b.on_success();
                }
            }

            metrics::counter!("payments_ok").increment(1);
            Ok((
                StatusCode::OK,
                Json(PayOut {
                    message: "payment processed successfully".into(),
                }),
            ))
        }
        Err((proc_name, code, msg)) => {
            if proc_name == "A" {
                st.breaker_a.on_failure();
            } else {
                st.breaker_b.on_failure();
            }
            metrics::counter!("payments_err", "code" => code.as_u16().to_string()).increment(1);
            Err((code, msg))
        }
    }
}

async fn transacao(
    State(st): State<AppState>,
    Path(cliente_id): Path<String>,
    Json(body): Json<TransacaoIn>,
) -> Result<(StatusCode, Json<TransacaoOut>), (StatusCode, String)> {
    // Validação básica do cliente (simplificada para Rinha)
    let cliente_id_num: i64 = match cliente_id.parse() {
        Ok(id) if id >= 1 && id <= 5 => id,
        _ => return Err((StatusCode::NOT_FOUND, "cliente not found".into())),
    };

    // Validações da Rinha
    if body.descricao.is_empty() || body.descricao.len() > 10 {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "invalid descricao".into()));
    }

    if body.tipo != "c" && body.tipo != "d" {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "invalid tipo".into()));
    }

    if body.valor <= 0 {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "invalid valor".into()));
    }

    // Simulação de limite e saldo (valores fixos por cliente para simplificar)
    let limite = match cliente_id_num {
        1 => 100000,
        2 => 80000,
        3 => 1000000,
        4 => 10000000,
        5 => 500000,
        _ => 0,
    };

    let mut saldo = 0; // Em um sistema real, isso viria do banco de dados

    // Aplicar transação
    if body.tipo == "d" {
        saldo -= body.valor;
        if saldo < -limite {
            return Err((StatusCode::UNPROCESSABLE_ENTITY, "limite exceeded".into()));
        }
    } else {
        saldo += body.valor;
    }

    // Aqui deveria salvar no banco de dados, mas para a Rinha vamos simular
    // e usar o upstream mock para processamento

    // Chamar upstream para processamento (usando o mesmo mecanismo)
    let correlation_id = uuid::Uuid::new_v4().to_string();

    let req_body = serde_json::json!({
        "correlationId": correlation_id,
        "amount": body.valor as f64,
        "requestedAt": chrono::Utc::now().to_rfc3339()
    }); // Usar o mesmo mecanismo de upstream da função pay
    let (prim, sec, prim_brk) = if st.strategy.pick_a_first(&st.breaker_a, &st.breaker_b) {
        (&st.up_a, &st.up_b, &st.breaker_a)
    } else {
        (&st.up_b, &st.up_a, &st.breaker_b)
    };

    let result = if prim_brk.is_open() {
        st.strategy.note_skip_primary();
        sec.clone()
            .request(Arc::clone(&st.cfg), req_body.clone())
            .await
    } else {
        let cfg_clone = Arc::clone(&st.cfg);
        let req_body_clone = req_body.clone();
        let prim_clone = prim.clone();
        let mut p_handle =
            tokio::spawn(async move { prim_clone.request(cfg_clone, req_body_clone).await });
        let res = tokio::select! {
            res = &mut p_handle => res,
            _ = tokio::time::sleep(Duration::from_millis(st.cfg.hedge_delay_ms)) => {
                let cfg_clone2 = Arc::clone(&st.cfg);
                let req_body_clone2 = req_body.clone();
                let sec_clone = sec.clone();
                let mut s_handle = tokio::spawn(async move { sec_clone.request(cfg_clone2, req_body_clone2).await });
                tokio::select! {
                    res = &mut s_handle => res,
                    res = &mut p_handle => res,
                }
            }
        };
        match res {
            Ok(r) => r,
            Err(_) => Err((
                "unknown".into(),
                StatusCode::INTERNAL_SERVER_ERROR,
                "task panicked".into(),
            )),
        }
    };

    match result {
        Ok((proc_name, _echo)) => {
            // Atualizar estatísticas
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

            metrics::counter!("transacoes_ok").increment(1);
            Ok((StatusCode::OK, Json(TransacaoOut { limite, saldo })))
        }
        Err((proc_name, code, msg)) => {
            if proc_name == "A" {
                st.breaker_a.on_failure();
            } else {
                st.breaker_b.on_failure();
            }
            metrics::counter!("transacoes_err", "code" => code.as_u16().to_string()).increment(1);
            Err((code, msg))
        }
    }
}

async fn payments_summary(
    State(st): State<AppState>,
    _query: Query<PaymentsSummaryQuery>,
) -> Result<Json<PaymentSummary>, (StatusCode, String)> {
    let stats = st.stats.lock().unwrap();
    Ok(Json(PaymentSummary {
        default: ProcessorSummary {
            total_requests: stats.default.total_requests,
            total_amount: stats.default.total_amount,
        },
        fallback: ProcessorSummary {
            total_requests: stats.fallback.total_requests,
            total_amount: stats.fallback.total_amount,
        },
    }))
}

async fn purge_payments(State(st): State<AppState>) -> Result<StatusCode, (StatusCode, String)> {
    let mut stats = st.stats.lock().unwrap();
    stats.default.total_requests = 0;
    stats.default.total_amount = 0.0;
    stats.fallback.total_requests = 0;
    stats.fallback.total_amount = 0.0;
    Ok(StatusCode::OK)
}
