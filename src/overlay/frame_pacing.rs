//! Frame pacing and adaptive degradation.
//!
//! The overlay competes with the game for the compositor. When
//! `UpdateLayeredWindow` starts taking longer, the fix is *not* to slow the
//! whole HUD down: the cluster carries RPM and pedal input, which a driver
//! reads continuously, while tire temperatures and the elevation profile can
//! lag without anyone noticing.
//!
//! [`RenderGovernor`] therefore tracks how long presentation takes and throttles
//! only the auxiliary panels. The cluster is never rate-limited, at any
//! pressure level.
//!
//! The critical throttle bar is not governed here at all — it owns a separate
//! surface and a dedicated high-priority thread, so its cadence is independent.

use std::time::{Duration, Instant};

use super::geometry::{Kind, PANEL_COUNT};

/// Minimum time between main-loop frames: roughly 60 Hz.
pub(crate) const FRAME_MIN_MS: u64 = 16;
// The critical throttle bar is small and owns its own surface and thread.
// Holding it at 125 Hz shortens the wait for the next draw when packets arrive
// in quick succession.
pub(crate) const THROTTLE_FRAME_MS: u64 = 8;
/// Smoothing time constant while the throttle is opening.
pub(crate) const THROTTLE_TAU_ATTACK: f32 = 0.004;
/// Smoothing time constant while the throttle is closing, slightly slower.
pub(crate) const THROTTLE_TAU_RELEASE: f32 = 0.010;
/// Sleep between polls while the HUD is hidden or fully faded out.
pub(crate) const IDLE_SLEEP_MS: u64 = 50;
/// Average present time above which auxiliary panels start slowing down.
const PRESENT_SLOW_MS: f32 = 4.0;
/// Average present time above which they slow down further.
const PRESENT_SEVERE_MS: f32 = 9.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// How hard the compositor is pushing back.
pub(crate) enum RenderPressure {
    Normal,
    Elevated,
    Severe,
}

/// Prioritizes the cluster (RPM and pedals) and reduces only the cadence of the
/// auxiliary panels once the compositor begins holding `UpdateLayeredWindow`.
pub(crate) struct RenderGovernor {
    last_present: [Instant; PANEL_COUNT],
    present_ewma_ms: f32,
    pressure: RenderPressure,
}

impl RenderGovernor {
    pub fn new() -> Self {
        let ready = Instant::now() - Duration::from_secs(1);
        Self {
            last_present: [ready; PANEL_COUNT],
            present_ewma_ms: 0.0,
            pressure: RenderPressure::Normal,
        }
    }

    /// Minimum gap between presents for `kind` at the current pressure.
    pub fn interval(&self, kind: Kind) -> Duration {
        let ms = match (self.pressure, kind) {
            (_, Kind::Cluster) => 0,
            (RenderPressure::Normal, Kind::Tires | Kind::GForce) => 33,
            (RenderPressure::Normal, Kind::Elevation) => 100,
            (RenderPressure::Elevated, Kind::Tires | Kind::GForce) => 50,
            (RenderPressure::Elevated, Kind::Elevation) => 250,
            (RenderPressure::Severe, Kind::Tires | Kind::GForce) => 100,
            (RenderPressure::Severe, Kind::Elevation) => 500,
        };
        Duration::from_millis(ms)
    }

    /// Whether `kind` may present this frame. The cluster always may.
    pub fn should_present(&self, kind: Kind, now: Instant, edit: bool) -> bool {
        edit || kind == Kind::Cluster
            || now.duration_since(self.last_present[kind.idx()]) >= self.interval(kind)
    }

    /// Records how long one present took, updating the pressure estimate.
    pub fn record(&mut self, kind: Kind, elapsed: Duration, now: Instant) {
        self.last_present[kind.idx()] = now;
        let sample_ms = elapsed.as_secs_f32() * 1000.0;
        self.present_ewma_ms = if self.present_ewma_ms == 0.0 {
            sample_ms
        } else {
            self.present_ewma_ms * 0.88 + sample_ms * 0.12
        };
        self.pressure = match self.pressure {
            RenderPressure::Normal if self.present_ewma_ms >= PRESENT_SLOW_MS => {
                RenderPressure::Elevated
            }
            RenderPressure::Elevated if self.present_ewma_ms >= PRESENT_SEVERE_MS => {
                RenderPressure::Severe
            }
            RenderPressure::Elevated if self.present_ewma_ms < PRESENT_SLOW_MS * 0.65 => {
                RenderPressure::Normal
            }
            RenderPressure::Severe if self.present_ewma_ms < PRESENT_SEVERE_MS * 0.65 => {
                RenderPressure::Elevated
            }
            pressure => pressure,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cluster_is_never_rate_limited() {
        let governor = RenderGovernor::new();
        let now = Instant::now();
        assert!(governor.should_present(Kind::Cluster, now, false));
    }

    #[test]
    fn pressure_slows_only_the_auxiliary_panels() {
        let mut governor = RenderGovernor::new();
        let now = Instant::now();
        for _ in 0..32 {
            governor.record(Kind::Cluster, Duration::from_millis(12), now);
        }
        assert_eq!(governor.pressure, RenderPressure::Severe);
        assert_eq!(governor.interval(Kind::Cluster), Duration::ZERO);
        assert_eq!(governor.interval(Kind::Tires), Duration::from_millis(100));
        assert_eq!(
            governor.interval(Kind::Elevation),
            Duration::from_millis(500)
        );
    }

    #[test]
    fn the_governor_recovers_once_the_compositor_catches_up() {
        let mut governor = RenderGovernor::new();
        let now = Instant::now();
        for _ in 0..32 {
            governor.record(Kind::Cluster, Duration::from_millis(12), now);
        }
        for _ in 0..80 {
            governor.record(Kind::Cluster, Duration::from_micros(500), now);
        }
        assert_eq!(governor.pressure, RenderPressure::Normal);
    }
}
