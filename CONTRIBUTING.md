# Contributing to Veloryft

Thanks for taking the time. This document covers what you need to build the
project, what CI will check, and the few areas where the usual advice does not
apply because the code is on a render hot path.

## Prerequisites

Veloryft is a Windows application. You need:

- **Windows 10 (64-bit) or later.** The overlay is built on Win32 layered
  windows and Direct2D; there is no cross-platform build, and pretending
  otherwise would waste your time.
- **Rust, MSVC toolchain.** Install from [rustup.rs](https://rustup.rs) and pick
  the `x86_64-pc-windows-msvc` host. The pinned toolchain and components come
  from [`rust-toolchain.toml`](rust-toolchain.toml) automatically.
- **Visual Studio Build Tools** with the "Desktop development with C++" workload.
  This provides the linker and `rc.exe`, which `build.rs` needs to embed the
  icon and version resource.
- **Inno Setup 6** — only if you intend to build the installer:
  `winget install --id JRSoftware.InnoSetup`

The minimum supported Rust version is **1.85**, and CI verifies it.

## Building

Clone this repository and nothing else. It has no private dependencies.

```bash
git clone https://github.com/ESousa97/veloryft-hud
cd veloryft-hud
cargo build --release
```

The binary lands at `target/release/veloryft.exe`.

## Testing without Forza

You do not need the game. Run the overlay in one terminal and the synthetic
telemetry sender in another:

```bash
cargo run
```

```bash
cargo run --example synthetic_telemetry
```

The HUD should appear within a second and show a car working through the gears
around a closed loop. The sender accepts `--format sled|dash|fh6`, `--port`,
`--idle` (to check that the HUD hides itself), and `--seconds`.

Everything it sends is generated from formulas. No recorded telemetry ships with
this project, and pull requests must not add any: a capture identifies a person's
driving, and we cannot license someone else's.

## What CI checks

Run these before opening a pull request. They are exactly what
[`ci.yml`](.github/workflows/ci.yml) runs.

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

Clippy warnings are errors in CI. If a lint is genuinely wrong for a particular
line, add a narrow `#[allow(...)]` with a comment explaining why — not a
module-wide or crate-wide allow.

## Commits and pull requests

- Commit subjects use a scope prefix: `build`, `deps`, `architecture`,
  `overlay`, `telemetry`, `docs`, `security`, `ci`, `release`, `legal`.
  Imperative mood, lower case, no trailing period.
- Explain **why** in the body. What changed is visible in the diff; why it
  changed is not.
- Keep pull requests to one concern. A refactor and a behaviour change in the
  same pull request cannot be reviewed properly, because the reviewer cannot
  tell which lines are which.
- If you change behaviour a user can observe, update the documentation in the
  same pull request. A `docs/` tree that lags the code is worse than none.
- Keep code, technical names, comments, tests, logs, and documentation in
  English. Put user-facing text in `src/localization/`; do not embed it in a
  renderer, window procedure, or diagnostics call. See the
  [localization guide](docs/development/localization.md).
- New behaviour needs a test. If it cannot be tested, say why in the pull
  request; that is usually a sign the boundary is in the wrong place.

## Performance-sensitive code

`src/overlay/` runs every frame while someone is driving. The overlay competes
with the game for the compositor, and a stutter in the HUD is a stutter the
player feels.

Inside the render loop and the critical throttle thread:

- **No allocation.** No `Vec`, `String`, `format!`, or collection growth.
- **No locking** beyond what is already there. The throttle thread shares
  exactly one mutex with the main loop, holding it only to copy a small
  `ThrottlePlacement`.
- **No logging.** Failures that can recur every frame are latched through
  `diagnostics::Once`, so the first one is recorded and the rest are silent.
- **No async runtime.** There is none in the dependency tree; adding one to this
  path would be a regression, not a modernization.

The critical throttle bar has its own surface and a `THREAD_PRIORITY_HIGHEST`
thread at 125 Hz. That is deliberate and measured. If you believe a different
design is better, bring measurements — see
[`docs/architecture/rendering_pipeline.md`](docs/architecture/rendering_pipeline.md)
for what to measure.

Panels other than the cluster may be rate-limited under compositor pressure. The
cluster never is.

## Unsafe Rust

Win32 and Direct2D are unsafe APIs, so `src/overlay/` contains `unsafe`. The
rules:

- Keep it in `src/overlay/`. `src/telemetry/` and `crates/veloryft_protocol/`
  contain none, and `veloryft-protocol` sets `#![forbid(unsafe_code)]`.
- Keep blocks small. Wrap the Win32 call, not the surrounding logic.
- Each module that uses `unsafe` documents the invariants it relies on once, at
  the top. Add a `// SAFETY:` comment on any individual block whose reasoning
  goes beyond those shared invariants — raw pointers, lifetimes, anything
  stored in and read back out of window user data.
- An `unsafe fn` needs a `# Safety` section in its doc comment stating what the
  caller must guarantee.

## Changing the telemetry protocol

Packet offsets are not style. They describe bytes a game sends, and a wrong
offset produces plausible-looking garbage rather than an error.

- Do not change an offset without evidence. Say in the pull request how you
  established it.
- `telemetry::synthetic` must be updated alongside `telemetry::packet`; the
  round-trip tests fail if they disagree, which is the point.
- The serialized shapes in `crates/veloryft_protocol` are a contract with saved
  map files and with the backend. Its tests pin the field names. If one fails,
  you have made a compatibility break — decide deliberately, then bump the
  version accordingly.

## Configuration compatibility

Users have files in `%APPDATA%\forza-hud-native`. Do not rename that directory,
change an existing settings key, or drop a field without a fallback. The parsers
in `overlay::settings` skip what they do not recognize, and there are tests for
loading files written by older releases; keep it that way.

Environment variables (`FORZA_UDP_PORT`, `FORZA_CAR_ORDINALS`, `VELORYFT_LOG`,
and the track-mapping ones) are a public contract. Adding one is fine; renaming
one needs a migration and a documentation update.

## Licensing of contributions

Contributions are accepted under [Apache-2.0](LICENSE), the license this project
uses. By opening a pull request you agree your contribution is licensed that way.

Do not add dependencies under copyleft licenses, or data whose origin you cannot
state. `cargo deny check` enforces the allowed license set, and
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) records everything shipped.

## Security

Do not open a public issue for a vulnerability. See [SECURITY.md](SECURITY.md).
