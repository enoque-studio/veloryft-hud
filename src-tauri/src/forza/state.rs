//! Snapshot compartilhado e lock-free do último pacote.
//! `ArcSwap` permite o loop de render ler sem bloqueio enquanto a thread UDP
//! troca o ponteiro a cada pacote.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;

use super::payload::Telemetry;

pub struct Shared {
    latest: ArcSwap<Telemetry>,
    packets: AtomicU64,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            latest: ArcSwap::from_pointee(Telemetry::default()),
            packets: AtomicU64::new(0),
        }
    }

    /// Próximo número de pacote (1, 2, 3, ...).
    pub fn next_pkt(&self) -> u64 {
        self.packets.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn store(&self, t: Arc<Telemetry>) {
        self.latest.store(t);
    }

    #[allow(dead_code)] // usado pelo comando de fallback / renderer nativo (Fase 3)
    pub fn latest(&self) -> Arc<Telemetry> {
        self.latest.load_full()
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}
