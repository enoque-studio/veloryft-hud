//! World elevation-map recorder. It aggregates the telemetry position's `y`
//! coordinate from `[x, y, z]` into grid cells while the player drives, using
//! the same position and teleport validation as
//! [`crate::telemetry::elevation::ElevationTracker`].
//!
//! Persistence seeds in-memory state from disk at startup (`load_seed`) and
//! overwrites snapshots during flushes (`save`). Because memory only grows from
//! the loaded state, overwriting is idempotent and merges sessions as long as
//! only one process writes the files. The overlay is that sole owner.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use veloryft_protocol::{HeightCell, WorldHeightmap, SURFACE_KINDS};

use crate::telemetry::model::Telemetry;

/// Surface classes used as indices in the `HeightCell::surf` histogram and
/// aligned with `veloryft_protocol::SURFACE_KINDS`. Forza does not label the
/// surface in UDP; this configurable heuristic infers it from rumble and puddle data.
pub mod surface {
    pub const UNKNOWN: usize = 0;
    pub const ASPHALT: usize = 1; // via pavimentada
    pub const DIRT: usize = 2; // dirt or packed-earth trail
    pub const GRASS: usize = 3; // grass or rough off-road terrain
    pub const WATER: usize = 4; // ford or deep puddle
}

/// Mean puddle depth (0..~1) above which a sample becomes water. This takes
/// precedence over rumble because crossing water wets every surface.
const PUDDLE_WET: f32 = 0.10;
/// Mean surface rumble (0..~1) below which the sample is smooth asphalt.
const RUMBLE_PAVED: f32 = 0.05;
/// Rumble below this value means dirt/trail; above it means grass/rough off-road.
const RUMBLE_DIRT: f32 = 0.30;
const MAX_VALID_TEMP: f32 = 900.0;

#[inline]
fn quad_avg(q: [f32; 4]) -> f32 {
    (q[0] + q[1] + q[2] + q[3]) * 0.25
}

/// Classify a sample's surface from packet signals. Return `UNKNOWN` when no
/// signal exists, such as a Sled packet without per-wheel data.
fn classify_surface(t: &Telemetry) -> usize {
    let Some(rumble) = t.surf_rumble else {
        return surface::UNKNOWN;
    };
    if let Some(p) = t.puddle {
        if quad_avg(p) > PUDDLE_WET {
            return surface::WATER;
        }
    }
    let r = quad_avg(rumble);
    if !r.is_finite() {
        return surface::UNKNOWN;
    }
    if r < RUMBLE_PAVED {
        surface::ASPHALT
    } else if r < RUMBLE_DIRT {
        surface::DIRT
    } else {
        surface::GRASS
    }
}

fn tire_temp_mean(tires: Option<[f32; 4]>) -> f32 {
    let Some(temps) = tires else {
        return 0.0;
    };
    let mut sum = 0.0;
    let mut n = 0.0;
    for temp in temps {
        if temp.is_finite() && temp > 0.0 && temp <= MAX_VALID_TEMP {
            sum += temp;
            n += 1.0;
        }
    }
    if n == 0.0 {
        0.0
    } else {
        sum / n
    }
}

/// Default cell edge in metres, fine enough to retain road width and detail.
/// The environment can override it.
pub const DEFAULT_CELL_M: f32 = 0.5;
/// A jump above this distance is a teleport, fast travel, or respawn; discard it.
const MAX_JUMP: f32 = 40.0;
/// Minimum movement between samples in metres, preventing a stationary vehicle
/// from inflating `n`. Five centimetres preserves fine route detail.
const MIN_MOVE_M: f32 = 0.05;
/// Gap from the last recorded point above which the ribbon starts a new segment.
/// This occurs when crossing mapped territory (skipped cells) or after a small
/// jump below the teleport threshold, preventing a false line between distant
/// points. Smaller gaps continue the existing segment.
const PATH_BREAK_GAP_M: f32 = 22.0;
/// World identifier; FH6 currently has one open world.
pub const WORLD_ID: &str = "fh6";
/// Disk autosave interval.
const AUTOSAVE_SECS: u64 = 5;
/// Write a complete JSON snapshot every N autosave ticks, approximately 60 seconds.
const COMPACT_EVERY_TICKS: u64 = 12;

/// Cell edge from `VELORYFT_WORLDMAP_CELL_M`, or the default.
fn cell_size_from_env() -> f32 {
    std::env::var("VELORYFT_WORLDMAP_CELL_M")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(DEFAULT_CELL_M)
}

fn client_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// One cell's accumulator, using `f64` sums for accuracy across many samples.
#[derive(Clone, Debug)]
struct CellAcc {
    sum_y: f64,
    min_y: f32,
    max_y: f32,
    sum_spd: f64,
    sum_temp: f64,
    n: u32,
    /// Surface-class histogram; see [`surface`].
    surf: [u32; SURFACE_KINDS],
}

impl Default for CellAcc {
    fn default() -> Self {
        Self {
            sum_y: 0.0,
            min_y: f32::INFINITY,
            max_y: f32::NEG_INFINITY,
            sum_spd: 0.0,
            sum_temp: 0.0,
            n: 0,
            surf: [0; SURFACE_KINDS],
        }
    }
}

impl CellAcc {
    fn observe(&mut self, y: f32, kind: usize, speed: f32, temp: f32) {
        self.sum_y += y as f64;
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
        self.sum_spd += speed as f64;
        self.sum_temp += temp as f64;
        self.n += 1;
        if let Some(slot) = self.surf.get_mut(kind) {
            *slot += 1;
        }
    }

    fn mean(&self) -> f32 {
        if self.n == 0 {
            0.0
        } else {
            (self.sum_y / self.n as f64) as f32
        }
    }

    fn spd_mean(&self) -> f32 {
        if self.n == 0 {
            0.0
        } else {
            (self.sum_spd / self.n as f64) as f32
        }
    }

    fn temp_mean(&self) -> f32 {
        if self.n == 0 {
            0.0
        } else {
            (self.sum_temp / self.n as f64) as f32
        }
    }
}

/// Lightweight statistics for UI and commands.
#[derive(Serialize, Clone, Default, Debug)]
pub struct WorldMapStats {
    pub world_id: String,
    pub cells: u32,
    pub samples: u64,
    pub cols: u32,
    pub rows: u32,
}

/// A route point in driving order.
#[derive(Clone, Copy, Debug)]
struct PathPoint {
    x: f32,
    y: f32,
    z: f32,
    spd: f32,
    temp: f32,
    surf: u8,
}

/// Ordered route served to the viewer for its fixed-width ribbon. Each point is
/// `[x, y, z, speed_kmh, temperature_c, surface]`; teleports and stops split segments.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct PathDto {
    pub world_id: String,
    pub points: usize,
    pub segments: Vec<Vec<[f32; 6]>>,
}

/// A segment with new or changed delta points. `from` is the index where `pts`
/// begins for appending; `from == 0` means replace the segment.
#[derive(Serialize, Clone, Debug)]
pub struct PathDeltaSeg {
    pub id: u64,
    pub from: usize,
    pub pts: Vec<[f32; 6]>,
}

/// Incremental `/path/delta` response. `order` lists all current IDs in route
/// order, letting the viewer discard missing segments and reorder the rest.
/// `changed` contains only segments with new points. `full` is true when epochs differ.
/// On reset/restart, the viewer clears its cache before applying the response.
#[derive(Serialize, Clone, Debug)]
pub struct PathDeltaDto {
    pub world_id: String,
    pub epoch: u64,
    pub points: usize,
    pub loops_locked: u32,
    pub full: bool,
    pub order: Vec<u64>,
    pub changed: Vec<PathDeltaSeg>,
}

/// Incremental persistence delta drained at each autosave flush. `reset` asks
/// the writer to truncate logs after an epoch change; `cells` and `path` are appended.
pub struct PersistDelta {
    pub reset: bool,
    pub cells: Vec<HeightCell>,
    /// New route points: `(segment_id, [x, y, z, speed, temperature, surface])`.
    pub path: Vec<(u64, [f32; 6])>,
}

pub struct WorldMapRecorder {
    cell_m: f32,
    cells: HashMap<(i32, i32), CellAcc>,
    last_xz: Option<(f32, f32)>,
    last_pos: Option<[f32; 3]>,
    recording: bool,
    dirty: bool,
    /// Ordered route segments for the ribbon, independent of the cell grid.
    path: Vec<Vec<PathPoint>>,
    /// Stable ID per segment, parallel to `path`, plus an epoch incremented on
    /// reset. The `/path/delta` API uses these to send only new or changed points.
    seg_ids: Vec<u64>,
    next_seg_id: u64,
    epoch: u64,
    seg_open: bool,
    path_len: usize,
    last_path_xz: Option<(f32, f32)>,
    /// Cells at `cell_m` resolution already covered by the ribbon. Revisiting a
    /// cell does not add another point. Rebuilt from disk independently of the
    /// heightmap so the ribbon can recover on its own.
    path_cells: HashSet<(i32, i32)>,
    /// Append-only persistence cursors. `pending_cells` contains cells not yet
    /// logged; `seg_saved[i]` counts logged points for segment `i`; `persist_epoch`
    /// is the epoch represented on disk. A mismatch means reset and truncates logs.
    pending_cells: Vec<(i32, i32)>,
    seg_saved: Vec<usize>,
    persist_epoch: u64,
}

impl Default for WorldMapRecorder {
    fn default() -> Self {
        Self::with_cell_size(cell_size_from_env())
    }
}

impl WorldMapRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with an explicit cell edge in metres.
    pub fn with_cell_size(cell_m: f32) -> Self {
        Self {
            cell_m,
            cells: HashMap::new(),
            last_xz: None,
            last_pos: None,
            recording: false,
            dirty: false,
            path: Vec::new(),
            seg_ids: Vec::new(),
            next_seg_id: 0,
            epoch: 0,
            seg_open: false,
            path_len: 0,
            last_path_xz: None,
            path_cells: HashSet::new(),
            pending_cells: Vec::new(),
            seg_saved: Vec::new(),
            persist_epoch: 0,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    pub fn set_recording(&mut self, on: bool) {
        self.recording = on;
        self.last_xz = None;
        self.break_segment(); // toggling recording starts a new route segment
    }

    pub fn last_pos(&self) -> Option<[f32; 3]> {
        self.last_pos
    }

    pub fn cell_size_m(&self) -> f32 {
        self.cell_m
    }

    /// Retained for live API compatibility. Whole-world mapping no longer locks
    /// laps, so this value is always zero.
    pub fn loops_locked(&self) -> u32 {
        0
    }

    /// Previously fixed the route by preventing rerecording. Without corridor
    /// deduplication, it now only closes the active segment so later points start
    /// on a new line.
    pub fn fix_map(&mut self) {
        self.break_segment();
        self.dirty = true;
    }

    #[inline]
    fn cell_index(&self, v: f32) -> i32 {
        (v / self.cell_m).floor() as i32
    }

    /// Integrate a telemetry frame when `active` indicates driving.
    /// Record cells only while `recording` is true; update `last_pos` whenever position exists.
    pub fn ingest(&mut self, t: &Telemetry, active: bool) {
        if !active || t.sled_only != 0 {
            self.last_xz = None;
            self.last_pos = None;
            self.break_segment();
            return;
        }
        let Some([x, y, z]) = t.pos else {
            self.last_pos = None;
            return;
        };
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            return;
        }

        self.last_pos = Some([x, y, z]);

        if !self.recording {
            return;
        }

        if let Some((lx, lz)) = self.last_xz {
            let jump = ((x - lx).powi(2) + (z - lz).powi(2)).sqrt();
            if jump > MAX_JUMP {
                self.last_xz = Some((x, z)); // reanchor without recording the jump
                self.break_segment();
                return;
            }
            if jump < MIN_MOVE_M {
                return; // negligible movement: do not resample the same cell
            }
        }

        let kind = classify_surface(t);
        let speed = t.speed.unwrap_or(0.0);
        let temp = tire_temp_mean(t.tires);
        let cell_key = (self.cell_index(x), self.cell_index(z));
        // Heightmap coverage records each cell once without marking it again.
        if !self.cells.contains_key(&cell_key) {
            self.cells
                .entry(cell_key)
                .or_default()
                .observe(y, kind, speed, temp);
            self.pending_cells.push(cell_key); // incremental persistence queue
        }
        self.last_xz = Some((x, z));

        // The ordered ribbon gets one point per newly covered cell in driving
        // order. There is no corridor lock, so it maps the whole world and only
        // ignores revisits to the exact same cell to avoid stacked geometry.
        if !self.path_cells.contains(&cell_key) {
            // A large gap after mapped territory or a sub-teleport jump starts a
            // new segment rather than drawing a false straight line.
            if let Some((px, pz)) = self.last_path_xz {
                if ((x - px).powi(2) + (z - pz).powi(2)).sqrt() > PATH_BREAK_GAP_M {
                    self.break_segment();
                }
            }
            self.record_path_point(PathPoint {
                x,
                y,
                z,
                spd: speed,
                temp,
                surf: kind as u8,
            });
            self.path_cells.insert(cell_key);
        }

        self.dirty = true;
    }

    /// Merge a heightmap, such as one loaded from disk, into current state.
    pub fn merge_from(&mut self, hm: &WorldHeightmap) {
        for c in &hm.cells {
            self.merge_cell(c);
        }
        self.dirty = true;
    }

    fn record_path_point(&mut self, p: PathPoint) {
        self.last_path_xz = Some((p.x, p.z));
        self.push_path(p);
    }

    fn break_segment(&mut self) {
        self.seg_open = false;
        self.last_path_xz = None;
    }

    fn push_path(&mut self, p: PathPoint) {
        if !self.seg_open {
            self.path.push(Vec::new());
            self.seg_ids.push(self.next_seg_id);
            self.seg_saved.push(0);
            self.next_seg_id = self.next_seg_id.wrapping_add(1);
            self.seg_open = true;
        }
        if let Some(seg) = self.path.last_mut() {
            seg.push(p);
            self.path_len += 1;
        }
        // No fixed ceiling: whole-world growth is bounded by spatial cell
        // coverage in `path_cells`, not by play time.
    }

    /// Number of route points, used by the UI to decide when to fetch `/path` again.
    pub fn path_points(&self) -> usize {
        self.path_len
    }

    /// Serialize ordered segments with at least two points for the viewer.
    pub fn to_path(&self) -> PathDto {
        let segments = self
            .path
            .iter()
            .filter(|s| s.len() >= 2)
            .map(|s| {
                s.iter()
                    .map(|p| [p.x, p.y, p.z, p.spd, p.temp, p.surf as f32])
                    .collect()
            })
            .collect();
        PathDto {
            world_id: WORLD_ID.to_string(),
            points: self.path_len,
            segments,
        }
    }

    /// Route delta for the viewer. Given `client_epoch` and its point count per
    /// segment in `have`, return only new or changed points. On kilometre-scale
    /// maps this reduces each update from megabytes to kilobytes.
    pub fn to_path_delta(&self, client_epoch: u64, have: &HashMap<u64, usize>) -> PathDeltaDto {
        let full = client_epoch != self.epoch;
        let mut order = Vec::with_capacity(self.path.len());
        let mut changed = Vec::new();

        for (seg, &id) in self.path.iter().zip(self.seg_ids.iter()) {
            order.push(id);
            let len = seg.len();
            let known = if full {
                0
            } else {
                have.get(&id).copied().unwrap_or(0)
            };
            if !full && known == len {
                continue; // unchanged segment appears only in `order`
            }
            // A trimmed/fixed segment is resent in full; otherwise append from `known`.
            let from = if known < len { known } else { 0 };
            let pts = seg[from..]
                .iter()
                .map(|p| [p.x, p.y, p.z, p.spd, p.temp, p.surf as f32])
                .collect();
            changed.push(PathDeltaSeg { id, from, pts });
        }

        PathDeltaDto {
            world_id: WORLD_ID.to_string(),
            epoch: self.epoch,
            points: self.path_len,
            loops_locked: self.loops_locked(),
            full,
            order,
            changed,
        }
    }

    /// Convert the sparse grid into the wire DTO.
    pub fn to_heightmap(&self) -> WorldHeightmap {
        let mut cells = Vec::with_capacity(self.cells.len());
        let (mut min_cx, mut min_cz) = (i32::MAX, i32::MAX);
        let (mut max_cx, mut max_cz) = (i32::MIN, i32::MIN);

        for (&(cx, cz), acc) in &self.cells {
            if acc.n == 0 {
                continue;
            }
            min_cx = min_cx.min(cx);
            min_cz = min_cz.min(cz);
            max_cx = max_cx.max(cx);
            max_cz = max_cz.max(cz);
            cells.push(cell_to_height(cx, cz, acc));
        }

        let (origin, cols, rows) = if cells.is_empty() {
            ([0.0, 0.0], 0, 0)
        } else {
            (
                [min_cx as f32 * self.cell_m, min_cz as f32 * self.cell_m],
                (max_cx - min_cx + 1) as u32,
                (max_cz - min_cz + 1) as u32,
            )
        };

        WorldHeightmap {
            client_version: client_version(),
            world_id: WORLD_ID.to_string(),
            cell_size_m: self.cell_m,
            origin,
            cols,
            rows,
            cells,
        }
    }

    pub fn stats(&self) -> WorldMapStats {
        let hm = self.to_heightmap();
        let samples = hm.cells.iter().map(|c| c.n as u64).sum();
        WorldMapStats {
            world_id: hm.world_id,
            cells: hm.cells.len() as u32,
            samples,
            cols: hm.cols,
            rows: hm.rows,
        }
    }

    pub fn reset(&mut self) {
        self.cells.clear();
        self.last_xz = None;
        self.last_pos = None;
        self.path.clear();
        self.seg_ids.clear();
        // `next_seg_id` stays monotonic and never reuses IDs; epoch++ forces a full viewer update.
        self.epoch = self.epoch.wrapping_add(1);
        self.seg_open = false;
        self.path_len = 0;
        self.last_path_xz = None;
        self.path_cells.clear();
        // Clear persistence queues and cursors. The changed epoch makes the next
        // autosave truncate logs; see `drain_persist_delta`.
        self.pending_cells.clear();
        self.seg_saved.clear();
        self.dirty = true;
    }

    /// Return `true` and clear the flag when state changed since the last flush.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Seed startup state from the JSON heightmap/route snapshots plus replayed
    /// `.ndjson` logs written after the latest snapshot for crash recovery.
    /// Everything loaded from disk is already persistent, so queues and cursors
    /// reset and autosave appends only later changes.
    pub fn load_seed(&mut self, path: &Path) {
        if let Some(hm) = read_heightmap(path) {
            // Adopt the file's cell edge when it contains data, avoiding mixed
            // grid resolutions if the environment changed between sessions.
            if !hm.cells.is_empty() && hm.cell_size_m.is_finite() && hm.cell_size_m > 0.0 {
                self.cell_m = hm.cell_size_m;
            }
            self.merge_from(&hm);
        }
        for c in read_cells_log(&cells_log_path(path)) {
            self.merge_cell(&c);
        }

        // The route ribbon lives in companion snapshot and log files.
        self.load_path_seed(&companion_path(path));
        self.load_path_log(&path_log_path(path));

        self.pending_cells.clear();
        self.seg_saved = self.path.iter().map(|s| s.len()).collect();
        self.persist_epoch = self.epoch;
        self.dirty = false;
    }

    /// Restore the ordered route from its snapshot and rebuild `path_cells`
    /// coverage. Cell deduplication prevents revisits from duplicating points.
    pub fn load_path_seed(&mut self, path: &Path) {
        let Some(dto) = read_path(path) else {
            return;
        };
        if dto.segments.is_empty() {
            return;
        }
        self.path.clear();
        self.seg_ids.clear();
        self.path_cells.clear();
        self.path_len = 0;
        for seg in &dto.segments {
            self.push_loaded_segment(seg);
        }
        self.seg_open = false;
        self.last_path_xz = None;
        self.dirty = false;
    }

    /// Apply incremental route points recorded after the snapshot. Consecutive
    /// points with the same `segment_id` form a segment. Log IDs only group data;
    /// fresh IDs here avoid collisions with snapshot IDs.
    fn load_path_log(&mut self, path: &Path) {
        let pts = read_path_log(path);
        if pts.is_empty() {
            return;
        }
        let mut cur_src: Option<u64> = None;
        let mut seg: Vec<[f32; 6]> = Vec::new();
        for (src_id, p) in pts {
            if cur_src != Some(src_id) {
                self.push_loaded_segment(&seg);
                seg = Vec::new();
                cur_src = Some(src_id);
            }
            seg.push(p);
        }
        self.push_loaded_segment(&seg);
        self.seg_open = false;
        self.last_path_xz = None;
    }

    /// Append a loaded segment with a fresh ID, update `path_len`, and mark its
    /// `path_cells` coverage. Ignore segments with fewer than two points.
    fn push_loaded_segment(&mut self, seg: &[[f32; 6]]) {
        if seg.len() < 2 {
            return;
        }
        let pts: Vec<PathPoint> = seg
            .iter()
            .map(|a| PathPoint {
                x: a[0],
                y: a[1],
                z: a[2],
                spd: a[3],
                temp: a[4],
                surf: a[5] as u8,
            })
            .collect();
        for p in &pts {
            self.path_cells
                .insert((self.cell_index(p.x), self.cell_index(p.z)));
        }
        self.path_len += pts.len();
        self.path.push(pts);
        self.seg_ids.push(self.next_seg_id);
        self.next_seg_id = self.next_seg_id.wrapping_add(1);
    }

    /// Merge one saved cell into the in-memory accumulator.
    fn merge_cell(&mut self, c: &HeightCell) {
        if c.n == 0 {
            return;
        }
        let e = self.cells.entry((c.cx, c.cz)).or_default();
        // Reconstruct the sum from the saved mean, exact when mean = sum / n.
        e.sum_y += c.y_mean as f64 * c.n as f64;
        e.sum_spd += c.spd_mean as f64 * c.n as f64;
        e.sum_temp += c.temp_mean as f64 * c.n as f64;
        e.min_y = e.min_y.min(c.y_min);
        e.max_y = e.max_y.max(c.y_max);
        e.n += c.n;
        for k in 0..SURFACE_KINDS {
            e.surf[k] = e.surf[k].saturating_add(c.surf[k]);
        }
    }

    /// Drain new cells and points since the last persistence flush. When the
    /// epoch differs after reset, request truncation and return all state as the
    /// new base. Autosave calls this under the lock, then serializes and writes
    /// outside it. Return `None` when nothing changed.
    pub fn drain_persist_delta(&mut self) -> Option<PersistDelta> {
        if self.persist_epoch != self.epoch {
            self.persist_epoch = self.epoch;
            self.pending_cells.clear();
            let cells: Vec<HeightCell> = self
                .cells
                .iter()
                .filter(|(_, a)| a.n > 0)
                .map(|(&(cx, cz), a)| cell_to_height(cx, cz, a))
                .collect();
            let mut path = Vec::new();
            for (seg, &id) in self.path.iter().zip(self.seg_ids.iter()) {
                for p in seg {
                    path.push((id, [p.x, p.y, p.z, p.spd, p.temp, p.surf as f32]));
                }
            }
            self.seg_saved = self.path.iter().map(|s| s.len()).collect();
            return Some(PersistDelta {
                reset: true,
                cells,
                path,
            });
        }

        let mut cells = Vec::new();
        for key in self.pending_cells.drain(..) {
            if let Some(acc) = self.cells.get(&key) {
                if acc.n > 0 {
                    cells.push(cell_to_height(key.0, key.1, acc));
                }
            }
        }

        if self.seg_saved.len() < self.path.len() {
            self.seg_saved.resize(self.path.len(), 0);
        }
        let mut path = Vec::new();
        for i in 0..self.path.len() {
            let saved = self.seg_saved[i];
            let len = self.path[i].len();
            if saved < len {
                let id = self.seg_ids[i];
                for p in &self.path[i][saved..] {
                    path.push((id, [p.x, p.y, p.z, p.spd, p.temp, p.surf as f32]));
                }
                self.seg_saved[i] = len;
            }
        }

        if cells.is_empty() && path.is_empty() {
            None
        } else {
            Some(PersistDelta {
                reset: false,
                cells,
                path,
            })
        }
    }

    /// Compact disk state by rewriting complete heightmap and route JSON snapshots
    /// and truncating incremental logs. Shutdown and `/fix` call this. It closes
    /// the active segment so the next session starts on a new line.
    pub fn compact(&mut self, path: &Path) -> std::io::Result<()> {
        self.save(path)?;
        self.save_path(path)?;
        std::fs::write(cells_log_path(path), b"")?;
        std::fs::write(path_log_path(path), b"")?;
        self.pending_cells.clear();
        self.seg_saved = self.path.iter().map(|s| s.len()).collect();
        self.persist_epoch = self.epoch;
        self.break_segment();
        Ok(())
    }

    /// Overwrite the heightmap cell snapshot with current state.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(&self.to_heightmap())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_atomic(path, &json)
    }

    /// Overwrite the route snapshot in `path`'s companion file.
    pub fn save_path(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(&self.to_path())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_atomic(&companion_path(path), &json)
    }
}

/// Build a wire cell from the in-memory accumulator.
fn cell_to_height(cx: i32, cz: i32, acc: &CellAcc) -> HeightCell {
    HeightCell {
        cx,
        cz,
        y_mean: acc.mean(),
        y_min: acc.min_y,
        y_max: acc.max_y,
        n: acc.n,
        spd_mean: acc.spd_mean(),
        temp_mean: acc.temp_mean(),
        surf: acc.surf,
    }
}

fn read_heightmap(path: &Path) -> Option<WorldHeightmap> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn read_path(path: &Path) -> Option<PathDto> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Incremental heightmap log: `worldmap-fh6.json` → `worldmap-fh6.ndjson`.
fn cells_log_path(hm_path: &Path) -> PathBuf {
    hm_path.with_file_name(format!("worldmap-{WORLD_ID}.ndjson"))
}

/// Incremental route log beside the snapshot: `worldpath-fh6.ndjson`.
fn path_log_path(hm_path: &Path) -> PathBuf {
    hm_path.with_file_name(format!("worldpath-{WORLD_ID}.ndjson"))
}

/// Read an NDJSON log while ignoring malformed lines, such as a final line
/// truncated by a crash during writing. A missing file yields an empty result.
fn read_cells_log(path: &Path) -> Vec<HeightCell> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<HeightCell>(l).ok())
        .collect()
}

fn read_path_log(path: &Path) -> Vec<(u64, [f32; 6])> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<(u64, [f32; 6])>(l).ok())
        .collect()
}

/// Append NDJSON lines, creating the file when needed. Each flush is O(new data)
/// instead of rewriting the entire map, so it scales to the complete FH6 world.
fn append_lines(path: &Path, lines: &[String]) -> std::io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    let mut buf = String::with_capacity(lines.len() * 64);
    for l in lines {
        buf.push_str(l);
        buf.push('\n');
    }
    f.write_all(buf.as_bytes())
}

/// Write atomically through a `.tmp` file and rename, so termination during a
/// write cannot leave a large heightmap truncated or corrupt. `rename` replaces
/// the existing destination on Windows and Unix.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Route companion file beside the heightmap:
/// `worldmap-fh6.json` → `worldpath-fh6.json`.
fn companion_path(hm_path: &Path) -> PathBuf {
    hm_path.with_file_name(format!("worldpath-{WORLD_ID}.json"))
}

/// Default persistence path: `%APPDATA%/forza-hud-native/worldmap-fh6.json`,
/// the same directory the overlay uses for `layout.txt` and `theme.txt`.
#[must_use]
pub fn default_path() -> PathBuf {
    crate::paths::config_dir().join(format!("worldmap-{WORLD_ID}.json"))
}

/// Start incremental autosave. Every `AUTOSAVE_SECS`, drain only new cells and
/// points and append them to NDJSON logs in O(new data), rather than rewriting
/// the complete FH6 world. Shutdown compacts to JSON snapshots and truncates the
/// logs. Only the overlay's single owning process may call this. Serialization
/// runs outside the lock.
pub fn spawn_autosave(rec: Arc<Mutex<WorldMapRecorder>>, path: PathBuf) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("worldmap-autosave".into())
        .spawn(move || {
            let mut tick: u64 = 0;
            loop {
                thread::sleep(Duration::from_secs(AUTOSAVE_SECS));
                tick += 1;
                let delta = {
                    let mut g = rec.lock().unwrap();
                    g.drain_persist_delta()
                };
                if let Some(delta) = delta {
                    // An epoch change means reset: truncate logs before writing the new base.
                    if delta.reset {
                        let _ = std::fs::write(cells_log_path(&path), b"");
                        let _ = std::fs::write(path_log_path(&path), b"");
                    }

                    let cell_lines: Vec<String> = delta
                        .cells
                        .iter()
                        .filter_map(|c| serde_json::to_string(c).ok())
                        .collect();
                    let path_lines: Vec<String> = delta
                        .path
                        .iter()
                        .filter_map(|p| serde_json::to_string(p).ok())
                        .collect();
                    let _ = append_lines(&cells_log_path(&path), &cell_lines);
                    let _ = append_lines(&path_log_path(&path), &path_lines);
                }

                // Periodic full JSON snapshots keep the database recoverable even
                // when the HUD does not shut down through Ctrl+Alt+Q.
                if tick % COMPACT_EVERY_TICKS == 0 {
                    let mut g = rec.lock().unwrap();
                    if !g.cells.is_empty() || g.path_points() > 0 {
                        let _ = g.compact(&path);
                    }
                }
            }
        })
        .expect("thread worldmap-autosave")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dash_pos(x: f32, y: f32, z: f32) -> Telemetry {
        Telemetry {
            sled_only: 0,
            race: 1,
            pos: Some([x, y, z]),
            ..Default::default()
        }
    }

    /// Like `dash_pos`, with uniform per-wheel rumble and puddle signals.
    fn dash_surf(x: f32, y: f32, z: f32, rumble: f32, puddle: f32) -> Telemetry {
        Telemetry {
            surf_rumble: Some([rumble; 4]),
            puddle: Some([puddle; 4]),
            ..dash_pos(x, y, z)
        }
    }

    /// Deterministic recorder with eight-metre cells for indexing tests.
    fn rec() -> WorldMapRecorder {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        r
    }

    #[test]
    fn indexes_a_cell_and_records_height() {
        let mut r = rec();
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        let hm = r.to_heightmap();
        assert_eq!(hm.cells.len(), 1);
        let c = &hm.cells[0];
        assert_eq!((c.cx, c.cz), (0, 0));
        assert_eq!(c.y_mean, 100.0);
        assert_eq!(c.n, 1);
        assert_eq!(hm.cell_size_m, 8.0);
        assert_eq!(hm.world_id, WORLD_ID);
    }

    #[test]
    fn positions_map_to_distinct_cells() {
        let mut r = rec();
        r.ingest(&dash_pos(0.0, 10.0, 0.0), true);
        r.ingest(&dash_pos(100.0, 20.0, 0.0), true); // ~100 m reaches another cell
                                                     // A 100 m jump exceeds MAX_JUMP, so reanchor;
                                                     // moving another 5 m then records.
        r.ingest(&dash_pos(105.0, 21.0, 0.0), true);
        let hm = r.to_heightmap();
        // Cell (0,0) from the first sample and approximately cell 13 from the third.
        assert_eq!(hm.cells.iter().map(|c| c.n).sum::<u32>(), 2);
        assert_eq!(hm.cells.len(), 2);
    }

    #[test]
    fn an_existing_cell_is_not_recorded_again() {
        let mut r = rec();
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        r.ingest(&dash_pos(2.0, 110.0, 0.0), true); // same cell (cx=0), moved 2 m
        let hm = r.to_heightmap();
        assert_eq!(hm.cells.len(), 1);
        let c = &hm.cells[0];
        assert_eq!(c.n, 1);
        assert_eq!(c.y_mean, 100.0);
    }

    #[test]
    fn negligible_movement_does_not_resample() {
        let mut r = WorldMapRecorder::new();
        r.set_recording(true);
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        r.ingest(&dash_pos(0.02, 999.0, 0.0), true); // below MIN_MOVE_M (5 cm), ignored
        let hm = r.to_heightmap();
        assert_eq!(hm.cells[0].n, 1);
        assert_eq!(hm.cells[0].y_mean, 100.0);
    }

    #[test]
    fn a_teleport_is_discarded() {
        let mut r = WorldMapRecorder::new();
        r.set_recording(true);
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        r.ingest(&dash_pos(5000.0, 7.0, 5000.0), true); // discard fast travel
        let hm = r.to_heightmap();
        assert_eq!(hm.cells.len(), 1); // only the first sample
        assert_eq!(hm.cells[0].cx, 0);
    }

    #[test]
    fn menu_and_sled_packets_do_not_record() {
        let mut r = WorldMapRecorder::new();
        r.ingest(&dash_pos(0.0, 100.0, 0.0), false); // inactive
        let mut sled = dash_pos(10.0, 100.0, 0.0);
        sled.sled_only = 1;
        r.ingest(&sled, true);
        assert_eq!(r.to_heightmap().cells.len(), 0);
    }

    #[test]
    fn classifies_surface_per_cell() {
        let mut r = rec();
        // Smooth asphalt first, then move to a medium-rumble dirt surface.
        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), true);
        r.ingest(&dash_surf(2.0, 10.0, 0.0, 0.01, 0.0), true); // same cell, no remark
        r.ingest(&dash_surf(100.0, 10.0, 0.0, 0.20, 0.0), true); // jump and reanchor
        r.ingest(&dash_surf(105.0, 10.0, 0.0, 0.20, 0.0), true); // another dirt cell
        let hm = r.to_heightmap();
        let asphalt = hm.cells.iter().find(|c| c.cx == 0).unwrap();
        assert_eq!(asphalt.surf[surface::ASPHALT], 1);
        assert_eq!(asphalt.dominant_surface(), surface::ASPHALT as u8);
        let dirt = hm.cells.iter().find(|c| c.cx != 0).unwrap();
        assert_eq!(dirt.surf[surface::DIRT], 1);
    }

    #[test]
    fn a_continuous_circuit_lap_in_one_segment_does_not_duplicate() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        let n = 36usize;
        let radius = 55.0f32;
        let mut after_lap1 = 0usize;

        for lap in 0..2 {
            for i in 0..n {
                let ang = std::f32::consts::TAU * i as f32 / n as f32;
                let x = radius * ang.cos();
                let z = radius * ang.sin();
                r.ingest(&dash_surf(x, 10.0, z, 0.01, 0.0), true);
            }
            if lap == 0 {
                after_lap1 = r.to_path().points;
                assert!(
                    after_lap1 >= 20,
                    "the first lap should record the circuit: {after_lap1}"
                );
            }
        }
        assert_eq!(
            r.to_path().points,
            after_lap1,
            "a continuous second lap in the same segment must not record again"
        );
    }

    #[test]
    fn a_new_ribbon_appears_only_far_from_a_segment_endpoint() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let base = r.to_path().points;

        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), false);
        // Five metres parallel and near the endpoint does not create a ribbon.
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 5.0, 0.01, 0.0), true);
        }
        assert_eq!(r.to_path().points, base);

        r.ingest(&dash_surf(0.0, 10.0, 5.0, 0.01, 0.0), false);
        // Thirty-two metres parallel is far from the ribbon and its endpoints.
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 32.0, 0.01, 0.0), true);
        }
        assert!(
            r.to_path().points > base,
            "a separate new ribbon should record"
        );
    }

    #[test]
    fn a_second_lap_with_lateral_offset_does_not_duplicate() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..25 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let pts1 = r.to_path().points;

        // Second lap about three metres aside, a typical lap-to-lap offset.
        for i in 0..25 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 3.0, 0.01, 0.0), true);
        }
        assert_eq!(
            r.to_path().points,
            pts1,
            "a lateral offset inside the halo must not duplicate the route"
        );
    }

    #[test]
    fn a_continuous_route_does_not_fragment_or_rerecord() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        // First pass: one continuous straight line.
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let p1 = r.to_path();
        assert_eq!(
            p1.segments.len(),
            1,
            "continuous travel should produce one segment"
        );
        let pts1 = p1.points;
        assert!(pts1 >= 18, "most points should be recorded: {pts1}");

        // Stop, close the segment, and drive the exact path again. The ribbon
        // already covers this road, so the route must not become thicker.
        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), false); // inactive, so break
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let p2 = r.to_path();
        assert_eq!(p2.segments.len(), 1);
        assert_eq!(p2.points, pts1, "a second pass must not thicken the ribbon");
    }

    #[test]
    fn returning_in_the_same_segment_does_not_rerecord() {
        // A U-turn without stopping: the outbound leg already covered the road,
        // so the return must not create another ribbon on top of it.
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..30 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let out = r.to_path().points;
        for i in (0..29).rev() {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let back = r.to_path().points;
        assert_eq!(
            back, out,
            "returning over covered road must not rerecord: out={out} back={back}"
        );
    }

    #[test]
    fn nearby_lateral_pass_does_not_thicken_but_a_parallel_road_records() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let base = r.to_path().points;

        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), false);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 5.0, 0.01, 0.0), true);
        }
        assert_eq!(
            r.to_path().points,
            base,
            "the other side of the same road must not widen the ribbon"
        );

        r.ingest(&dash_surf(0.0, 10.0, 5.0, 0.01, 0.0), false);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 32.0, 0.01, 0.0), true);
        }
        let parallel = r.to_path();
        assert!(
            parallel.points > base,
            "a separate parallel road must continue recording"
        );
        assert_eq!(parallel.segments.len(), 2);
    }

    #[test]
    fn aggregates_mean_speed_per_cell() {
        let mut r = rec();
        let mut a = dash_pos(0.0, 10.0, 0.0);
        a.speed = Some(80.0);
        r.ingest(&a, true);
        r.ingest(&dash_pos(100.0, 10.0, 0.0), true); // jump and reanchor
        let mut c = dash_pos(105.0, 10.0, 0.0);
        c.speed = Some(120.0);
        r.ingest(&c, true);
        let hm = r.to_heightmap();
        let fast = hm.cells.iter().find(|c| c.cx != 0).unwrap();
        assert_eq!(fast.n, 1);
        assert_eq!(fast.spd_mean, 120.0);
    }

    #[test]
    fn aggregates_mean_temperature_per_cell() {
        let mut r = rec();
        let mut a = dash_pos(0.0, 10.0, 0.0);
        a.tires = Some([160.0, 170.0, 180.0, 190.0]);
        r.ingest(&a, true);
        r.ingest(&dash_pos(100.0, 10.0, 0.0), true);
        let mut c = dash_pos(105.0, 10.0, 0.0);
        c.tires = Some([200.0, 210.0, 220.0, 230.0]);
        r.ingest(&c, true);
        let hm = r.to_heightmap();
        let hot = hm.cells.iter().find(|c| c.cx != 0).unwrap();
        assert_eq!(hot.n, 1);
        assert_eq!(hot.temp_mean, 215.0);
    }

    #[test]
    fn water_takes_priority_over_rumble() {
        // A deep puddle remains water even with high rumble.
        let mut r = rec();
        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.9, 0.5), true);
        let hm = r.to_heightmap();
        assert_eq!(hm.cells[0].surf[surface::WATER], 1);
    }

    #[test]
    fn missing_surface_signal_is_unknown() {
        let mut r = rec();
        r.ingest(&dash_pos(0.0, 10.0, 0.0), true); // surf_rumble None
        let hm = r.to_heightmap();
        assert_eq!(hm.cells[0].surf[surface::UNKNOWN], 1);
    }

    #[test]
    fn merge_combines_and_sums_cells() {
        let mut a = WorldMapRecorder::new();
        a.set_recording(true);
        a.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        let hm = a.to_heightmap();

        let mut b = WorldMapRecorder::new();
        b.set_recording(true);
        b.ingest(&dash_pos(0.0, 200.0, 0.0), true); // same cell (0,0)
        b.merge_from(&hm);

        let out = b.to_heightmap();
        assert_eq!(out.cells.len(), 1);
        assert_eq!(out.cells[0].n, 2);
        assert_eq!(out.cells[0].y_mean, 150.0); // (200 + 100) / 2
    }

    #[test]
    fn round_trip_save_load() {
        let mut r = WorldMapRecorder::new();
        r.set_recording(true);
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        r.ingest(&dash_pos(50.0, 120.0, 0.0), true);
        let original = r.to_heightmap();

        let dir = std::env::temp_dir().join(format!("vry-wm-{}", std::process::id()));
        let path = dir.join("worldmap-test.json");
        r.save(&path).unwrap();

        let mut loaded = WorldMapRecorder::new();
        loaded.load_seed(&path);
        let back = loaded.to_heightmap();

        assert_eq!(original.cells.len(), back.cells.len());
        assert_eq!(original.cols, back.cols);
        assert_eq!(original.rows, back.rows);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn take_dirty_toggles_after_consumption() {
        let mut r = WorldMapRecorder::new();
        r.set_recording(true);
        assert!(!r.take_dirty()); // clean initially
        r.ingest(&dash_pos(0.0, 100.0, 0.0), true);
        assert!(r.take_dirty());
        assert!(!r.take_dirty()); // already consumed
    }

    #[test]
    fn disabled_recording_keeps_position_without_creating_a_cell() {
        let mut r = WorldMapRecorder::new();
        r.ingest(&dash_pos(10.0, 50.0, 20.0), true);
        assert_eq!(r.to_heightmap().cells.len(), 0);
        assert_eq!(r.last_pos(), Some([10.0, 50.0, 20.0]));
        r.set_recording(true);
        r.ingest(&dash_pos(12.0, 51.0, 20.0), true);
        assert_eq!(r.to_heightmap().cells.len(), 1);
    }

    #[test]
    fn path_delta_is_full_then_incremental() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..10 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }

        // First fetch: epochs match but empty `have` requests everything from zero.
        let d0 = r.to_path_delta(0, &HashMap::new());
        assert!(!d0.full);
        assert_eq!(d0.order.len(), 1);
        assert_eq!(d0.changed.len(), 1);
        assert_eq!(d0.changed[0].from, 0);
        let id = d0.order[0];
        let len0 = d0.changed[0].pts.len();
        assert!(
            len0 >= 5,
            "the first fetch should include the segment: {len0}"
        );

        // Drive farther. The client has `len0` points, so the delta appends only new ones.
        for i in 10..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let mut have = HashMap::new();
        have.insert(id, len0);
        let d1 = r.to_path_delta(d0.epoch, &have);
        assert_eq!(d1.order, vec![id]);
        assert_eq!(d1.changed.len(), 1);
        assert_eq!(
            d1.changed[0].from, len0,
            "the delta should append from the client's known length"
        );
        assert!(d1.changed[0].pts.len() >= 5);

        // With nothing new since d1, an unchanged segment resends no points.
        let total = len0 + d1.changed[0].pts.len();
        have.insert(id, total);
        let d2 = r.to_path_delta(d1.epoch, &have);
        assert_eq!(d2.order, vec![id]);
        assert!(
            d2.changed.is_empty(),
            "an unchanged segment must not be resent"
        );
    }

    #[test]
    fn a_new_delta_segment_gets_its_own_id() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..6 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let d0 = r.to_path_delta(0, &HashMap::new());
        let id0 = d0.order[0];
        let len0 = d0.changed[0].pts.len();

        // Break while inactive, then drive a separate parallel road into a new segment.
        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), false);
        for i in 0..6 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 60.0, 0.01, 0.0), true);
        }

        let mut have = HashMap::new();
        have.insert(id0, len0);
        let d1 = r.to_path_delta(d0.epoch, &have);
        assert_eq!(d1.order.len(), 2, "there should be two segments");
        assert_eq!(d1.order[0], id0);
        assert_ne!(d1.order[1], id0, "the new segment has a distinct ID");
        // Only the new segment changes; the unchanged old one stays out of `changed`.
        assert_eq!(d1.changed.len(), 1);
        assert_eq!(d1.changed[0].id, d1.order[1]);
        assert_eq!(d1.changed[0].from, 0);
    }

    #[test]
    fn reset_increments_the_delta_epoch_and_forces_full() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..6 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let d0 = r.to_path_delta(0, &HashMap::new());
        let e0 = d0.epoch;

        r.reset();
        // A client on the old epoch receives `full=true` and an empty route.
        let d1 = r.to_path_delta(e0, &HashMap::new());
        assert!(d1.full, "an epoch mismatch should request a full update");
        assert_ne!(d1.epoch, e0, "reset should increment the epoch");
        assert!(d1.order.is_empty());
        assert!(d1.changed.is_empty());
    }

    #[test]
    fn the_route_persists_and_a_restored_base_does_not_duplicate() {
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let before = r.to_path().points;
        assert!(before >= 15, "the baseline should record: {before}");

        let dir = std::env::temp_dir().join(format!("vry-wp-{}", std::process::id()));
        let hm_path = dir.join("worldmap-test.json");
        r.save(&hm_path).unwrap();
        r.save_path(&hm_path).unwrap();

        // A new recorder simulates restart; `load_seed` restores cells and route.
        let mut loaded = WorldMapRecorder::with_cell_size(8.0);
        loaded.load_seed(&hm_path);
        assert_eq!(
            loaded.to_path().points,
            before,
            "the route should survive restart"
        );

        // Driving over the restored base must not duplicate the ribbon.
        loaded.set_recording(true);
        for i in 0..20 {
            loaded.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        assert_eq!(
            loaded.to_path().points,
            before,
            "the restored base is stable and must not rerecord over itself"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_ribbon_keeps_growing_in_new_territory_after_a_break() {
        // Regression: after an inactive break, driving into new territory must
        // continue growing the ribbon.
        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let p1 = r.to_path().points;
        assert!(p1 >= 15, "the first pass should record: {p1}");

        // Pause through an inactive menu/respawn break, then continue onto new road.
        r.ingest(&dash_surf(0.0, 10.0, 0.0, 0.01, 0.0), false);
        for i in 20..40 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let p2 = r.to_path().points;
        assert!(
            p2 > p1,
            "the ribbon should keep growing in new territory: p1={p1} p2={p2}"
        );
    }

    #[test]
    fn incremental_persistence_recovers_from_logs() {
        // Crash recovery without a JSON snapshot: autosave's incremental NDJSON
        // logs alone must rebuild cells and route at startup.
        let dir = std::env::temp_dir().join(format!("vry-log-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let hm_path = dir.join("worldmap-test.json");

        let mut r = WorldMapRecorder::with_cell_size(8.0);
        r.set_recording(true);
        for i in 0..20 {
            r.ingest(&dash_surf(i as f32 * 8.0, 10.0, 0.0, 0.01, 0.0), true);
        }
        let pts = r.to_path().points;
        let cells = r.stats().cells;
        assert!(pts >= 15 && cells >= 15, "base: pts={pts} cells={cells}");

        // Simulate one autosave tick: drain and append the delta without compaction.
        let delta = r.drain_persist_delta().expect("a non-empty delta");
        assert!(
            !delta.reset,
            "the first drain of a fresh session is incremental"
        );
        let cell_lines: Vec<String> = delta
            .cells
            .iter()
            .filter_map(|c| serde_json::to_string(c).ok())
            .collect();
        let path_lines: Vec<String> = delta
            .path
            .iter()
            .filter_map(|p| serde_json::to_string(p).ok())
            .collect();
        append_lines(&cells_log_path(&hm_path), &cell_lines).unwrap();
        append_lines(&path_log_path(&hm_path), &path_lines).unwrap();

        // Crash restart: only logs exist, with no snapshot.
        let mut loaded = WorldMapRecorder::with_cell_size(8.0);
        loaded.load_seed(&hm_path);
        assert_eq!(
            loaded.to_path().points,
            pts,
            "the route is recovered from logs"
        );
        assert_eq!(loaded.stats().cells, cells, "cells are recovered from logs");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
