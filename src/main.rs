use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    routing::{get, post},
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tower::{ServiceBuilder, limit::ConcurrencyLimitLayer, timeout::TimeoutLayer};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::{Level, info};

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
}

#[derive(Deserialize)]
struct PayIn {
    idempotency_key: String,
    amount: i64,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    metadata: serde_json::Value,
}

#[derive(Serialize)]
struct PayOut {
    processor: String,
    status: String,
    latency_ms: u64,
    echo: serde_json::Value,
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
    };

    // router
    let prom_handle_route = prom_handle.clone();
    let app = Router::new()
        .route("/payments", post(pay))
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(|| async { "ready" }))
        .route(
            "/metrics",
            get(move || {
                let h = prom_handle_route.clone();
                async move { h.render() }
            }),
        )
        .with_state(state.clone())
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(PropagateRequestIdLayer::x_request_id())
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(ConcurrencyLimitLayer::new(state.cfg.concurrency_limit))
                .layer(TimeoutLayer::new(Duration::from_millis(
                    state.cfg.request_timeout_ms,
                ))),
        );

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
    let key = format!("idemp:{}", body.idempotency_key);
    if st.idem.get(&key).is_some() {
        return Err((
            StatusCode::CONFLICT,
            "duplicate idempotency_key (ttl)".into(),
        ));
    }

    // seleção prim/sec com breaker
    let (prim, sec, prim_brk, sec_brk) = if st.strategy.pick_a_first(&st.breaker_a, &st.breaker_b) {
        (&st.up_a, &st.up_b, &st.breaker_a, &st.breaker_b)
    } else {
        (&st.up_b, &st.up_a, &st.breaker_b, &st.breaker_a)
    };

    // payload para upstream
    let req_body = serde_json::json!({
        "amount": body.amount,
        "currency": body.currency.as_deref().unwrap_or("BRL"),
        "metadata": body.metadata,
        "idempotency_key": body.idempotency_key,
    });

    let start = std::time::Instant::now();

    // Hedge simples: dispara sec depois de um delay se prim não retornou
    let hedge_delay = Duration::from_millis(st.cfg.hedge_delay_ms);

    let result = if prim_brk.is_open() {
        st.strategy.note_skip_primary();
        sec.request(&st.cfg, &req_body).await
    } else {
        let p = prim.request(&st.cfg, &req_body);
        tokio::select! {
            res = p => res,
            _ = tokio::time::sleep(hedge_delay) => {
                if !prim_brk.is_open() {
                    let s = sec.request(&st.cfg, &req_body);
                    tokio::select! {
                        r1 = s => r1,
                        r2 = p => r2,
                    }
                } else {
                    sec.request(&st.cfg, &req_body).await
                }
            }
        }
    };

    let elapsed = start.elapsed().as_millis() as u64;
    metrics::histogram!("payments_latency_ms", elapsed as f64);

    match result {
        Ok((proc_name, echo)) => {
            st.idem.insert(key, ());
            metrics::increment_counter!("payments_ok");
            Ok((
                StatusCode::OK,
                Json(PayOut {
                    processor: proc_name,
                    status: "approved".into(),
                    latency_ms: elapsed,
                    echo,
                }),
            ))
        }
        Err((proc_name, code, msg)) => {
            if proc_name == "A" {
                st.breaker_a.on_failure();
            } else {
                st.breaker_b.on_failure();
            }
            metrics::increment_counter!("payments_err", "code" => code.as_str());
            Err((code, msg))
        }
    }
}
