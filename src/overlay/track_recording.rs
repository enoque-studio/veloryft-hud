//! Adapter between the overlay runtime and the optional track-mapping feature.
//!
//! The runtime calls the same four methods either way. With the `track_mapping`
//! feature off, [`TrackRecording::start`] returns `None`, nothing is recorded,
//! and no HTTP listener is created — the recorder and its dependencies are not
//! even compiled.

#[cfg(feature = "track_mapping")]
mod implementation {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use crate::diagnostics::{log_info, log_warn};
    use crate::telemetry::model::Telemetry;
    use crate::track_mapping::local_api;
    use crate::track_mapping::world_map::{self, WorldMapRecorder};

    /// A live world-map recording plus the file it persists to.
    #[derive(Clone)]
    pub struct TrackRecording {
        recorder: Arc<Mutex<WorldMapRecorder>>,
        path: PathBuf,
    }

    impl TrackRecording {
        /// Seeds the recorder from disk, starts autosave, and opens the
        /// loopback viewer API.
        pub fn start() -> Option<Self> {
            let path = world_map::default_path();
            let mut recorder = WorldMapRecorder::new();
            recorder.load_seed(&path);
            let recorder = Arc::new(Mutex::new(recorder));

            world_map::spawn_autosave(recorder.clone(), path.clone());
            local_api::spawn(recorder.clone(), path.clone());
            log_info!("track mapping enabled, persisting to {}", path.display());

            Some(Self { recorder, path })
        }

        /// Feeds one telemetry frame into the recorder.
        pub fn ingest(&self, frame: &Telemetry, driving: bool) {
            match self.recorder.lock() {
                Ok(mut recorder) => recorder.ingest(frame, driving),
                Err(_) => log_warn!("track recorder mutex poisoned; dropping frame"),
            }
        }

        /// Writes the map and the driven path without truncating the logs.
        pub fn save(&self) {
            if let Ok(recorder) = self.recorder.lock() {
                let _ = recorder.save(&self.path);
                let _ = recorder.save_path(&self.path);
            }
        }

        /// Writes a full snapshot and truncates the incremental logs, so the
        /// next start is fast.
        pub fn compact(&self) {
            if let Ok(mut recorder) = self.recorder.lock() {
                let _ = recorder.compact(&self.path);
            }
        }
    }
}

#[cfg(not(feature = "track_mapping"))]
mod implementation {
    use crate::telemetry::model::Telemetry;

    /// Stand-in used when the feature is compiled out. Never constructed.
    ///
    /// Deliberately not `Copy`: the enabled implementation holds an `Arc`, and
    /// the runtime clones the handle. Matching that here keeps the call site
    /// identical under both feature settings.
    #[derive(Clone)]
    pub struct TrackRecording;

    impl TrackRecording {
        pub fn start() -> Option<Self> {
            None
        }

        pub fn ingest(&self, _frame: &Telemetry, _driving: bool) {}

        pub fn save(&self) {}

        pub fn compact(&self) {}
    }
}

pub(crate) use implementation::TrackRecording;
