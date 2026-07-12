//! Núcleo compartilhado do HUD: captura/parsing de telemetria (`forza`) e o
//! overlay nativo Direct2D (`render`). O binário in-game é `main.rs`.

pub mod forza;
pub mod license;

#[cfg(windows)]
pub mod render;
