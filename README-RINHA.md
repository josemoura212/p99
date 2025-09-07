# P99 - Load Balancer para Rinha de Backend 2025

Este projeto foi adaptado para participar da **Rinha de Backend 2025**, implementando um load balancer/proxy inteligente que intermedia pagamentos entre dois processadores (default e fallback).

## 🎯 Funcionalidades Implementadas

- **Load Balancing**: Distribuição de carga entre dois processadores de pagamento
- **Circuit Breaker**: Proteção contra falhas nos processadores
- **Hedging**: Estratégia de hedge para reduzir latência
- **Idempotency**: Controle de duplicatas via cache TTL
- **Health Monitoring**: Verificação de saúde dos processadores
- **Métricas**: Prometheus para monitoramento de performance
- **Auditoria**: Endpoint `/payments-summary` para consistência

## 🚀 Como Executar

### 1. Subir os Serviços
```bash
# Construir e subir todos os serviços
docker-compose up --build -d

# Verificar se estão rodando
docker-compose ps
```

### 2. Testar Endpoints
```bash
# Health check
curl http://localhost:9999/healthz

# Processar pagamento
curl -X POST http://localhost:9999/payments \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer 123" \
  -d '{"correlationId": "550e8400-e29b-41d4-a716-446655440000", "amount": 100.50}'

# Ver resumo de pagamentos
curl http://localhost:9999/payments-summary

# Ver métricas
curl http://localhost:9999/metrics
```

### 3. Executar Teste de Carga
```bash
# Instalar k6 (se não tiver)
sudo apt update && sudo apt install -y k6

# Executar teste
k6 run test-load.js
```

## 📋 Endpoints Disponíveis

### POST /payments
Processa um pagamento através dos processadores upstream.

**Request:**
```json
{
  "correlationId": "uuid",
  "amount": 100.50
}
```

**Response (200):**
```json
{
  "message": "payment processed successfully"
}
```

### GET /payments-summary
Retorna estatísticas de processamento para auditoria.

**Response (200):**
```json
{
  "default": {
    "total_requests": 150,
    "total_amount": 15000.00
  },
  "fallback": {
    "total_requests": 25,
    "total_amount": 2500.00
  }
}
```

## ⚙️ Configuração

As configurações são feitas via variáveis de ambiente:

| Variável | Descrição | Padrão |
|----------|-----------|---------|
| `UPSTREAM_A_URL` | URL do processador default | - |
| `UPSTREAM_B_URL` | URL do processador fallback | - |
| `REQUEST_TIMEOUT_MS` | Timeout das requisições | 120 |
| `HEDGE_DELAY_MS` | Delay para hedging | 40 |
| `CB_FAIL_RATE` | Taxa de falha para circuit breaker | 0.25 |
| `CB_MIN_SAMPLES` | Mínimo de amostras para CB | 50 |
| `CB_OPEN_SECS` | Tempo de abertura do CB | 2 |

## 🏗️ Arquitetura

```
Cliente → Nginx (LB) → API Instances → Payment Processors
                              ↓
                        Circuit Breaker
                              ↓
                    Health Check & Routing
```

## 📊 Estratégia de Roteamento

1. **Health Check**: Verifica se o processador está saudável
2. **Circuit Breaker**: Evita enviar para processadores com falha
3. **Taxas**: Prioriza processador com menor taxa (default)
4. **Hedging**: Dispara para ambos se demorar muito
5. **Fallback**: Usa processador alternativo se necessário

## 🎯 Pontuação na Rinha

- **Lucro**: Baseado em pagamentos processados com menor taxa
- **Performance**: Bônus por p99 < 11ms
- **Consistência**: Penalidade se houver diferenças na auditoria

## 🛠️ Desenvolvimento

### Compilar
```bash
cargo build --release
```

### Executar Local
```bash
# Com processadores locais
UPSTREAM_A_URL=http://localhost:8001 \
UPSTREAM_B_URL=http://localhost:8002 \
cargo run
```

## 📝 Notas Técnicas

- Usa **MiMalloc** para otimização de memória
- **HTTP/2** para melhor performance
- **Async/Await** com Tokio
- **Moka** para cache de idempotency
- **Prometheus** para métricas
- **Tracing** para logs

---

**Pronto para competir! 🚀**
