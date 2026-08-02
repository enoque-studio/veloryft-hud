//! Serialization contract for Veloryft world-map data.
//!
//! These types cross a process boundary: the HUD produces them while the player
//! drives, the local viewer API serves them, and the Veloryft backend consumes
//! them on upload. They exist as their own crate so that producer and consumer
//! can agree on one definition without either repository depending on the other.
//!
//! # Compatibility rules
//!
//! Every field is `#[serde(default)]`-tolerant where it was added after the
//! first release, so an older map file or an older client still deserializes.
//! Adding a field is a minor version bump; removing or renaming one, or changing
//! the meaning of an existing field, is a major version bump. Do not reorder
//! [`SURFACE_KINDS`] entries — the index is the wire value.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Number of recognized surface classes. Each cell keeps a
/// `[u32; SURFACE_KINDS]` histogram of how many samples fell into each class;
/// the dominant class (argmax) drives the color in the viewer.
///
/// | index | class          | inferred from                                        |
/// |-------|----------------|------------------------------------------------------|
/// | 0     | Unknown        | no surface signal in the packet                      |
/// | 1     | Asphalt / road | `surface_rumble` near zero (smooth)                  |
/// | 2     | Dirt / trail   | moderate `surface_rumble`                            |
/// | 3     | Grass / rough  | high `surface_rumble`                                |
/// | 4     | Water / wet    | `wheel_in_puddle_depth` above the threshold (wins)   |
///
/// Forza does not label the surface in its UDP packets. This is a tunable
/// heuristic derived from `SurfaceRumble` and `WheelInPuddleDepth`; the
/// classification itself lives in the HUD, and this crate only carries the
/// contract and the merge rule.
pub const SURFACE_KINDS: usize = 5;

/// Sparse-grid elevation map of a world.
///
/// The HUD samples the telemetry position `[x, y, z]` while the player drives
/// and aggregates height `y` per `(cx, cz)` cell of fixed size. Only *visited*
/// cells are stored — this is the driven network plus the heights observed
/// along it, not a complete terrain mesh.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct WorldHeightmap {
    /// Version of the client that produced this map.
    pub client_version: String,
    /// World identifier. `"fh6"` for the single open world supported today.
    pub world_id: String,
    /// Cell edge length in meters (the world is divided into a square grid).
    pub cell_size_m: f32,
    /// Lower corner `(min_x, min_z)` of the driven bounding box, which makes
    /// each cell's absolute coordinate recoverable: `x = origin[0] + cx * cell`.
    pub origin: [f32; 2],
    /// Bounding-box width in cells, along the x axis.
    pub cols: u32,
    /// Bounding-box height in cells, along the z axis.
    pub rows: u32,
    /// Sparse cells — only the visited ones.
    pub cells: Vec<HeightCell>,
}

/// One aggregated cell of the heightmap.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct HeightCell {
    /// Grid index on the x axis (`floor(x / cell_size_m)`).
    pub cx: i32,
    /// Grid index on the z axis (`floor(z / cell_size_m)`).
    pub cz: i32,
    /// Mean height of the samples in this cell.
    pub y_mean: f32,
    pub y_min: f32,
    pub y_max: f32,
    /// Number of aggregated samples — the cell's weight and confidence.
    pub n: u32,
    /// Mean speed (km/h) of the samples in this cell. Merges as an `n`-weighted
    /// mean, like `y_mean`. Older maps without the field arrive as 0.
    #[serde(default)]
    pub spd_mean: f32,
    /// Mean tire temperature (°C) of the samples in this cell. The HUD averages
    /// the four tires; merges as an `n`-weighted mean.
    #[serde(default)]
    pub temp_mean: f32,
    /// Surface histogram: `surf[k]` counts samples classified as class `k` (see
    /// [`SURFACE_KINDS`]). The sum usually matches `n` but is not required to —
    /// older maps arrive zeroed and count as Unknown. Merges element-wise.
    #[serde(default)]
    pub surf: [u32; SURFACE_KINDS],
}

impl HeightCell {
    /// Dominant surface class (index of the largest histogram bucket).
    /// Returns `0` (Unknown) when nothing has been classified.
    #[must_use]
    pub fn dominant_surface(&self) -> u8 {
        let mut best = 0_usize;
        let mut best_count = 0_u32;
        for (kind, &count) in self.surf.iter().enumerate() {
            if count > best_count {
                best_count = count;
                best = kind;
            }
        }
        best as u8
    }
}

/// Response body of the backend's world-map upload endpoint.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct WorldMapAccepted {
    pub accepted: bool,
    pub world_id: String,
    /// Total cells in the server's accumulated map after the merge.
    pub cells_total: u32,
    /// Populated when `accepted == false`.
    #[serde(default)]
    pub rejected_reason: Option<String>,
}

impl WorldHeightmap {
    /// Merges `other` into this map cell by cell: `n`-weighted means for the
    /// averaged fields, min/max recombined, histograms summed. This is the one
    /// rule both the HUD and the backend use when accumulating sessions, and it
    /// recomputes `origin`, `cols`, and `rows`.
    pub fn merge(&mut self, other: &WorldHeightmap) {
        use std::collections::HashMap;

        let mut index: HashMap<(i32, i32), usize> = self
            .cells
            .iter()
            .enumerate()
            .map(|(position, cell)| ((cell.cx, cell.cz), position))
            .collect();

        for incoming in &other.cells {
            if incoming.n == 0 {
                continue;
            }
            match index.get(&(incoming.cx, incoming.cz)) {
                Some(&position) => {
                    let cell = &mut self.cells[position];
                    let total = cell.n + incoming.n;
                    cell.y_mean = weighted_mean(cell.y_mean, cell.n, incoming.y_mean, incoming.n);
                    cell.spd_mean =
                        weighted_mean(cell.spd_mean, cell.n, incoming.spd_mean, incoming.n);
                    cell.temp_mean =
                        weighted_mean(cell.temp_mean, cell.n, incoming.temp_mean, incoming.n);
                    cell.y_min = cell.y_min.min(incoming.y_min);
                    cell.y_max = cell.y_max.max(incoming.y_max);
                    cell.n = total;
                    for kind in 0..SURFACE_KINDS {
                        cell.surf[kind] = cell.surf[kind].saturating_add(incoming.surf[kind]);
                    }
                }
                None => {
                    index.insert((incoming.cx, incoming.cz), self.cells.len());
                    self.cells.push(incoming.clone());
                }
            }
        }

        if self.cell_size_m == 0.0 {
            self.cell_size_m = other.cell_size_m;
        }
        if self.world_id.is_empty() {
            self.world_id.clone_from(&other.world_id);
        }
        if self.client_version.is_empty() {
            self.client_version.clone_from(&other.client_version);
        }
        self.recompute_extent();
    }

    /// Recomputes `origin`, `cols`, and `rows` from the cells present.
    pub fn recompute_extent(&mut self) {
        let (mut min_cx, mut min_cz) = (i32::MAX, i32::MAX);
        let (mut max_cx, mut max_cz) = (i32::MIN, i32::MIN);
        for cell in &self.cells {
            min_cx = min_cx.min(cell.cx);
            min_cz = min_cz.min(cell.cz);
            max_cx = max_cx.max(cell.cx);
            max_cz = max_cz.max(cell.cz);
        }
        if self.cells.is_empty() {
            self.origin = [0.0, 0.0];
            self.cols = 0;
            self.rows = 0;
        } else {
            self.origin = [
                min_cx as f32 * self.cell_size_m,
                min_cz as f32 * self.cell_size_m,
            ];
            self.cols = (max_cx - min_cx + 1) as u32;
            self.rows = (max_cz - min_cz + 1) as u32;
        }
    }
}

/// Combines two means weighted by their sample counts, in `f64` so that long
/// sessions do not accumulate `f32` rounding error.
fn weighted_mean(left: f32, left_n: u32, right: f32, right_n: u32) -> f32 {
    let total = f64::from(left_n) + f64::from(right_n);
    if total == 0.0 {
        return left;
    }
    ((f64::from(left) * f64::from(left_n) + f64::from(right) * f64::from(right_n)) / total) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heightmap(cell_size_m: f32, cells: Vec<HeightCell>) -> WorldHeightmap {
        let mut map = WorldHeightmap {
            client_version: "test".into(),
            world_id: "fh6".into(),
            cell_size_m,
            cells,
            ..Default::default()
        };
        map.recompute_extent();
        map
    }

    fn cell(cx: i32, cz: i32, mean: f32, n: u32) -> HeightCell {
        HeightCell {
            cx,
            cz,
            y_mean: mean,
            y_min: mean,
            y_max: mean,
            n,
            ..Default::default()
        }
    }

    #[test]
    fn merge_combines_a_shared_cell() {
        let mut accumulated = heightmap(8.0, vec![cell(0, 0, 100.0, 1)]);
        accumulated.merge(&heightmap(
            8.0,
            vec![cell(0, 0, 200.0, 1), cell(5, 0, 50.0, 2)],
        ));

        assert_eq!(accumulated.cells.len(), 2);
        let shared = accumulated
            .cells
            .iter()
            .find(|candidate| (candidate.cx, candidate.cz) == (0, 0))
            .expect("the shared cell survives the merge");
        assert_eq!(shared.n, 2);
        assert_eq!(shared.y_mean, 150.0);
    }

    #[test]
    fn merge_sums_the_surface_histogram() {
        // The same cell seen in two sessions over different surfaces.
        let mut accumulated = heightmap(
            8.0,
            vec![HeightCell {
                surf: [0, 3, 0, 0, 0],
                ..cell(0, 0, 0.0, 3)
            }],
        );
        accumulated.merge(&heightmap(
            8.0,
            vec![HeightCell {
                surf: [0, 0, 2, 0, 0],
                ..cell(0, 0, 0.0, 2)
            }],
        ));

        let merged = &accumulated.cells[0];
        assert_eq!(merged.n, 5);
        assert_eq!(merged.surf, [0, 3, 2, 0, 0]);
        assert_eq!(merged.dominant_surface(), 1, "asphalt still wins, 3 > 2");
    }

    #[test]
    fn merge_takes_a_sample_weighted_mean_of_speed_and_temperature() {
        let mut accumulated = heightmap(
            8.0,
            vec![HeightCell {
                spd_mean: 100.0,
                temp_mean: 180.0,
                ..cell(0, 0, 0.0, 1)
            }],
        );
        accumulated.merge(&heightmap(
            8.0,
            vec![HeightCell {
                spd_mean: 40.0,
                temp_mean: 220.0,
                ..cell(0, 0, 0.0, 3)
            }],
        ));

        let merged = &accumulated.cells[0];
        assert_eq!(merged.n, 4);
        assert_eq!(merged.spd_mean, 55.0, "(100*1 + 40*3) / 4");
        assert_eq!(merged.temp_mean, 210.0, "(180*1 + 220*3) / 4");
    }

    #[test]
    fn merge_ignores_empty_cells() {
        let mut accumulated = heightmap(8.0, vec![cell(0, 0, 100.0, 2)]);
        accumulated.merge(&heightmap(8.0, vec![cell(0, 0, 999.0, 0)]));

        assert_eq!(accumulated.cells[0].n, 2);
        assert_eq!(accumulated.cells[0].y_mean, 100.0);
    }

    #[test]
    fn dominant_surface_is_unknown_without_samples() {
        assert_eq!(cell(0, 0, 0.0, 0).dominant_surface(), 0);
    }

    #[test]
    fn merge_recomputes_the_extent() {
        let mut accumulated = heightmap(8.0, vec![cell(2, 3, 10.0, 1)]);
        accumulated.merge(&heightmap(8.0, vec![cell(5, 3, 20.0, 1)]));

        assert_eq!(accumulated.origin, [2.0 * 8.0, 3.0 * 8.0]);
        assert_eq!(accumulated.cols, 4, "cells 2..=5");
        assert_eq!(accumulated.rows, 1);
    }

    #[test]
    fn an_empty_map_has_no_extent() {
        let mut empty = heightmap(8.0, Vec::new());
        empty.recompute_extent();

        assert_eq!(empty.origin, [0.0, 0.0]);
        assert_eq!(empty.cols, 0);
        assert_eq!(empty.rows, 0);
    }

    // --- wire-format contract ------------------------------------------------
    //
    // These pin the serialized shape. A change here breaks every already-saved
    // map file and every deployed backend, so it must be a deliberate major bump.

    #[test]
    fn height_cell_field_names_are_stable() {
        let json = serde_json::to_string(&HeightCell {
            cx: 1,
            cz: -2,
            y_mean: 3.5,
            y_min: 3.0,
            y_max: 4.0,
            n: 7,
            spd_mean: 80.0,
            temp_mean: 90.0,
            surf: [1, 2, 3, 4, 5],
        })
        .expect("HeightCell serializes");

        assert_eq!(
            json,
            r#"{"cx":1,"cz":-2,"y_mean":3.5,"y_min":3.0,"y_max":4.0,"n":7,"spd_mean":80.0,"temp_mean":90.0,"surf":[1,2,3,4,5]}"#
        );
    }

    #[test]
    fn maps_written_before_the_optional_fields_still_load() {
        let legacy = r#"{
            "client_version": "0.1.0",
            "world_id": "fh6",
            "cell_size_m": 8.0,
            "origin": [0.0, 0.0],
            "cols": 1,
            "rows": 1,
            "cells": [{"cx":0,"cz":0,"y_mean":1.0,"y_min":1.0,"y_max":1.0,"n":1}]
        }"#;

        let map: WorldHeightmap = serde_json::from_str(legacy).expect("legacy maps still load");
        assert_eq!(map.cells.len(), 1);
        assert_eq!(map.cells[0].spd_mean, 0.0);
        assert_eq!(map.cells[0].temp_mean, 0.0);
        assert_eq!(map.cells[0].surf, [0; SURFACE_KINDS]);
    }

    #[test]
    fn world_map_accepted_round_trips() {
        let response = WorldMapAccepted {
            accepted: false,
            world_id: "fh6".into(),
            cells_total: 12,
            rejected_reason: Some("stale client".into()),
        };
        let json = serde_json::to_string(&response).expect("response serializes");

        assert_eq!(
            serde_json::from_str::<WorldMapAccepted>(&json).expect("response deserializes"),
            response
        );
    }
}
