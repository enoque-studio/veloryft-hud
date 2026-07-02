//! Núcleo de telemetria Forza: recepção UDP, parsing e snapshot compartilhado.
//! Porta do backend Python (`backend/packet.py`, `backend/payload.py`).

/// Telemetria de **mapeamento de pista** (worldmap, live API `:9877`, autosave).
///
/// Desligada no lançamento inicial — o HUD in-game (RPM, marcha, pedais, elevação
/// da volta) continua normal via UDP. Reativar quando viewer + back + upload
/// estiverem prontos para a galera.
pub const TRACK_MAPPING_ENABLED: bool = false;

pub mod catalog;
pub mod display;
pub mod elevation;
pub mod live_api;
pub mod packet;
pub mod payload;
pub mod revlimit;
pub mod state;
pub mod tires;
pub mod udp;
pub mod visibility;
pub mod worldmap;
