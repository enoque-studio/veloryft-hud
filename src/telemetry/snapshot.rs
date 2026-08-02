//! Shared lock-free snapshot of the latest packet.
//! `ArcSwap` lets the render loop read without blocking while the UDP thread
//! swaps the pointer for each packet.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;

use super::model::Telemetry;

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

    /// Next packet number (1, 2, 3, ...).
    pub fn next_pkt(&self) -> u64 {
        self.packets.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn store(&self, t: Arc<Telemetry>) {
        self.latest.store(t);
    }

    #[allow(dead_code)] // used by the fallback command and native renderer
    pub fn latest(&self) -> Arc<Telemetry> {
        self.latest.load_full()
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}
