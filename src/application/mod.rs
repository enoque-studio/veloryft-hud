//! Startup orchestration: everything that happens before and after the overlay
//! owns the thread.
//!
//! This module deliberately holds no rendering, windowing, or telemetry logic.
//! It decides whether the application may run, prepares diagnostics, hands
//! control to the overlay runtime, and turns a failure into something a user
//! without a console can act on.

pub mod authorization;

use crate::diagnostics::{self, log_info};
use authorization::{OpenBetaAuthorization, StartupAuthorization};

/// Runs the application to completion. Returns `Err` only when the overlay could
/// not start or crashed; the user has already been told by then.
pub fn run() -> Result<(), StartupFailure> {
    diagnostics::init();
    log_info!("veloryft {} starting", env!("CARGO_PKG_VERSION"));

    if let Err(cause) = OpenBetaAuthorization.authorize() {
        diagnostics::report_startup_failure(
            "This build is not authorized to run on this machine.",
            &format_args!("{cause}"),
        );
        return Err(StartupFailure::NotAuthorized);
    }

    #[cfg(windows)]
    {
        crate::overlay::run().map_err(|cause| {
            diagnostics::report_startup_failure(
                "The Veloryft overlay could not be created. This usually means \
                 Direct2D or DirectWrite is unavailable — update your graphics \
                 driver and make sure you are on 64-bit Windows 10 or later.",
                &format_args!("{cause:?}"),
            );
            StartupFailure::OverlayUnavailable
        })?;
        log_info!("veloryft exiting normally");
        Ok(())
    }

    #[cfg(not(windows))]
    {
        diagnostics::report_startup_failure(
            "Veloryft runs only on Windows: the overlay is built on Win32 layered \
             windows and Direct2D.",
            &format_args!("target platform is not windows"),
        );
        Err(StartupFailure::UnsupportedPlatform)
    }
}

/// Why the application stopped before reaching its message loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupFailure {
    NotAuthorized,
    OverlayUnavailable,
    #[cfg_attr(windows, allow(dead_code))]
    UnsupportedPlatform,
}

impl StartupFailure {
    /// Process exit code. Distinct per cause so scripts and crash reports can
    /// tell "not supported here" from "the overlay broke".
    #[must_use]
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::NotAuthorized => 2,
            Self::OverlayUnavailable => 3,
            Self::UnsupportedPlatform => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_failure_has_a_distinct_non_zero_exit_code() {
        let codes = [
            StartupFailure::NotAuthorized.exit_code(),
            StartupFailure::OverlayUnavailable.exit_code(),
            StartupFailure::UnsupportedPlatform.exit_code(),
        ];
        assert!(codes.iter().all(|&code| code != 0));
        let mut unique = codes;
        unique.sort_unstable();
        unique.iter().reduce(|previous, current| {
            assert_ne!(previous, current, "exit codes must distinguish causes");
            current
        });
    }
}
