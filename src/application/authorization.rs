//! Startup authorization boundary.
//!
//! The application asks a [`StartupAuthorization`] whether it may run before it
//! creates any window. The public build ships [`OpenBetaAuthorization`], which
//! always allows startup and performs no network access — that is exactly what
//! the open beta does today.
//!
//! Keeping the decision behind a trait means a different distribution can supply
//! its own implementation without the overlay, telemetry, or rendering code
//! knowing anything about entitlements.

use std::fmt;

/// Why startup was refused. Deliberately coarse: the overlay only needs to know
/// that it must not start, and the user needs one actionable sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationError {
    /// The authorization backend exists but could not reach a verdict.
    Unavailable,
    /// The verdict was a refusal.
    Denied,
}

impl fmt::Display for AuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("startup authorization is unavailable"),
            Self::Denied => formatter.write_str("startup authorization was denied"),
        }
    }
}

impl std::error::Error for AuthorizationError {}

/// Decides whether the application may start.
pub trait StartupAuthorization {
    /// Returns `Ok(())` when the application may start.
    ///
    /// Implementations must not block for long: this runs before the first
    /// window exists, so the user sees nothing while it is running.
    fn authorize(&self) -> Result<(), AuthorizationError>;
}

/// The open beta: everyone may run the overlay.
///
/// No entitlement check, no machine fingerprint, no network call. This is the
/// only implementation compiled into the public build.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenBetaAuthorization;

impl StartupAuthorization for OpenBetaAuthorization {
    fn authorize(&self) -> Result<(), AuthorizationError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_beta_always_authorizes_startup() {
        assert_eq!(OpenBetaAuthorization.authorize(), Ok(()));
    }

    #[test]
    fn a_refusing_implementation_blocks_startup() {
        struct AlwaysDenied;
        impl StartupAuthorization for AlwaysDenied {
            fn authorize(&self) -> Result<(), AuthorizationError> {
                Err(AuthorizationError::Denied)
            }
        }

        assert_eq!(
            AlwaysDenied.authorize(),
            Err(AuthorizationError::Denied),
            "the boundary must be able to refuse, or it is not a boundary"
        );
    }
}
