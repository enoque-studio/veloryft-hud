# Technology stack

## Language and toolchain

| | |
| --- | --- |
| Language | Rust, edition 2021 |
| Minimum supported version | 1.85 |
| Target | `x86_64-pc-windows-msvc` |
| Toolchain | Pinned in `rust-toolchain.toml` |

The MSVC toolchain is required, not preferred: the overlay links against Win32
and Direct2D, and `build.rs` needs `rc.exe` from the Windows SDK to embed the
icon and version resource.

1.85 is the highest MSRV among the dependencies, and CI has a job that builds on
exactly that version so the declaration cannot quietly become false.

## Runtime dependencies

Five direct dependencies. Twenty-four crates in the default graph once the
Windows bindings and serde's derive machinery are counted.

| Crate | Why |
| ----- | --- |
| `windows` | Win32, Direct2D, DirectWrite, GDI, shell bindings |
| `windows-numerics` | Matrix and vector types Direct2D expects |
| `arc-swap` | Lock-free telemetry publication |
| `serde`, `serde_json` | The rev-limit store, the vehicle catalog, and the world map |
| `tiny_http` | Only with `track_mapping`: the loopback viewer API |
| `veloryft-protocol` | Only with `track_mapping`: the world-map contract |
| `winresource` | Build-time only: embeds the icon and version block |

The full graph, with licenses, is in
[THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md).

## What is deliberately not here

**No async runtime.** There is no `tokio`, no `async-std`, nothing. The render
path must not allocate or schedule, and the one blocking socket is served by a
dedicated thread. An executor would add scheduling to the hot path and buy
nothing.

**No HTTP client, no TLS.** The default build makes no outbound connections, so
it needs neither. `reqwest`, `rustls`, `webpki-roots` and `x509-parser` were
present for the licensing subsystem that no longer exists here.

**No cryptography.** `ed25519-dalek`, `hmac`, `sha2`, `rand` and `base64` went
the same way. Nothing in the current application signs or verifies anything.

**No logging framework.** `diagnostics` is about a hundred lines: a level, a
bounded file with one rotation, and a one-shot latch for errors that recur every
frame. A framework would bring formatters, filters and appenders for a program
that writes a few dozen lines per session.

**No UI toolkit.** Direct2D draws the panels. A toolkit would add a layout
engine, an event loop and a widget tree to draw eleven shapes.

**No WebView2.** See [ADR 0001](../decisions/adr_0001_native_direct2d_overlay.md).

## Build configuration

**Static CRT.** `.cargo/config.toml` sets `-C target-feature=+crt-static`, so
the executable runs on a clean Windows 10 or 11 install with no Visual C++
Redistributable. See [ADR 0002](../decisions/adr_0002_static_crt.md).

**Release profile.** Fat LTO, one codegen unit, symbols stripped,
`panic = "abort"`. Optimized for a small standalone binary.

## Dependency policy

`deny.toml` enforces, on every pull request:

- **Sources:** crates.io only. Not git, not a path outside the workspace. This
  is what stops a private path dependency from reappearing.
- **Licenses:** an explicit permissive allow-list.
- **Advisories:** any unpatched vulnerability fails.
- **Bans:** wildcard versions are refused; duplicate versions warn.

Dependabot proposes updates weekly, grouping the `windows-*` family so that
those crates move together — mixed versions would put two copies of the same
Win32 bindings in one binary.

See [dependency management](../development/dependency_management.md) for how to
evaluate an update.
