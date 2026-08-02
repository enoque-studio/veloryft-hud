//! Forza telemetry: UDP reception, packet decoding, and the shared snapshot.
//!
//! Layering, from the wire inwards:
//!
//! * [`receiver`] owns the socket and its thread.
//! * [`packet`] decodes raw bytes by offset, zero-copy.
//! * [`model`] is the domain frame the rest of the application reasons about.
//! * [`snapshot`] publishes the latest frame lock-free.
//! * [`display`], [`visibility`], [`elevation`], [`rev_limit`], and
//!   [`tire_temperature`] derive presentation state from that frame.
//!
//! Nothing in this module may reference Win32 or Direct2D types.

pub mod display;
pub mod elevation;
pub mod model;
pub mod packet;
pub mod receiver;
pub mod rev_limit;
pub mod snapshot;
pub mod tire_temperature;
pub mod vehicle_catalog;
pub mod visibility;
