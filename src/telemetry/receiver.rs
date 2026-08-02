//! Dedicated UDP receive thread.
//!
//! Independent of the presentation layer: it stores the snapshot and invokes an
//! optional callback (the track-map recorder, when that feature is enabled).
//! The overlay reads the snapshot directly in its render loop, so nothing here
//! knows a window exists.

use std::net::UdpSocket;
use std::sync::Arc;
use std::thread;

use super::model::{self, Telemetry};
use super::packet;
use super::snapshot::Shared;
use crate::diagnostics::{log_error, Once};

/// Largest datagram Forza sends is 323 bytes (FH6); the margin absorbs any
/// longer future format without truncating mid-packet.
const RECEIVE_BUFFER_BYTES: usize = 2048;

/// A failing socket fails on every iteration of a tight loop. Report once.
static RECV_FAILED: Once = Once::new();

/// Binds `0.0.0.0:port` and starts decoding packets into `shared`.
///
/// Returns the bind error when the port is unavailable, so the caller can tell
/// the user which port is taken.
pub fn spawn<F>(
    shared: Arc<Shared>,
    port: u16,
    on_packet: F,
) -> std::io::Result<thread::JoinHandle<()>>
where
    F: Fn(&Telemetry) + Send + 'static,
{
    let socket = UdpSocket::bind(("0.0.0.0", port))?;

    thread::Builder::new()
        .name("forza-udp".into())
        .spawn(move || {
            let mut buffer = [0_u8; RECEIVE_BUFFER_BYTES];
            loop {
                match socket.recv_from(&mut buffer) {
                    Ok((length, _sender)) => {
                        if let Some(packet) = packet::parse(&buffer[..length]) {
                            let mut frame = model::build(&packet);
                            frame._pkt = shared.next_pkt();
                            let frame = Arc::new(frame);
                            shared.store(frame.clone());
                            on_packet(frame.as_ref());
                        }
                    }
                    Err(cause) => {
                        if RECV_FAILED.first() {
                            log_error!(
                                "telemetry socket recv failed: {cause} (repeats suppressed)"
                            );
                        }
                    }
                }
            }
        })
}
