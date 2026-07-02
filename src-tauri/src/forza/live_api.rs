//! API HTTP local para o viewer 3D, exposta pelo HUD/overlay em `127.0.0.1:9877`
//! (override: `VELORYFT_LIVE_PORT`). Rotas:
//! - `GET  /live`      — posição atual + estado de gravação (polling do marcador).
//! - `GET  /map`       — heightmap acumulado completo (grade de células).
//! - `GET  /path`      — traçado ordenado p/ a fita de largura fixa (cresce ao vivo).
//! - `POST /path/delta`— traçado incremental: só pontos novos/mudados desde o último fetch.
//! - `POST /recording` — liga/desliga a gravação (`{ "enabled": bool }`).
//! - `POST /fix`       — fixa o traçado (bloqueia regravação, remove ramos).
//! - `POST /reset`     — zera o mapa local.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Response, Server, StatusCode};

use super::worldmap::WorldMapRecorder;

pub const DEFAULT_LIVE_PORT: u16 = 9877;

fn cors() -> Header {
    Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).expect("cors")
}

fn json_response(status: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
        .with_header(cors())
}

#[derive(Serialize)]
struct LiveDto {
    recording: bool,
    world_id: String,
    cell_size_m: f32,
    pos: Option<[f32; 3]>,
    cells: u32,
    samples: u64,
    path_points: usize,
    loops_locked: u32,
}

#[derive(Deserialize)]
struct RecordingBody {
    enabled: bool,
}

/// Corpo de `POST /path/delta`: o que o viewer já tem. `have` são pares
/// `[id_segmento, nº_pontos]` (array, não objeto, p/ manter ids numéricos).
#[derive(Deserialize)]
struct PathDeltaReq {
    epoch: u64,
    #[serde(default)]
    have: Vec<(u64, usize)>,
}

fn live_port() -> u16 {
    std::env::var("VELORYFT_LIVE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_LIVE_PORT)
}

/// Sobe o servidor HTTP em uma thread dedicada.
pub fn spawn_live_api(
    rec: Arc<Mutex<WorldMapRecorder>>,
    persist_path: PathBuf,
) -> thread::JoinHandle<()> {
    let port = live_port();
    thread::Builder::new()
        .name("worldmap-live-api".into())
        .spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            let server = match Server::http(&addr) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[worldmap-live] falha ao abrir {addr}: {e}");
                    return;
                }
            };
            println!("[worldmap-live] API em http://{addr}/live");

            for mut req in server.incoming_requests() {
                if req.method() == &Method::Options {
                    let res = Response::empty(StatusCode(204))
                        .with_header(cors())
                        .with_header(
                            Header::from_bytes(
                                &b"Access-Control-Allow-Methods"[..],
                                &b"GET, POST, OPTIONS"[..],
                            )
                            .unwrap(),
                        )
                        .with_header(
                            Header::from_bytes(
                                &b"Access-Control-Allow-Headers"[..],
                                &b"Content-Type"[..],
                            )
                            .unwrap(),
                        );
                    let _ = req.respond(res);
                    continue;
                }

                let path = req.url().split('?').next().unwrap_or(req.url());
                let response = match (req.method(), path) {
                    (&Method::Get, "/map") => {
                        // Grade de células acumulada (cobertura). A fita usa /path.
                        let hm = rec.lock().unwrap().to_heightmap();
                        match serde_json::to_string(&hm) {
                            Ok(json) => json_response(200, &json),
                            Err(_) => json_response(500, r#"{"error":"serialização"}"#),
                        }
                    }
                    (&Method::Get, "/path") => {
                        // Traçado ordenado no tempo — a fita de largura fixa cresce
                        // ao vivo conforme se dirige.
                        let p = rec.lock().unwrap().to_path();
                        match serde_json::to_string(&p) {
                            Ok(json) => json_response(200, &json),
                            Err(_) => json_response(500, r#"{"error":"serialização"}"#),
                        }
                    }
                    (&Method::Post, "/path/delta") => {
                        // Incremental: o viewer manda o que já tem; devolvemos só
                        // os pontos novos/mudados (ver WorldMapRecorder::to_path_delta).
                        let mut raw = Vec::new();
                        let _ = std::io::copy(&mut req.as_reader(), &mut raw);
                        let body = String::from_utf8_lossy(&raw);
                        match serde_json::from_str::<PathDeltaReq>(&body) {
                            Ok(r) => {
                                let have: HashMap<u64, usize> = r.have.into_iter().collect();
                                let dto = rec.lock().unwrap().to_path_delta(r.epoch, &have);
                                match serde_json::to_string(&dto) {
                                    Ok(json) => json_response(200, &json),
                                    Err(_) => json_response(500, r#"{"error":"serialização"}"#),
                                }
                            }
                            Err(_) => json_response(400, r#"{"error":"JSON inválido"}"#),
                        }
                    }
                    (&Method::Get, "/live") => {
                        let g = rec.lock().unwrap();
                        let stats = g.stats();
                        let dto = LiveDto {
                            recording: g.is_recording(),
                            world_id: stats.world_id,
                            cell_size_m: g.cell_size_m(),
                            pos: g.last_pos(),
                            cells: stats.cells,
                            samples: stats.samples,
                            path_points: g.path_points(),
                            loops_locked: g.loops_locked(),
                        };
                        match serde_json::to_string(&dto) {
                            Ok(json) => json_response(200, &json),
                            Err(_) => json_response(500, r#"{"error":"serialização"}"#),
                        }
                    }
                    (&Method::Post, "/recording") => {
                        let mut raw = Vec::new();
                        let _ = std::io::copy(&mut req.as_reader(), &mut raw);
                        let body = String::from_utf8_lossy(&raw).into_owned();
                        match serde_json::from_str::<RecordingBody>(&body) {
                            Ok(b) => {
                                let mut g = rec.lock().unwrap();
                                g.set_recording(b.enabled);
                                if !b.enabled {
                                    let stats = g.stats();
                                    if stats.cells > 0 || g.path_points() > 0 {
                                        let _ = g.compact(&persist_path);
                                    }
                                }
                                json_response(200, &format!(r#"{{"recording":{}}}"#, b.enabled))
                            }
                            Err(_) => json_response(400, r#"{"error":"JSON inválido"}"#),
                        }
                    }
                    (&Method::Post, "/reset") => {
                        rec.lock().unwrap().reset();
                        json_response(200, r#"{"ok":true}"#)
                    }
                    (&Method::Post, "/fix") => {
                        let mut g = rec.lock().unwrap();
                        g.fix_map();
                        let locked = g.loops_locked();
                        let pts = g.path_points();
                        json_response(
                            200,
                            &format!(
                                r#"{{"ok":true,"loops_locked":{locked},"path_points":{pts}}}"#
                            ),
                        )
                    }
                    _ => json_response(404, r#"{"error":"not found"}"#),
                };
                let _ = req.respond(response);
            }
        })
        .expect("thread worldmap-live-api")
}
