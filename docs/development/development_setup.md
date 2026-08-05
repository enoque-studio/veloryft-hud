# Development setup

## Prerequisites

- **64-bit Windows 10 or later.** The overlay is Win32 and Direct2D; there is no
  cross-platform build.
- **Rust with the MSVC toolchain**, from [rustup.rs](https://rustup.rs). The
  pinned version and components come from `rust-toolchain.toml` automatically.
- **Visual Studio Build Tools** with the "Desktop development with C++"
  workload, for the linker and `rc.exe`.
- **Inno Setup 6**, only to build the installer:
  `winget install --id JRSoftware.InnoSetup`
- **Python with Pillow**, only to regenerate the installer artwork.

## Clone and build

This repository has no private dependencies and needs no access token.

```bash
git clone https://github.com/ESousa97/veloryft-hud
cd veloryft-hud
cargo build --release
```

The binary is `target/release/veloryft.exe`.

If the build fails in `build.rs` with something about `rc.exe`, the Windows SDK
is missing — install the C++ workload above.

## Run it

```bash
cargo run
```

Debug builds keep a console, so `eprintln!` output and log records are visible
there. Release builds target the Windows subsystem and have none; everything
goes to the log file.

## Develop without the game

You do not need Forza Horizon 6.

```bash
cargo run
```

```bash
cargo run --example synthetic_telemetry
```

The HUD appears within a second, showing a car working through the gears around
a closed loop. Useful flags:

| Flag | Effect |
| ---- | ------ |
| `--format sled\|dash\|fh6` | Which wire format to send |
| `--idle` | Send `is_race_on = 0`, so the HUD stays hidden |
| `--port N` | Send somewhere other than 20066 |
| `--seconds N` | Stop after N seconds |

`--format sled` is the quickest way to confirm the HUD correctly refuses to draw
on a feed that has no dash data.

## Before pushing

Exactly what CI runs:

```bash
cargo fmt --all -- --check
```

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

```bash
cargo test --workspace --all-features
```

```bash
cargo doc --workspace --all-features --no-deps
```

## Optional tooling

```bash
cargo install cargo-deny cargo-audit
```

```bash
cargo deny check
```

`cargo deny` is the same check CI runs for licenses, advisories and sources.

## Editor

`.editorconfig` and `rustfmt.toml` cover formatting. For VS Code,
`.vscode/extensions.json` recommends rust-analyzer; nothing else is required.

Enable all features in your editor so the track-mapping modules are analyzed:

```json
{ "rust-analyzer.cargo.features": "all" }
```

## Working with the optional feature

```bash
cargo run --features track_mapping
```

```bash
cargo test --features track_mapping
```

Check both configurations before pushing. Code that compiles only when a feature
is on, or only when it is off, is a break waiting for a release.

## If the toolchain fights you

**Cargo cannot download crates, certificate revocation errors.** Some corporate
networks block the revocation endpoint Schannel wants, and Cargo fails with
`CRYPT_E_NO_REVOCATION_CHECK`.

This repository deliberately does **not** disable that check for everyone. If
you need the workaround, put it in your own Cargo configuration:

```toml
# %USERPROFILE%\.cargo\config.toml
[http]
check-revoke = false
```

Understand what you are turning off: Cargo will no longer notice that a
certificate in the chain has been revoked. Scope it to your machine, and prefer
fixing the network path if you can.

**Linker errors mentioning `vcruntime`.** The static CRT setting in
`.cargo/config.toml` applies to `x86_64-pc-windows-msvc`. Building for a
different target will not pick it up. See
[ADR 0002](../decisions/adr_0002_static_crt.md).

## Next

- [Coding standards](coding_standards.md)
- [Testing strategy](testing_strategy.md)
- [Debugging](debugging.md)
- [Contributing](../../CONTRIBUTING.md)
