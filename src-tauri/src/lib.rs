pub mod forza;

#[cfg(windows)]
pub mod render;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::{Emitter, Manager};

use forza::display::DisplaySmoother;
use forza::state::Shared;

/// Porta UDP padrão do Forza (igual ao backend Python). Override via env.
const UDP_PORT: u16 = 20066;

/// Frame suavizado para a webview — mesma lógica do overlay Direct2D.
pub const DISPLAY_EVENT: &str = "display";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            forza::catalog::init();
            let shared = Arc::new(Shared::new());
            app.manage(shared.clone());

            let port = std::env::var("FORZA_UDP_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(UDP_PORT);

            match forza::udp::spawn(shared.clone(), port, |_| {}) {
                Ok(_) => println!("[forza-hud] ouvindo UDP em 0.0.0.0:{port}"),
                Err(e) => eprintln!("[forza-hud] falha ao abrir UDP {port}: {e}"),
            }

            let handle = app.handle().clone();
            thread::Builder::new()
                .name("hud-display".into())
                .spawn(move || {
                    let mut smoother = DisplaySmoother::new();
                    let interval = Duration::from_micros(16_666);
                    loop {
                        let snap = shared.latest();
                        let frame = smoother.tick(&snap, false).clone();
                        let _ = handle.emit(DISPLAY_EVENT, frame);
                        thread::sleep(interval);
                    }
                })
                .expect("thread hud-display");

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
