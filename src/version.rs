// This file is `include!`d by build.rs as well as compiled as a library module,
// so it must not carry inner doc comments (`//!`). The module documentation
// lives on the `mod version;` declaration in lib.rs.

/// Converts `major.minor.patch[-prerelease.N]` into the four-component
/// `major.minor.patch.build` form that Windows version resources require.
///
/// The fourth component carries the prerelease ordinal, or `0` for a final
/// release. Windows has no concept of a prerelease, so `0.2.0-beta.1` becomes
/// `0.2.0.1` and sorts *above* the eventual `0.2.0` (`0.2.0.0`). That is
/// cosmetic: Inno Setup identifies installs by `AppId`, not by this number, so
/// upgrades are unaffected. Build metadata (`+sha`) is ignored.
#[must_use]
pub fn windows_resource_version(semver: &str) -> Option<String> {
    let semver = semver.split('+').next()?;
    let (core, prerelease) = match semver.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (semver, None),
    };

    let mut parts = core.split('.');
    let (major, minor, patch) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    for part in [major, minor, patch] {
        part.parse::<u16>().ok()?;
    }

    // Take the last dot-separated numeric identifier of the prerelease, so both
    // `beta.1` and `rc.4` map onto the build component.
    let build = match prerelease {
        None => 0,
        Some(prerelease) => prerelease
            .rsplit('.')
            .find_map(|identifier| identifier.parse::<u16>().ok())
            .unwrap_or(0),
    };

    Some(format!("{major}.{minor}.{patch}.{build}"))
}

#[cfg(test)]
mod tests {
    use super::windows_resource_version;

    #[test]
    fn final_releases_get_a_zero_build_component() {
        assert_eq!(windows_resource_version("0.2.0").as_deref(), Some("0.2.0.0"));
        assert_eq!(
            windows_resource_version("12.34.56").as_deref(),
            Some("12.34.56.0")
        );
    }

    #[test]
    fn prereleases_carry_their_ordinal_in_the_build_component() {
        assert_eq!(
            windows_resource_version("0.2.0-beta.1").as_deref(),
            Some("0.2.0.1")
        );
        assert_eq!(
            windows_resource_version("1.0.0-rc.4").as_deref(),
            Some("1.0.0.4")
        );
        assert_eq!(
            windows_resource_version("1.0.0-alpha").as_deref(),
            Some("1.0.0.0")
        );
    }

    #[test]
    fn build_metadata_is_ignored() {
        assert_eq!(
            windows_resource_version("0.2.0-beta.1+abc123").as_deref(),
            Some("0.2.0.1")
        );
    }

    #[test]
    fn malformed_versions_are_rejected_rather_than_guessed() {
        assert_eq!(windows_resource_version("0.2"), None);
        assert_eq!(windows_resource_version("0.2.0.0"), None);
        assert_eq!(windows_resource_version("x.y.z"), None);
        assert_eq!(windows_resource_version("0.2.0-"), Some("0.2.0.0".into()));
    }
}
