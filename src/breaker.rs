/// Implementação de Circuit Breaker para proteção contra falhas em cascata
/// Baseado no padrão Circuit Breaker do Martin Fowler
/// Previne chamadas para serviços que estão falhando repetidamente
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

/// Estrutura principal do Circuit Breaker
/// Mantém estado atômico para operações thread-safe sem locks
pub struct Breaker {
    /// Número mínimo de amostras necessário para calcular taxa de falha
    min_samples: usize,
    /// Taxa de falha limite para abrir o circuito (ex: 0.5 = 50%)
    fail_rate: f64,
    /// Tempo que o circuito fica aberto antes de tentar half-open
    open_for: Duration,
    /// Contador atômico de falhas na janela atual
    fails: AtomicUsize,
    /// Contador atômico total de requisições na janela atual
    total: AtomicUsize,
    /// Timestamp (ms) quando o circuito foi aberto (0 = fechado)
    opened_at_ms: AtomicU64,
}

impl Breaker {
    /// Cria novo Circuit Breaker com parâmetros configuráveis
    /// # Arguments
    /// * `min_samples` - Mínimo de requisições para calcular taxa de falha
    /// * `fail_rate` - Taxa de falha limite (0.0-1.0)
    /// * `open_for` - Duração que o circuito fica aberto
    pub fn new(min_samples: usize, fail_rate: f64, open_for: Duration) -> Self {
        Self {
            min_samples,
            fail_rate,
            open_for,
            fails: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            opened_at_ms: AtomicU64::new(0),
        }
    }

    /// Registra uma falha no circuit breaker
    /// Incrementa contadores e recalcula se deve abrir o circuito
    pub fn on_failure(&self) {
        self.fails.fetch_add(1, Ordering::Relaxed);
        self.total.fetch_add(1, Ordering::Relaxed);
        self.recalc();
    }

    /// Registra um sucesso no circuit breaker
    /// Apenas incrementa total, não afeta contador de falhas
    #[allow(dead_code)]
    pub fn on_success(&self) {
        self.total.fetch_add(1, Ordering::Relaxed);
        self.recalc();
    }

    /// Recalcula o estado do circuit breaker baseado nos contadores atuais
    /// Chamado após cada sucesso ou falha para verificar se deve abrir o circuito
    fn recalc(&self) {
        let t = self.total.load(Ordering::Relaxed);

        // ========== VERIFICAÇÃO DE AMOSTRAS ==========
        // Só calcula taxa de falha se temos amostras suficientes
        if t < self.min_samples {
            return;
        }

        // ========== CÁLCULO DA TAXA DE FALHA ==========
        let f = self.fails.load(Ordering::Relaxed);
        let rate = f as f64 / t as f64;

        // ========== DECISÃO DE ABERTURA ==========
        // Se taxa de falha >= limite configurado, abre o circuito
        if rate >= self.fail_rate {
            // Registra timestamp de abertura
            self.opened_at_ms.store(now_ms(), Ordering::Relaxed);

            // ========== RESET DA JANELA ==========
            // Zera contadores para próxima janela quando circuito reabrir
            self.fails.store(0, Ordering::Relaxed);
            self.total.store(0, Ordering::Relaxed);
        }
    }

    /// Verifica se o circuito está aberto (bloqueando requisições)
    /// # Returns
    /// * `true` se circuito está aberto (bloquear requisição)
    /// * `false` se circuito está fechado ou half-open (permitir requisição)
    pub fn is_open(&self) -> bool {
        let opened = self.opened_at_ms.load(Ordering::Relaxed);

        // ========== CIRCUITO FECHADO ==========
        // Se nunca foi aberto, permite passagem
        if opened == 0 {
            return false;
        }

        // ========== VERIFICAÇÃO DE TIMEOUT ==========
        // Calcula tempo decorrido desde abertura
        let elapsed_ms = now_ms().saturating_sub(opened);
        let open_duration_ms = self.open_for.as_millis() as u64;

        // Circuito ainda aberto se não passou tempo suficiente
        elapsed_ms < open_duration_ms
    }
}

/// Função utilitária para obter timestamp atual em milissegundos
/// Usada para controlar timeouts do circuit breaker
fn now_ms() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
