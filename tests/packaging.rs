//! Packaging smoke tests.
//!
//! These do not build an installer — that needs Inno Setup and a release
//! binary. They check the things that silently rot between releases: a script
//! pointing at a path that moved, an installer referencing an asset that was
//! renamed, or the installer identity changing so that existing users get a
//! second copy instead of an upgrade.
//!
//! Every failure here would otherwise surface as a broken release artifact.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use veloryft_hud::version::windows_resource_version;

fn repository_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is the package root, which is the repository root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path = repository_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|cause| panic!("reading {}: {cause}", path.display()))
}

fn exists(relative: &str) -> bool {
    repository_root().join(relative).exists()
}

#[test]
fn the_installer_points_at_the_binary_cargo_actually_builds() {
    // Cargo's [[bin]] name and the path in veloryft.iss are edited in different
    // files; if they drift, the installer build fails late, on a release tag.
    let installer = read("installer/veloryft.iss");

    assert!(
        installer.contains(r"..\target\release\veloryft.exe"),
        "veloryft.iss must package target\\release\\veloryft.exe"
    );
    assert!(
        read("Cargo.toml").contains(r#"name = "veloryft""#),
        "the [[bin]] target must still be named veloryft"
    );
}

#[test]
fn the_installer_app_id_is_unchanged() {
    // This GUID is the installed product's identity. Changing it makes every
    // existing install invisible to the new installer: users end up with two
    // copies and no upgrade path.
    let installer = read("installer/veloryft.iss");

    assert!(
        installer.contains("{{A7F3C2E1-9B4D-4E6A-8C1F-2D5B7E0A9C34}"),
        "the installer AppId must never change"
    );
}

#[test]
fn every_asset_the_installer_references_exists() {
    let installer = read("installer/veloryft.iss");
    let assets = Path::new("installer/assets");

    let referenced: Vec<&str> = installer
        .lines()
        .filter(|line| line.contains("assets\\"))
        .flat_map(|line| line.split(&['=', ',', '"'][..]))
        .map(str::trim)
        .filter(|token| token.starts_with("assets\\"))
        .collect();

    assert!(
        !referenced.is_empty(),
        "expected veloryft.iss to reference installer assets"
    );
    for asset in referenced {
        let name = asset.trim_start_matches("assets\\");
        assert!(
            exists(&assets.join(name).to_string_lossy()),
            "veloryft.iss references a missing asset: {asset}"
        );
    }
}

#[test]
fn the_installer_ships_the_documents_it_declares() {
    for document in [
        "installer/readme.txt",
        "LICENSE",
        "NOTICE",
        "THIRD_PARTY_NOTICES.md",
    ] {
        assert!(exists(document), "{document} is missing");
        assert!(
            !read(document).trim().is_empty(),
            "{document} must not be empty"
        );
    }
}

#[test]
fn the_icon_the_build_script_embeds_exists() {
    // build.rs fails the whole build if this is missing, but only when
    // targeting Windows, so a rename can hide until release.
    assert!(
        exists("installer/assets/veloryft.ico"),
        "build.rs embeds installer/assets/veloryft.ico"
    );
}

#[test]
fn the_packaging_scripts_exist_under_their_documented_names() {
    for script in [
        "scripts/common.ps1",
        "scripts/build_installer.ps1",
        "scripts/build_portable.ps1",
        "scripts/generate_checksums.ps1",
    ] {
        assert!(exists(script), "{script} is missing");
    }
}

#[test]
fn checksum_generation_accepts_a_single_release_artifact() {
    // PowerShell unwraps a one-item pipeline into a scalar. Keep this execution
    // test because StrictMode then makes `.Count` fail unless the script
    // explicitly preserves the collection.
    let test_directory = repository_root()
        .join("target")
        .join(format!("checksum-test-{}", std::process::id()));
    let artifact = test_directory.join("Veloryft-Portable-test.zip");
    fs::create_dir_all(&test_directory).expect("creating the checksum test directory");
    fs::write(&artifact, b"synthetic portable artifact").expect("writing the test artifact");

    let output = Command::new("pwsh")
        .args(["-NoProfile", "-File"])
        .arg(repository_root().join("scripts/generate_checksums.ps1"))
        .arg("-Path")
        .arg(&test_directory)
        .output()
        .expect("running generate_checksums.ps1");

    let cleanup_result = fs::remove_dir_all(&test_directory);
    assert!(
        output.status.success(),
        "checksum generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    cleanup_result.expect("removing the checksum test directory");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("OK: checksums"));
}

#[test]
fn the_build_scripts_pass_both_version_forms_to_the_installer() {
    // veloryft.iss refuses to compile without both, so this catches a script
    // change that would only fail once Inno Setup runs.
    let script = read("scripts/build_installer.ps1");

    assert!(
        script.contains("/DMyAppVersion="),
        "build_installer.ps1 must define MyAppVersion"
    );
    assert!(
        script.contains("/DMyAppVersionInfo="),
        "build_installer.ps1 must define MyAppVersionInfo"
    );
}

#[test]
fn the_powershell_and_rust_version_rules_agree() {
    // ConvertTo-WindowsVersion in scripts/common.ps1 mirrors
    // version::windows_resource_version. They are separate implementations, so
    // this pins the cases where they could diverge.
    for (semver, expected) in [
        ("0.2.0", "0.2.0.0"),
        ("0.2.0-beta.1", "0.2.0.1"),
        ("1.0.0-rc.4", "1.0.0.4"),
        ("12.34.56", "12.34.56.0"),
    ] {
        assert_eq!(
            windows_resource_version(semver).as_deref(),
            Some(expected),
            "version derivation changed for {semver}"
        );
    }

    let script = read("scripts/common.ps1");
    assert!(
        script.contains("function ConvertTo-WindowsVersion"),
        "common.ps1 must still provide ConvertTo-WindowsVersion"
    );
}

#[test]
fn this_packages_version_produces_a_valid_windows_resource_version() {
    let version = env!("CARGO_PKG_VERSION");

    let resource = windows_resource_version(version)
        .unwrap_or_else(|| panic!("package version {version} cannot become a Windows version"));
    assert_eq!(
        resource.split('.').count(),
        4,
        "a Windows version resource needs four components"
    );
    assert!(
        resource.split('.').all(|part| part.parse::<u16>().is_ok()),
        "every component must fit in a u16"
    );
}

#[test]
fn no_unlicensed_vehicle_catalog_is_bundled() {
    assert!(
        !exists("data/car_ordinals.json"),
        "third-party vehicle data must not be redistributed"
    );
    assert!(
        !read("src/telemetry/vehicle_catalog.rs").contains("include_str!"),
        "the executable must not embed a vehicle catalog"
    );
}

#[test]
fn no_build_input_references_the_old_src_tauri_layout() {
    // The package moved out of src-tauri. A leftover path in a script or the
    // installer would break the release build.
    for file in [
        "installer/veloryft.iss",
        "scripts/build_installer.ps1",
        "scripts/build_portable.ps1",
        "scripts/common.ps1",
        "build.rs",
    ] {
        assert!(
            !read(file).contains("src-tauri"),
            "{file} still references src-tauri"
        );
    }
}
