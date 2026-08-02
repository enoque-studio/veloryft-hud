//! Embeds the Windows resources into the executable: the application icon and
//! the version block (ProductName, copyright, and so on). Without them the
//! binary shows up with no identity in Explorer, Task Manager, and SmartScreen.
//!
//! The version block is derived from the Cargo package version, which is the
//! single source of truth for this repository — see
//! `docs/distribution/versioning.md`.

const ICON: &str = "installer/assets/veloryft.ico";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    // The build script runs on the host; only emit resources when the *target*
    // is Windows.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    println!("cargo:rerun-if-changed={ICON}");

    let semver = std::env::var("CARGO_PKG_VERSION").expect("Cargo must set CARGO_PKG_VERSION");
    let resource_version = windows_resource_version(&semver)
        .unwrap_or_else(|| panic!("package version {semver:?} is not a supported SemVer string"));

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(ICON);
    resource.set("ProductName", "Veloryft");
    resource.set("FileDescription", "Veloryft — Forza telemetry overlay");
    resource.set("CompanyName", "Veloryft");
    resource.set("LegalCopyright", "\u{a9} 2026 Veloryft. All rights reserved.");
    resource.set("InternalName", "veloryft");
    resource.set("OriginalFilename", "Veloryft.exe");
    resource.set("ProductVersion", &resource_version);
    resource.set("FileVersion", &resource_version);
    resource
        .compile()
        .expect("embedding Windows resources (needs rc.exe from the Windows SDK)");
}

// Shared with the library so the rule has one implementation and one test suite.
// `cargo test` does not build test targets for build scripts, so the tests that
// cover this live in `src/version.rs` and run as part of the library.
include!("src/version.rs");
