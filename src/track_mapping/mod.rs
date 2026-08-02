//! Track mapping — optional, off by default.
//!
//! While the player drives, [`world_map`] aggregates the telemetry position into
//! a sparse height grid, and [`local_api`] serves that grid to a 3D viewer over
//! loopback HTTP. Neither is compiled unless the `track_mapping` feature is
//! enabled, so the default overlay opens no listening socket at all.
//!
//! See `docs/decisions/adr_0004_track_mapping_feature.md` for the rationale and
//! `docs/security/local_http_server.md` for the threat model of the local API.

pub mod local_api;
pub mod world_map;
