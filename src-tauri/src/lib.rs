//! Núcleo compartilhado do HUD: captura/parsing de telemetria (`forza`) e o
//! overlay nativo Direct2D (`render`). O binário in-game é `main.rs`.

pub mod forza;

#[cfg(windows)]
pub mod render;
