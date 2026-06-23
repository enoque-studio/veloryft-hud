pub mod forza;

#[cfg(windows)]
pub mod render;

use std::sync::Arc;

use tauri::{Emitter, Manager};

use forza::state::Shared;

/// Porta UDP padrão do Forza (igual ao backend Python). Override via env.
const UDP_PORT: u16 = 20066;

/// Evento recebido pela webview via `listen("telemetry", ...)`.
pub const TELEMETRY_EVENT: &str = "telemetry";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            forza::catalog::init();
            let shared = Arc::new(Shared::new());
            app.manage(shared.clone());

            // Janela overlay: cobre o monitor primário e deixa cliques passarem.
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_ignore_cursor_events(true);
                if let Ok(Some(monitor)) = win.current_monitor() {
                    let _ = win.set_position(*monitor.position());
                    let _ = win.set_size(*monitor.size());
                }
            }

            let port = std::env::var("FORZA_UDP_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(UDP_PORT);

            let handle = app.handle().clone();
            match forza::udp::spawn(shared, port, move |t| {
                let _ = handle.emit(TELEMETRY_EVENT, t);
            }) {
                Ok(_) => println!("[forza-hud] ouvindo UDP em 0.0.0.0:{port}"),
                Err(e) => eprintln!("[forza-hud] falha ao abrir UDP {port}: {e}"),
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
