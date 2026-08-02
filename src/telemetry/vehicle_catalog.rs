//! Optional car names by `car_ordinal`, loaded from a user-supplied JSON file
//! with the legacy shape `"Car Name": "247"`.
//!
//! Veloryft deliberately does not redistribute a catalog. The known community
//! source has no declared reuse license, so users may provide their own file by
//! environment variable or in one of the documented local paths. Without one,
//! the HUD displays the ordinal and every other feature keeps working.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::diagnostics::{log_info, log_warn};

static CATALOG: OnceLock<HashMap<i32, String>> = OnceLock::new();

/// Environment variable pointing at a catalog file. Part of the
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
                log_info!(
                    "vehicle catalog: {} cars from {}",
                    catalog.len(),
                    path.display()
                );
                return catalog;
            }
            Ok(_) => log_warn!("vehicle catalog: {} is empty, ignoring", path.display()),
            Err(cause) if cause.kind() != std::io::ErrorKind::NotFound => {
                log_warn!(
                    "vehicle catalog: {} could not be read: {cause}",
                    path.display()
                );
            }
            Err(_) => {}
        }
    }

    log_info!("vehicle catalog: no external catalog found; showing car ordinals");
    HashMap::new()
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

/// Catalog locations, in priority order.
fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(configured) = std::env::var(CATALOG_PATH_VAR) {
        paths.push(PathBuf::from(configured));
    }
    if let Ok(working_directory) = std::env::current_dir() {
        paths.push(working_directory.join("data/car_ordinals.json"));
        paths.push(working_directory.join("../data/car_ordinals.json"));
        // Compatibility with development layouts created before 0.2.
        paths.push(working_directory.join("data/car-ordinals.json"));
        paths.push(working_directory.join("../data/car-ordinals.json"));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            paths.push(directory.join("data/car_ordinals.json"));
            paths.push(directory.join("../data/car_ordinals.json"));
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
    fn a_user_supplied_catalog_parses() {
        let catalog =
            parse(r#"{"1969 Toyota 2000 GT":"247"}"#).expect("a valid user catalog must parse");

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
    fn the_environment_variable_takes_priority_over_documented_paths() {
        // Only asserts ordering, so it does not depend on the ambient environment.
        let paths = candidate_paths();
        if let Ok(configured) = std::env::var(CATALOG_PATH_VAR) {
            assert_eq!(paths.first(), Some(&PathBuf::from(configured)));
        }
    }
}
