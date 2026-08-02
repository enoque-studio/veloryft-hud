//! Car names by `car_ordinal`, loaded from `data/car-ordinals.json`
//! (legacy shape: `"Car Name": "247"`).
//!
//! The JSON is embedded into the binary at compile time, so installed and
//! portable builds always have a catalog. A file on disk (environment variable,
//! working directory, or next to the executable) only *overrides* the embedded
//! copy, which is how names can be updated without recompiling.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::diagnostics::{log_info, log_warn};

static CATALOG: OnceLock<HashMap<i32, String>> = OnceLock::new();

static EMBEDDED_CATALOG: &str = include_str!("../../data/car-ordinals.json");

/// Environment variable pointing at a replacement catalog file. Part of the
/// public contract — see `docs/reference/environment_variables.md`.
pub const CATALOG_PATH_VAR: &str = "FORZA_CAR_ORDINALS";

/// Watts → cv (metric horsepower), the unit the HUD displays.
pub const WATTS_PER_CV: f32 = 745.7;

/// Loads the catalog once. Later calls are no-ops.
pub fn init() {
    let _ = CATALOG.set(load());
}

/// Human-readable name for an ordinal, if the catalog knows it.
#[must_use]
pub fn lookup(ordinal: i32) -> Option<String> {
    if ordinal == 0 {
        return None;
    }
    CATALOG.get()?.get(&ordinal).cloned()
}

/// Number of cars loaded. Zero before [`init`].
#[must_use]
pub fn len() -> usize {
    CATALOG.get().map_or(0, HashMap::len)
}

fn load() -> HashMap<i32, String> {
    for path in candidate_paths() {
        match load_file(&path) {
            Ok(catalog) if !catalog.is_empty() => {
                log_info!("vehicle catalog: {} cars from {}", catalog.len(), path.display());
                return catalog;
            }
            Ok(_) => log_warn!("vehicle catalog: {} is empty, ignoring", path.display()),
            Err(cause) if cause.kind() != std::io::ErrorKind::NotFound => {
                log_warn!("vehicle catalog: {} could not be read: {cause}", path.display());
            }
            Err(_) => {}
        }
    }

    match parse(EMBEDDED_CATALOG) {
        Ok(catalog) if !catalog.is_empty() => {
            log_info!("vehicle catalog: {} cars from the embedded copy", catalog.len());
            catalog
        }
        _ => {
            // Only reachable if the embedded JSON was corrupted at build time.
            log_warn!("vehicle catalog: embedded copy is invalid; car names unavailable");
            HashMap::new()
        }
    }
}

fn load_file(path: &Path) -> std::io::Result<HashMap<i32, String>> {
    parse(&std::fs::read_to_string(path)?)
}

fn parse(text: &str) -> std::io::Result<HashMap<i32, String>> {
    let raw: HashMap<String, String> = serde_json::from_str(text)
        .map_err(|cause| std::io::Error::new(std::io::ErrorKind::InvalidData, cause))?;

    let mut catalog = HashMap::with_capacity(raw.len());
    for (name, ordinal) in raw {
        if let Ok(ordinal) = ordinal.parse::<i32>() {
            catalog.insert(ordinal, name);
        }
    }
    Ok(catalog)
}

/// Override locations, in priority order. The embedded copy is the fallback.
fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(configured) = std::env::var(CATALOG_PATH_VAR) {
        paths.push(PathBuf::from(configured));
    }
    if let Ok(working_directory) = std::env::current_dir() {
        paths.push(working_directory.join("data/car-ordinals.json"));
        paths.push(working_directory.join("../data/car-ordinals.json"));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            paths.push(directory.join("data/car-ordinals.json"));
            paths.push(directory.join("../data/car-ordinals.json"));
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_watts_to_metric_horsepower() {
        assert!((750_000.0 / WATTS_PER_CV - 1006.0).abs() < 1.0);
    }

    #[test]
    fn the_embedded_catalog_parses_and_is_populated() {
        let catalog = parse(EMBEDDED_CATALOG).expect("the embedded catalog is valid JSON");

        assert!(!catalog.is_empty());
        assert_eq!(
            catalog.get(&247).map(String::as_str),
            Some("1969 Toyota 2000 GT")
        );
    }

    #[test]
    fn entries_with_non_numeric_ordinals_are_skipped_not_fatal() {
        let catalog = parse(r#"{"Good Car":"12","Broken Car":"not-a-number"}"#)
            .expect("a malformed ordinal must not fail the whole catalog");

        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog.get(&12).map(String::as_str), Some("Good Car"));
    }

    #[test]
    fn invalid_json_is_reported_as_an_error() {
        assert!(parse("{not json").is_err());
    }

    #[test]
    fn the_override_variable_takes_priority_over_bundled_paths() {
        // Only asserts ordering, so it does not depend on the ambient environment.
        let paths = candidate_paths();
        if let Ok(configured) = std::env::var(CATALOG_PATH_VAR) {
            assert_eq!(paths.first(), Some(&PathBuf::from(configured)));
        }
    }
}
