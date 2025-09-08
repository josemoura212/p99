/// Estratégia de roteamento inteligente para load balancer
/// Implementa balanceamento round-robin com awareness de circuit breaker
/// Prioriza processadores saudáveis e distribui carga uniformemente
use crate::breaker::Breaker;
use std::sync::atomic::{AtomicU64, Ordering};

/// Estrutura da estratégia de roteamento
/// Mantém contador atômico para distribuição uniforme de carga
pub struct RouteStrategy {
    /// Contador atômico para implementar round-robin
    /// Usado para alternar entre processadores A e B
    skew: AtomicU64,
}

impl RouteStrategy {
    /// Cria nova instância da estratégia de roteamento
    pub fn new() -> Self {
        Self {
            skew: AtomicU64::new(0),
        }
    }

    /// Decide qual processador usar primeiro (primário)
    /// Considera estado dos circuit breakers e implementa round-robin
    ///
    /// # Arguments
    /// * `a` - Circuit breaker do processador A
    /// * `b` - Circuit breaker do processador B
    ///
    /// # Returns
    /// * `true` se deve tentar A primeiro
    /// * `false` se deve tentar B primeiro
    pub fn pick_a_first(&self, a: &Breaker, b: &Breaker) -> bool {
        // ========== VERIFICAÇÃO DE CIRCUIT BREAKERS ==========
        // Se A está aberto mas B está fechado, usa B primeiro
        if a.is_open() && !b.is_open() {
            return false;
        }

        // Se B está aberto mas A está fechado, usa A primeiro
        if b.is_open() && !a.is_open() {
            return true;
        }

        // ========== ROUND-ROBIN ==========
        // Ambos circuit breakers fechados ou ambos abertos
        // Usa contador atômico para alternar uniformemente
        self.skew.fetch_add(1, Ordering::Relaxed) % 2 == 0
    }

    /// Registra quando o primário foi pulado devido a circuit breaker
    /// Incrementa contador para manter distribuição uniforme
    pub fn note_skip_primary(&self) {
        let _ = self.skew.fetch_add(1, Ordering::Relaxed);
    }
}
