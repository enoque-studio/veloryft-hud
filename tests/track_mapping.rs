//! Feature-specific tests for track mapping.
//!
//! The whole file compiles away without the `track_mapping` feature, which is
//! itself part of what is being verified: the default build must not contain
//! this code at all.
//!
//! Run them with:
//!
//! ```text
//! cargo test --features track_mapping
//! ```

#![cfg(feature = "track_mapping")]

use veloryft_hud::telemetry::model::Telemetry;
use veloryft_hud::track_mapping::world_map::WorldMapRecorder;
use veloryft_protocol::SURFACE_KINDS;

/// A telemetry frame at a world position, driving on smooth asphalt.
fn frame_at(position: [f32; 3], speed_kmh: f32) -> Telemetry {
    Telemetry {
        race: 1,
        pos: Some(position),
        speed: Some(speed_kmh),
        tires: Some([80.0; 4]),
        surf_rumble: Some([0.01; 4]),
        puddle: Some([0.0; 4]),
        ..Default::default()
    }
}

#[test]
fn recording_is_off_until_it_is_switched_on() {
    let mut recorder = WorldMapRecorder::with_cell_size(8.0);

    recorder.ingest(&frame_at([0.0, 10.0, 0.0], 90.0), true);

    assert_eq!(
        recorder.stats().cells,
        0,
        "nothing may be recorded before recording starts"
    );
}

#[test]
fn driving_accumulates_cells_and_samples() {
    let mut recorder = WorldMapRecorder::with_cell_size(8.0);
    recorder.set_recording(true);

    // Drive a straight line far enough to cross several cells.
    for step in 0..40 {
        let x = step as f32 * 8.0;
        recorder.ingest(&frame_at([x, 10.0 + step as f32, 0.0], 120.0), true);
    }

    let stats = recorder.stats();
    assert!(
        stats.cells > 1,
        "a drive across cells must fill more than one"
    );
    assert!(stats.samples > 0);
    assert_eq!(stats.world_id, "fh6");
}

#[test]
fn a_recorded_map_round_trips_through_disk() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("worldmap-test.json");

    let mut recorder = WorldMapRecorder::with_cell_size(8.0);
    recorder.set_recording(true);
    for step in 0..20 {
        recorder.ingest(&frame_at([step as f32 * 8.0, 5.0, 0.0], 100.0), true);
    }
    let before = recorder.stats();
    recorder.save(&path).expect("saving the map");

    let mut reloaded = WorldMapRecorder::with_cell_size(8.0);
    reloaded.load_seed(&path);

    assert_eq!(
        reloaded.stats().cells,
        before.cells,
        "reloading must recover every cell"
    );
}

#[test]
fn the_heightmap_it_produces_matches_the_published_contract() {
    let mut recorder = WorldMapRecorder::with_cell_size(8.0);
    recorder.set_recording(true);
    for step in 0..10 {
        recorder.ingest(&frame_at([step as f32 * 8.0, 12.0, 0.0], 100.0), true);
    }

    let heightmap = recorder.to_heightmap();

    assert_eq!(heightmap.cell_size_m, 8.0);
    assert_eq!(heightmap.world_id, "fh6");
    assert!(!heightmap.cells.is_empty());
    for cell in &heightmap.cells {
        assert!(cell.n > 0, "an emitted cell must carry samples");
        assert!(cell.y_min <= cell.y_mean && cell.y_mean <= cell.y_max);
        assert_eq!(cell.surf.len(), SURFACE_KINDS);
    }

    // The map must serialize as the contract crate defines it, because this is
    // exactly what the loopback API serves and the backend consumes.
    let json = serde_json::to_string(&heightmap).expect("the heightmap serializes");
    let parsed: veloryft_protocol::WorldHeightmap =
        serde_json::from_str(&json).expect("and deserializes through the contract crate");
    assert_eq!(parsed, heightmap);
}

#[test]
fn smooth_asphalt_is_classified_as_road_not_as_unknown() {
    let mut recorder = WorldMapRecorder::with_cell_size(8.0);
    recorder.set_recording(true);
    for step in 0..10 {
        recorder.ingest(&frame_at([step as f32 * 8.0, 0.0, 0.0], 100.0), true);
    }

    let heightmap = recorder.to_heightmap();
    let classified = heightmap
        .cells
        .iter()
        .any(|cell| cell.dominant_surface() == 1);
    assert!(classified, "low surface rumble must classify as asphalt");
}

#[test]
fn resetting_clears_everything_that_was_recorded() {
    let mut recorder = WorldMapRecorder::with_cell_size(8.0);
    recorder.set_recording(true);
    for step in 0..10 {
        recorder.ingest(&frame_at([step as f32 * 8.0, 0.0, 0.0], 100.0), true);
    }
    assert!(recorder.stats().cells > 0);

    recorder.reset();

    assert_eq!(recorder.stats().cells, 0);
    assert_eq!(recorder.path_points(), 0);
    assert_eq!(recorder.last_pos(), None);
}

#[test]
fn the_default_persistence_path_lives_beside_the_other_settings() {
    // Users are told where their data is; this keeps that promise true.
    let path = veloryft_hud::track_mapping::world_map::default_path();

    assert!(path.ends_with("worldmap-fh6.json"));
    assert!(
        path.parent()
            .is_some_and(|parent| parent.ends_with(veloryft_hud::paths::CONFIG_DIR_NAME)),
        "the map must sit in the documented configuration directory"
    );
}
