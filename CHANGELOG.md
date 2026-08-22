# Changelog

All notable changes to this project are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). The
version in `Cargo.toml` is the single source of truth — see
[versioning](docs/distribution/versioning.md).

## [Unreleased]

## [0.2.0-beta.1] — 2026-08-02

The first release prepared entirely from the public repository. It preserves
the native overlay and its performance-sensitive rendering model while making
the project reproducible, reviewable, and ready for outside contributors.

### Added

- `examples/synthetic_telemetry`, which drives the HUD without the game, so the
  overlay can be developed and reviewed without owning Forza Horizon 6.
- Structured diagnostics. Release builds use the Windows subsystem and have no
  console, so failures now go to a bounded, rotated log in
  `%APPDATA%\forza-hud-native\logs\`, and failures that prevent startup — a UDP
  port already in use, Direct2D unavailable — are shown in a message box with
  what to do about it. Log level is set with `VELORYFT_LOG`.
- `crates/veloryft_protocol`, the world-map serialization contract shared with
  the Veloryft backend, with its own versioning policy and tests pinning the
  wire format.
- Public CI running formatting, Clippy with warnings as errors, tests for both
  feature configurations, documentation build, and `cargo deny`.
- SHA-256 checksums published with every release artifact.
- Apache-2.0 license, third-party notices, trademark policy, contribution guide,
  security policy, and a documentation tree in English.
- A typed English localization catalog that keeps user-facing text out of
  rendering, windowing, telemetry, and diagnostics code while providing an
  explicit fallback for future complete locales.

### Changed

- The Rust package moved out of `src-tauri/`, which had not been a Tauri project
  for some time, to a conventional root layout.
- `render.rs` was 3,706 lines owning windowing, graphics, pacing, persistence,
  input and every panel's drawing. It is now nineteen modules under
  `src/overlay/`, the largest 700 lines.
- `forza/` became `telemetry/`, with the world map and its HTTP API moved to
  `track_mapping/`.
- Track mapping is a Cargo feature rather than a runtime constant. The default
  build no longer compiles its recorder, its HTTP server, or `tiny_http`, and
  opens no listening socket.
- The binary is now `veloryft.exe` rather than `forza-hud-native.exe`. The
  installer still deploys it as `Veloryft.exe`, and the installer `AppId` is
  unchanged, so upgrades find existing installs.
- The version comes from `Cargo.toml`; the separate `VERSION` file is gone.
- Packaging scripts moved to `scripts/` with snake_case names and a shared
  helper for prerequisite checks and version lookup.
- Official binaries now use the same Apache-2.0 terms as the source. The
  unreviewed custom EULA and its click-through installer page were retired;
  packages include `LICENSE`, `NOTICE`, and third-party notices instead.

### Removed

- The path dependency on `../../veloryft-back/crates/core`. A clean clone of
  this repository now builds with no other repository and no access token.
- The commercial licensing subsystem, which nothing called — the beta never ran
  it — and which pulled in fourteen dependencies for cryptography, TLS, WMI and
  an async runtime. It is replaced by a `StartupAuthorization` boundary whose
  only public implementation lets the open beta start. The removed code remains
  in git history.
- `.cargo/config.toml` no longer disables Cargo's certificate revocation
  checking for everyone who clones the repository. It is documented as an opt-in
  local workaround instead.
- Two tester-feedback PDFs and a screenshot of an unrelated product surface.
  They remain in git history; see the release notes.
- The embedded vehicle-name catalog. Its source was traced to Martin Refseth's
  public FH6 ordinal gist, which declares no reuse license. Veloryft keeps the
  local catalog extension point and displays raw ordinals when none is supplied,
  without redistributing the external data.

### Fixed

- The tray icon resource ordinal was built by casting `1usize` to a pointer.
  It now uses `ptr::without_provenance`, which is what the Win32
  `MAKEINTRESOURCE` idiom actually means.

### Security

- The default build opens no listening socket at all. The loopback HTTP server
  exists only when `track_mapping` is enabled; its origin checks and request
  size limits are now covered by tests.

## [0.1.0] — 2026-07-11

First open beta, distributed as an installer and a portable package. Not
released from this repository's public history.

[Unreleased]: https://github.com/enoquesousa/veloryft-hud/compare/v0.2.0-beta.1...HEAD
[0.2.0-beta.1]: https://github.com/enoquesousa/veloryft-hud/releases/tag/v0.2.0-beta.1
