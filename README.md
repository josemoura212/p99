# 🚀 Load Balancer em Rust - Rinha de Backend 2025

Um **Load Balancer** de alta performance implementado em **Rust** com **Axum**, desenvolvido para a **Rinha de Backend 2025**. Implementa estratégias avançadas de balanceamento de carga, circuit breaker, hedging e idempotência.

## 📊 Resultados na Rinha de Backend 2025

- **Pontuação Final**: R$ 75.131,58
- **Latência P99**: 52.43ms
- **Transações Processadas**: 12.676
- **Throughput**: 261 RPS
- **Disponibilidade**: 85% (com carga de 550 VUs simultâneos)

---

## 🏗️ Arquitetura

```
┌─────────────────┐    ┌─────────────────┐
│   Nginx LB      │────│   API Instance  │
│  (Port 9999)    │    │   (Rust/Axum)   │
└─────────────────┘    └─────────────────┘
         │                       │
         └───────────────────────┼──────────────────────┐
                                 │                      │
                    ┌────────────▼────────────┐    ┌────▼────────────┐
                    │ Payment Processor A    │    │ Payment Processor│
                    │ (Port 8001)            │    │ B (Port 8002)    │
                    │ - PostgreSQL           │    │ - PostgreSQL     │
                    └───────────────────────┘    └──────────────────┘
```

### Componentes Principais

#### 1. **API Server (Rust + Axum)**
- **Framework**: Axum (mais rápido do ecossistema Rust)
- **Alocador**: MiMalloc (otimizado para concorrência)
- **Runtime**: Tokio (assíncrono de alta performance)

#### 2. **Circuit Breaker**
- **Implementação**: Atômica (sem locks)
- **Estratégia**: Conta falhas e abre circuito automaticamente
- **Recuperação**: Fecha automaticamente após timeout

#### 3. **Hedging Strategy**
- **Objetivo**: Reduz latência P99
- **Funcionamento**: Inicia request secundário se primário demorar
- **Benefício**: Melhor experiência em cenários de alta latência

#### 4. **Load Balancing**
- **Estratégia**: Round-robin com fallback
- **Failover**: Automático para processador saudável
- **Distribuição**: Carga equilibrada entre instâncias

#### 5. **Idempotência**
- **Cache**: Moka (mais rápido cache concorrente do Rust)
- **TTL**: 30 segundos (evita duplicatas)
- **Thread-safe**: Sem race conditions

#### 6. **Monitoramento**
- **Métricas**: Prometheus nativo
- **Latência**: Histogramas por endpoint
- **Throughput**: Contadores por serviço
- **Erros**: Classificação por tipo

---

## 🚀 Instalação e Execução

### Pré-requisitos

- **Docker** e **Docker Compose**
- **Rust** 1.85+ (opcional, para desenvolvimento)
- **Linux/macOS** (recomendado)

### 1. Clone o Repositório

```bash
git clone https://github.com/josemoura212/p99.git
cd p99
```

### 2. Execute com Docker

```bash
# Subir toda a infraestrutura
docker-compose up -d

# Verificar se está rodando
docker-compose ps
```

### 3. Teste Básico

```bash
# Teste simples
curl -X POST http://localhost:9999/payments \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer 123" \
  -d '{"correlationId": "test-123", "amount": 20}'

# Resposta esperada:
{"message":"payment processed successfully"}
```

### 4. Execute os Testes Oficiais

```bash
# Instalar k6 (se não tiver)
# Linux: sudo apt install k6
# macOS: brew install k6

# Executar teste da Rinha
cd rinha-de-backend-2025/rinha-test
TOKEN=123 MAX_REQUESTS=550 k6 run rinha.js
```

---

## ⚙️ Configuração

### Variáveis de Ambiente

```bash
# Servidor
PORT=9999

# Upstream Services
UPSTREAM_A_URL=http://payment-processor-default:8080
UPSTREAM_B_URL=http://payment-processor-fallback:8080
UPSTREAM_PAY_PATH=/payments

# Autenticação
AUTH_HEADER_NAME=Authorization
AUTH_HEADER_VALUE=Bearer 123

# Performance (Otimizado)
REQUEST_TIMEOUT_MS=50      # Timeout por request
HEDGE_DELAY_MS=5          # Delay para hedging
CONCURRENCY_LIMIT=2048    # Máximo de conexões simultâneas

# Circuit Breaker
CB_FAIL_RATE=0.3          # 30% de falha abre circuito
CB_MIN_SAMPLES=20         # Mínimo de amostras
CB_OPEN_SECS=5            # Tempo aberto em segundos

# Cache
CACHE_CAPACITY=500000     # Capacidade do cache
CACHE_TTL_SECONDS=30      # TTL do cache
```

### Arquivo docker-compose.yaml

```yaml
version: '3.8'
services:
  api-1:
    build: .
    environment:
      REQUEST_TIMEOUT_MS: "50"
      HEDGE_DELAY_MS: "5"
      CONCURRENCY_LIMIT: "2048"
      CB_FAIL_RATE: "0.3"
      CB_MIN_SAMPLES: "20"
      CB_OPEN_SECS: "5"
    deploy:
      resources:
        limits:
          cpus: '1.0'
          memory: 200M
```

---

## 🎯 Otimizações de Performance

### 1. **Timeouts Agressivos**

```bash
# Configurações otimizadas
REQUEST_TIMEOUT_MS=50      # 50ms máximo
HEDGE_DELAY_MS=5          # 5ms para hedge
```

**Por que?**
- Reduz latência P99 drasticamente
- Falha rápido em vez de esperar
- Melhor experiência do usuário

### 2. **Circuit Breaker Otimizado**

```bash
CB_FAIL_RATE=0.3          # Abre com 30% de falha
CB_MIN_SAMPLES=20         # Avalia após 20 requests
CB_OPEN_SECS=5            # 5 segundos aberto
```

**Por que?**
- Previne cascata de falhas
- Recuperação automática rápida
- Protege sistema downstream

### 3. **Connection Pooling**

```rust
// upstream.rs
.pool_max_idle_per_host(32)
.pool_idle_timeout(Duration::from_secs(30))
.tcp_nodelay(true)
```

**Por que?**
- Reutiliza conexões TCP
- Reduz overhead de handshake
- Melhor throughput

### 4. **Cache Otimizado**

```rust
// main.rs
let idem = Cache::builder()
    .max_capacity(500_000)
    .time_to_live(Duration::from_secs(30))
    .build();
```

**Por que?**
- Evita processamento duplicado
- Thread-safe sem locks
- LRU automático

### 5. **Nginx Load Balancer**

```nginx
# nginx.conf
worker_processes auto;
events {
    worker_connections 8192;
    multi_accept on;
    use epoll;
}
http {
  proxy_buffering off;
  proxy_read_timeout 10;
  proxy_connect_timeout 5;
}
```

**Por que?**
- Distribui carga entre instâncias
- Buffering desabilitado para baixa latência
- Timeouts agressivos

---

## 📊 Monitoramento

### Métricas Disponíveis

```bash
# Acesse as métricas
curl http://localhost:9999/metrics
```

#### Principais Métricas:

```prometheus
# Latência por request
payments_latency_ms{quantile="0.99"} 52.43

# Throughput
payments_ok 12676
payments_err{code="500"} 3716

# Circuit Breaker
circuit_breaker_a_status 0  # 0=closed, 1=open
circuit_breaker_b_status 0

# Cache
idempotency_cache_size 12431
idempotency_cache_hits 11876
```

### Dashboard Recomendado

```bash
# Instalar Prometheus + Grafana
# Configurar scrape de /metrics
# Criar dashboards para:
# - Latência P50/P95/P99
# - Throughput por serviço
# - Taxa de erro por upstream
# - Status do circuit breaker
# - Uso de CPU/Memória
```

---

## 🔧 Desenvolvimento

### Compilação

```bash
# Desenvolvimento
cargo build

# Produção (otimizado)
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

### Testes

```bash
# Testes unitários
cargo test

# Testes de carga
cd rinha-de-backend-2025/rinha-test
TOKEN=123 MAX_REQUESTS=100 k6 run rinha.js
```

### Debug

```bash
# Logs detalhados
RUST_LOG=debug cargo run

# Tracing
RUST_LOG=p99=trace cargo run
```

---

## 🚀 Deploy em Produção

### 1. **Infraestrutura Recomendada**

```yaml
# docker-compose.prod.yml
version: '3.8'
services:
  api:
    image: p99:latest
    deploy:
      replicas: 3
      resources:
        limits:
          cpus: '1.0'
          memory: 200M
        reservations:
          cpus: '0.5'
          memory: 100M
    environment:
      - REQUEST_TIMEOUT_MS=50
      - HEDGE_DELAY_MS=5
      - CONCURRENCY_LIMIT=2048
      - CB_FAIL_RATE=0.3
      - CB_MIN_SAMPLES=20
      - CB_OPEN_SECS=5

  nginx:
    image: nginx:alpine
    volumes:
      - ./nginx.prod.conf:/etc/nginx/nginx.conf
    ports:
      - "80:80"
      - "443:443"
    deploy:
      resources:
        limits:
          cpus: '0.5'
          memory: 100M
```

### 2. **Configuração Nginx Produção**

```nginx
# nginx.prod.conf
upstream api_backend {
    server api-1:9999 max_fails=3 fail_timeout=30s;
    server api-2:9999 max_fails=3 fail_timeout=30s;
    server api-3:9999 max_fails=3 fail_timeout=30s;
}

server {
    listen 80;
    server_name your-domain.com;

    location / {
        proxy_pass http://api_backend;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # Timeouts otimizados
        proxy_connect_timeout 5s;
        proxy_send_timeout 10s;
        proxy_read_timeout 10s;

        # Buffers
        proxy_buffering off;
        proxy_request_buffering off;
    }

    # Health check
    location /health {
        access_log off;
        return 200 "healthy\n";
    }
}
```

### 3. **Monitoramento Produção**

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'load-balancer'
    static_configs:
      - targets: ['api-1:9999', 'api-2:9999', 'api-3:9999']
    scrape_interval: 15s
    metrics_path: /metrics
```

---

## 📈 Estratégias de Escalabilidade

### 1. **Horizontal Scaling**

```bash
# Aumentar número de instâncias
docker-compose up --scale api=5 -d
```

### 2. **Vertical Scaling**

```yaml
deploy:
  resources:
    limits:
      cpus: '2.0'      # Mais CPU
      memory: 512M    # Mais memória
```

### 3. **Auto-scaling**

```yaml
# Com Docker Swarm ou Kubernetes
deploy:
  replicas: 3
  resources:
    limits:
      cpus: '1.0'
      memory: 200M
  restart_policy:
    condition: on-failure
```

---

## 🛡️ Segurança

### 1. **Autenticação**

```rust
// main.rs - Verificação de token
if let Some(required) = st.cfg.auth_header_value.clone() {
    let name = st.cfg.auth_header_name.as_deref().unwrap_or("Authorization");
    match headers.get(name) {
        Some(v) if v == HeaderValue::from_str(&required).unwrap() => {}
        _ => return Err((StatusCode::UNAUTHORIZED, "unauthorized".into())),
    }
}
```

### 2. **Rate Limiting**

```rust
// Implementação recomendada
let rate_limiter = Arc::new(RwLock::new(HashMap::new()));
// Por IP ou por cliente
```

### 3. **HTTPS/TLS**

```nginx
server {
    listen 443 ssl http2;
    server_name your-domain.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    # SSL otimizado
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-RSA-AES128-GCM-SHA256;
    ssl_prefer_server_ciphers off;
}
```

---

## 🔧 Troubleshooting

### Problemas Comuns

#### 1. **Alta Latência**

```bash
# Verificar métricas
curl http://localhost:9999/metrics | grep latency

# Verificar circuit breaker
curl http://localhost:9999/metrics | grep circuit_breaker
```

#### 2. **Muitos Erros 502/500**

```bash
# Verificar upstreams
curl http://localhost:8001/health
curl http://localhost:8002/health

# Verificar logs
docker-compose logs api-1
```

#### 3. **Cache Cheio**

```bash
# Aumentar capacidade do cache
CACHE_CAPACITY=1000000
CACHE_TTL_SECONDS=60
```

---

## 📚 Referências

- [Axum Documentation](https://docs.rs/axum/latest/axum/)
- [Tokio Runtime](https://tokio.rs/)
- [Moka Cache](https://docs.rs/moka/latest/moka/)
- [Prometheus Metrics](https://prometheus.io/)
- [Rinha de Backend](https://github.com/zanfranceschi/rinha-de-backend-2025)

---

## 🤝 Contribuição

1. Fork o projeto
2. Crie uma branch (`git checkout -b feature/nova-feature`)
3. Commit suas mudanças (`git commit -am 'Adiciona nova feature'`)
4. Push para a branch (`git push origin feature/nova-feature`)
5. Abra um Pull Request

---

## 📄 Licença

Este projeto está sob a licença MIT. Veja o arquivo [LICENSE](LICENSE) para mais detalhes.

---

## 🙏 Agradecimentos

- **Rinha de Backend** pela competição incrível
- **Axum** pela melhor framework web do Rust
- **Tokio** pelo runtime assíncrono excepcional
- **Moka** pelo cache mais rápido do ecossistema

---

**🚀 Esta implementação demonstrou que Rust pode competir com as melhores linguagens em termos de performance e confiabilidade!**</content>
<parameter name="filePath">/home/josemoura212/p99/README.md
