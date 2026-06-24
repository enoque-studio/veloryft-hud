//! Núcleo de telemetria Forza: recepção UDP, parsing e snapshot compartilhado.
//! Porta do backend Python (`backend/packet.py`, `backend/payload.py`).

pub mod catalog;
pub mod display;
pub mod elevation;
pub mod packet;
pub mod payload;
pub mod state;
pub mod tires;
pub mod udp;
pub mod upload;
pub mod visibility;
