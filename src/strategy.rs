use crate::breaker::Breaker;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct RouteStrategy {
    skew: AtomicU64,
}

impl RouteStrategy {
    pub fn new() -> Self {
        Self {
            skew: AtomicU64::new(0),
        }
    }

    /// true => A primeiro, false => B primeiro
    pub fn pick_a_first(&self, a: &Breaker, b: &Breaker) -> bool {
        if a.is_open() && !b.is_open() {
            return false;
        }
        if b.is_open() && !a.is_open() {
            return true;
        }
        self.skew.fetch_add(1, Ordering::Relaxed) % 2 == 0
    }

    pub fn note_skip_primary(&self) {
        let _ = self.skew.fetch_add(1, Ordering::Relaxed);
    }
}
