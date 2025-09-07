use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

pub struct Breaker {
    min_samples: usize,
    fail_rate: f64,
    open_for: Duration,
    fails: AtomicUsize,
    total: AtomicUsize,
    opened_at_ms: AtomicU64,
}

impl Breaker {
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

    pub fn on_failure(&self) {
        self.fails.fetch_add(1, Ordering::Relaxed);
        self.total.fetch_add(1, Ordering::Relaxed);
        self.recalc();
    }

    #[allow(dead_code)]
    pub fn on_success(&self) {
        self.total.fetch_add(1, Ordering::Relaxed);
        self.recalc();
    }

    fn recalc(&self) {
        let t = self.total.load(Ordering::Relaxed);
        if t < self.min_samples {
            return;
        }
        let f = self.fails.load(Ordering::Relaxed);
        let rate = f as f64 / t as f64;
        if rate >= self.fail_rate {
            self.opened_at_ms.store(now_ms(), Ordering::Relaxed);
            // reset janela
            self.fails.store(0, Ordering::Relaxed);
            self.total.store(0, Ordering::Relaxed);
        }
    }

    pub fn is_open(&self) -> bool {
        let opened = self.opened_at_ms.load(Ordering::Relaxed);
        if opened == 0 {
            return false;
        }
        now_ms().saturating_sub(opened) < self.open_for.as_millis() as u64
    }
}

fn now_ms() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
