//! Where Veloryft keeps per-user state on disk.
//!
//! The directory name is `forza-hud-native` for historical reasons: it is the
//! folder existing beta installs already use. Renaming it would silently orphan
//! every user's saved layout, theme, and learned rev limits, so it stays.

use std::path::PathBuf;

/// Directory name under `%APPDATA%`. Part of the user-facing configuration
/// contract — see `docs/reference/configuration_files.md`.
pub const CONFIG_DIR_NAME: &str = "forza-hud-native";

/// `%APPDATA%/forza-hud-native`, or `./forza-hud-native` when `%APPDATA%` is not
/// set (portable use from a shell that clears the environment).
#[must_use]
pub fn config_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join(CONFIG_DIR_NAME)
}

/// `%APPDATA%/forza-hud-native/logs` — see [`crate::diagnostics`].
#[must_use]
pub fn log_dir() -> PathBuf {
    config_dir().join("logs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_keeps_the_historical_folder_name() {
        // Changing this orphans existing users' settings.
        assert!(config_dir().ends_with("forza-hud-native"));
        assert!(log_dir().ends_with("forza-hud-native/logs") || log_dir().ends_with("logs"));
    }
}
