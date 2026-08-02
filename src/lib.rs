//! Veloryft — a native telemetry overlay for Forza on Windows.
//!
//! The library half of the crate holds everything except the process entry
//! point, so that the telemetry pipeline can be tested without a window.
//!
//! ```text
//! Forza UDP telemetry
//!         ↓  telemetry::receiver
//! packet decoding                     telemetry::packet
//!         ↓
//! domain model                        telemetry::model
//!         ↓
//! lock-free snapshot                  telemetry::snapshot
//!         ↓
//! smoothing and visibility            telemetry::display
//!         ↓
//! Win32 layered windows + Direct2D    overlay
//! ```
//!
//! [`application`] owns startup: it decides whether the program may run, opens
//! the log, and hands control to the overlay.
//!
//! [`telemetry`] never references Win32 or Direct2D types, and the overlay never
//! parses packets. That separation is what keeps the pipeline testable on any
//! platform.

pub mod application;
pub mod diagnostics;
pub mod localization;
pub mod paths;
pub mod telemetry;
/// Derivation of the Windows version resource from the Cargo package version.
///
/// Shared by the build script (through `include!`) and the library, so the rule
/// has exactly one implementation and one set of tests. See
/// `docs/distribution/versioning.md`.
pub mod version;

// Win32 + Direct2D presentation. Internal: the overlay is reached through
// `application::run`, never driven directly.
#[cfg(windows)]
pub(crate) mod overlay;

// World-height recording and its loopback API. Compiled only with the
// `track_mapping` feature — see docs/decisions/adr_0004_track_mapping_feature.md.
#[cfg(feature = "track_mapping")]
pub mod track_mapping;
