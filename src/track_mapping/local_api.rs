//! Loopback HTTP API for the 3D track viewer, served at `127.0.0.1:9877`
//! (override with `VELORYFT_LIVE_PORT`).
//!
//! Routes:
//! - `GET  /live`       — current position and recording state (marker polling).
//! - `GET  /map`        — the full accumulated heightmap (cell grid).
//! - `GET  /path`       — ordered driven path for the fixed-width ribbon.
//! - `POST /path/delta` — only points added or changed since the viewer's last fetch.
//! - `POST /recording`  — start or stop recording (`{ "enabled": bool }`).
//! - `POST /fix`        — freeze the path (block re-recording, drop branches).
//! - `POST /reset`      — clear the local map.
//!
//! # Security boundary
//!
//! This server exists only when the `track_mapping` feature is compiled in. It
//! binds loopback exclusively, rejects any browser `Origin` that is not
//! `localhost`/`127.0.0.1`, and caps request bodies. It performs no
//! authentication: any local process can call it, so it must never be bound to a
//! routable address. See `docs/security/local_http_server.md`.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Response, Server, StatusCode};

use super::world_map::WorldMapRecorder;
use crate::diagnostics::{log_error, log_info};

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
    read_limited(req.as_reader())
}

/// Reads at most [`MAX_REQUEST_BODY_BYTES`], returning 413 rather than
/// allocating whatever a caller claims to be sending.
///
/// Split out from [`read_limited_body`] so the limit can be tested without
/// standing up an HTTP server: the cap is the only thing between this loopback
/// endpoint and a local process exhausting memory.
fn read_limited(source: &mut (impl Read + ?Sized)) -> Result<Vec<u8>, u16> {
    let mut body = Vec::with_capacity(1_024);
    source
        .take(MAX_REQUEST_BODY_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|_| 400_u16)?;
    if body.len() > MAX_REQUEST_BODY_BYTES as usize {
        return Err(413);
    }
    Ok(body)
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

/// Environment variable overriding [`DEFAULT_LIVE_PORT`]. Part of the public
/// contract — see `docs/reference/environment_variables.md`.
pub const LIVE_PORT_VAR: &str = "VELORYFT_LIVE_PORT";

fn live_port() -> u16 {
    std::env::var(LIVE_PORT_VAR)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_LIVE_PORT)
}

/// Starts the loopback HTTP server on a dedicated thread.
pub fn spawn(rec: Arc<Mutex<WorldMapRecorder>>, persist_path: PathBuf) -> thread::JoinHandle<()> {
    let port = live_port();
    thread::Builder::new()
        .name("worldmap-live-api".into())
        .spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            let server = match Server::http(&addr) {
                Ok(s) => s,
                Err(cause) => {
                    log_error!("track-mapping API could not bind {addr}: {cause}");
                    return;
                }
            };
            log_info!("track-mapping API listening on http://{addr}/live");

            for mut req in server.incoming_requests() {
                let requested_origin = browser_origin(&req).map(str::to_owned);
                let allowed_origin = requested_origin
                    .as_deref()
                    .filter(|origin| is_allowed_viewer_origin(origin))
                    .map(str::to_owned);
                if requested_origin.is_some() && allowed_origin.is_none() {
                    let _ = req.respond(json_response(403, r#"{"error":"origin not allowed"}"#));
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
                            Err(_) => json_response(500, r#"{"error":"serialization failed"}"#),
                        }
                    }
                    (&Method::Get, "/path") => {
                        // Traçado ordenado no tempo — a fita de largura fixa cresce
                        // ao vivo conforme se dirige.
                        let p = rec.lock().unwrap().to_path();
                        match serde_json::to_string(&p) {
                            Ok(json) => json_response(200, &json),
                            Err(_) => json_response(500, r#"{"error":"serialization failed"}"#),
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
                                        Err(_) => json_response(
                                            500,
                                            r#"{"error":"serialization failed"}"#,
                                        ),
                                    }
                                }
                                Ok(_) => json_response(413, r#"{"error":"too many segments"}"#),
                                Err(_) => json_response(400, r#"{"error":"invalid JSON"}"#),
                            },
                            Err(413) => json_response(413, r#"{"error":"request body too large"}"#),
                            Err(_) => json_response(400, r#"{"error":"invalid request body"}"#),
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
                            Err(_) => json_response(500, r#"{"error":"serialization failed"}"#),
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
                            Err(_) => json_response(400, r#"{"error":"invalid JSON"}"#),
                        },
                        Err(413) => json_response(413, r#"{"error":"request body too large"}"#),
                        Err(_) => json_response(400, r#"{"error":"invalid request body"}"#),
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
        .expect("spawning the track-mapping API thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_loopback_viewer_origins() {
        assert!(is_allowed_viewer_origin("http://localhost"));
        assert!(is_allowed_viewer_origin("http://127.0.0.1"));
        assert!(is_allowed_viewer_origin("http://localhost:5173"));
        assert!(is_allowed_viewer_origin("http://127.0.0.1:4173"));
    }

    #[test]
    fn rejects_origins_that_only_look_like_loopback() {
        // A site can register a hostname ending in "localhost"; matching on a
        // prefix without the port separator would let it through.
        assert!(!is_allowed_viewer_origin(
            "http://localhost.evil.example:5173"
        ));
        assert!(!is_allowed_viewer_origin("http://127.0.0.1.evil.example"));
        assert!(!is_allowed_viewer_origin("https://evil.example"));
        assert!(!is_allowed_viewer_origin("http://localhost:notaport"));
        assert!(!is_allowed_viewer_origin("http://localhost:"));
        assert!(!is_allowed_viewer_origin(""));
    }

    #[test]
    fn rejects_loopback_over_https_because_the_viewer_is_served_over_http() {
        // The dev viewer is plain HTTP. Accepting https would widen the surface
        // for no benefit, since this server never speaks TLS.
        assert!(!is_allowed_viewer_origin("https://localhost:5173"));
        assert!(!is_allowed_viewer_origin("https://127.0.0.1:4173"));
    }

    #[test]
    fn a_body_within_the_limit_is_read_whole() {
        let body = vec![b'x'; MAX_REQUEST_BODY_BYTES as usize];

        assert_eq!(read_limited(&mut body.as_slice()), Ok(body));
    }

    #[test]
    fn a_body_over_the_limit_is_refused_with_413() {
        let body = vec![b'x'; MAX_REQUEST_BODY_BYTES as usize + 1];

        assert_eq!(read_limited(&mut body.as_slice()), Err(413));
    }

    #[test]
    fn an_empty_body_is_not_an_error() {
        assert_eq!(read_limited(&mut [].as_slice()), Ok(Vec::new()));
    }

    #[test]
    fn the_body_limit_already_bounds_the_segment_count() {
        // Two limits guard /path/delta: the body size and MAX_DELTA_SEGMENTS.
        // The smallest a `have` pair can be encoded is `[0,0],` — six bytes —
        // so the body limit caps the count well below the explicit segment cap.
        // That ordering is the point: the cheap check (size) rejects first, and
        // the segment cap is a backstop that cannot be reached by a larger body.
        const SMALLEST_ENCODED_PAIR_BYTES: usize = 6;
        let reachable_segments = MAX_REQUEST_BODY_BYTES as usize / SMALLEST_ENCODED_PAIR_BYTES;

        assert!(
            reachable_segments < MAX_DELTA_SEGMENTS,
            "a request within the body limit must not be able to exceed the segment cap"
        );
    }
}
