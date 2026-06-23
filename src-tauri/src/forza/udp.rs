//! Thread dedicada de recepção UDP. Independente da camada de apresentação:
//! armazena o snapshot e chama um callback opcional. A app Tauri passa um
//! callback que emite evento; o overlay nativo passa um no-op e lê o snapshot
//! direto no loop de render.

use std::net::UdpSocket;
use std::sync::Arc;
use std::thread;

use super::packet;
use super::payload::{self, Telemetry};
use super::state::Shared;

pub fn spawn<F>(shared: Arc<Shared>, port: u16, on_packet: F) -> std::io::Result<thread::JoinHandle<()>>
where
    F: Fn(&Telemetry) + Send + 'static,
{
    let sock = UdpSocket::bind(("0.0.0.0", port))?;

    let handle = thread::Builder::new()
        .name("forza-udp".into())
        .spawn(move || {
            let mut buf = [0u8; 2048];
            loop {
                match sock.recv_from(&mut buf) {
                    Ok((n, _addr)) => {
                        if let Some(pkt) = packet::parse(&buf[..n]) {
                            let mut t = payload::build(&pkt);
                            t._pkt = shared.next_pkt();
                            let arc = Arc::new(t);
                            shared.store(arc.clone());
                            on_packet(arc.as_ref());
                        }
                    }
                    Err(e) => {
                        eprintln!("[forza-udp] erro recv: {e}");
                    }
                }
            }
        })?;

    Ok(handle)
}
