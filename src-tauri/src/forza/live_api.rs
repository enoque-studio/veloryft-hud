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
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Response, Server, StatusCode};

use super::worldmap::WorldMapRecorder;

pub const DEFAULT_LIVE_PORT: u16 = 9877;
const MAX_REQUEST_BODY_BYTES: u64 = 16 * 1024;
const MAX_DELTA_SEGMENTS: usize = 8_192;

fn browser_origin(req: &tiny_http::Request) -> Option<&str> {
    req.headers()
        .iter()
        .find(|header| header.field.equiv("Origin"))
        .map(|header| header.value.as_str())
}

/// The 3D viewer is a local development tool. Do not let arbitrary websites
/// issue state-changing requests to this loopback server from a browser.
fn is_allowed_viewer_origin(origin: &str) -> bool {
    matches!(origin, "http://localhost" | "http://127.0.0.1")
        || ["http://localhost:", "http://127.0.0.1:"]
            .iter()
            .any(|prefix| {
                origin
                    .strip_prefix(prefix)
                    .is_some_and(|port| !port.is_empty() && port.parse::<u16>().is_ok())
            })
}

fn with_cors<R: Read>(response: Response<R>, origin: Option<&str>) -> Response<R> {
    let Some(origin) = origin else {
        return response;
    };
    response
        .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], origin).expect("cors"))
        .with_header(Header::from_bytes(&b"Vary"[..], &b"Origin"[..]).expect("vary"))
}

fn json_response(status: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap())
        .with_header(Header::from_bytes(&b"X-Content-Type-Options"[..], &b"nosniff"[..]).unwrap())
}

fn read_limited_body(req: &mut tiny_http::Request) -> Result<Vec<u8>, u16> {
    let mut raw = Vec::with_capacity(1_024);
    req.as_reader()
        .take(MAX_REQUEST_BODY_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| 400_u16)?;
    if raw.len() > MAX_REQUEST_BODY_BYTES as usize {
        return Err(413);
    }
    Ok(raw)
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
#[serde(deny_unknown_fields)]
struct RecordingBody {
    enabled: bool,
}

/// Corpo de `POST /path/delta`: o que o viewer já tem. `have` são pares
/// `[id_segmento, nº_pontos]` (array, não objeto, p/ manter ids numéricos).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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
                let requested_origin = browser_origin(&req).map(str::to_owned);
                let allowed_origin = requested_origin
                    .as_deref()
                    .filter(|origin| is_allowed_viewer_origin(origin))
                    .map(str::to_owned);
                if requested_origin.is_some() && allowed_origin.is_none() {
                    let _ = req.respond(json_response(403, r#"{"error":"origem não permitida"}"#));
                    continue;
                }

                if req.method() == &Method::Options {
                    let res = Response::empty(StatusCode(204))
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
                    let _ = req.respond(with_cors(res, allowed_origin.as_deref()));
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
                        match read_limited_body(&mut req) {
                            Ok(raw) => match serde_json::from_slice::<PathDeltaReq>(&raw) {
                                Ok(r) if r.have.len() <= MAX_DELTA_SEGMENTS => {
                                    let have: HashMap<u64, usize> = r.have.into_iter().collect();
                                    let dto = rec.lock().unwrap().to_path_delta(r.epoch, &have);
                                    match serde_json::to_string(&dto) {
                                        Ok(json) => json_response(200, &json),
                                        Err(_) => json_response(500, r#"{"error":"serialização"}"#),
                                    }
                                }
                                Ok(_) => json_response(413, r#"{"error":"muitos segmentos"}"#),
                                Err(_) => json_response(400, r#"{"error":"JSON inválido"}"#),
                            },
                            Err(413) => json_response(413, r#"{"error":"corpo muito grande"}"#),
                            Err(_) => json_response(400, r#"{"error":"corpo inválido"}"#),
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
                    (&Method::Post, "/recording") => match read_limited_body(&mut req) {
                        Ok(raw) => match serde_json::from_slice::<RecordingBody>(&raw) {
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
                        },
                        Err(413) => json_response(413, r#"{"error":"corpo muito grande"}"#),
                        Err(_) => json_response(400, r#"{"error":"corpo inválido"}"#),
                    },
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
                let _ = req.respond(with_cors(response, allowed_origin.as_deref()));
            }
        })
        .expect("thread worldmap-live-api")
}

#[cfg(test)]
mod tests {
    use super::is_allowed_viewer_origin;

    #[test]
    fn accepts_only_loopback_viewer_origins() {
        assert!(is_allowed_viewer_origin("http://localhost:5173"));
        assert!(is_allowed_viewer_origin("http://127.0.0.1:4173"));
        assert!(!is_allowed_viewer_origin("https://localhost:5173"));
        assert!(!is_allowed_viewer_origin(
            "http://localhost.evil.example:5173"
        ));
        assert!(!is_allowed_viewer_origin("https://evil.example"));
    }
}
