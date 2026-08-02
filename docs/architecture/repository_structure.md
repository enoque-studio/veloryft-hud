# Repository structure

```
veloryft-hud/
├── Cargo.toml              root package and workspace
├── Cargo.lock
├── build.rs                embeds the icon and version resource
├── deny.toml               allowed licenses, advisories, sources
├── rust-toolchain.toml     pinned toolchain and components
├── rustfmt.toml
│
├── src/
│   ├── main.rs             process entry point
│   ├── lib.rs
│   ├── version.rs          SemVer to Windows version rule (shared with build.rs)
│   ├── paths.rs            where per-user state lives
│   ├── diagnostics.rs      bounded logging and startup error reporting
│   ├── application/        startup orchestration and the authorization boundary
│   ├── telemetry/          UDP, decoding, the domain model, derived state
│   ├── overlay/            Win32 and Direct2D presentation
│   └── track_mapping/      optional: world-map recorder and loopback API
│
├── crates/
│   └── veloryft_protocol/  serialization contract shared with the backend
│
├── tests/                  integration, packaging, feature-specific
├── examples/               synthetic telemetry sender
├── data/                   car-ordinals.json, embedded at build time
├── assets/brand/           logo and wordmark sources
├── installer/              Inno Setup definition, EULA, generated artwork
├── scripts/                build, packaging, checksums
├── docs/
└── .github/                CI, release, issue and pull-request templates
```

## Why a workspace with two members

The root package is the application. The only other member is
`crates/veloryft_protocol`.

A workspace would not be justified for organization alone. It is justified here
because the protocol crate has an independent consumer: the Veloryft backend
deserializes the same world-map types. Both sides need one definition, versioned
on its own cadence, and neither repository should have to depend on the other's
source tree. See
[ADR 0003](../decisions/adr_0003_public_protocol_boundary.md).

Everything else stays in one crate. Splitting the overlay from the telemetry
would create a public API between two halves of the same binary, which buys
nothing and costs churn on every change.

## `src/overlay/`

`render.rs` was one 3,706-line file. It is now:

| Module | Owns |
| ------ | ---- |
| `runtime.rs` | Lifecycle, window procedures, the render loop |
| `window.rs` | Window creation, layered presentation, click-through |
| `graphics.rs` | Direct2D, DirectWrite and GDI resources |
| `frame_pacing.rs` | Frame intervals and adaptive degradation |
| `geometry.rs` | Panel identity, design sizes, rectangle math |
| `theme.rs` | Colors, glass settings, color conversion |
| `settings.rs` | Loading, validating and saving persisted state |
| `editing.rs` | Dragging, resizing, snapping |
| `tray.rs` | Tray icon and menu |
| `track_recording.rs` | Adapter to the optional track-mapping feature |
| `panels/mod.rs` | The `Panel` type: its window, surface and draw dispatch |
| `panels/cluster.rs` | RPM, gear, speed, pedals |
| `panels/tires.rs` | Per-corner temperature and session range |
| `panels/g_force.rs` | Lateral and longitudinal load |
| `panels/elevation.rs` | Lap elevation profile |
| `panels/throttle.rs` | The critical throttle bar and its thread |
| `panels/toolbar.rs` | Edit-mode toolbar: layout and hit testing |
| `panels/backdrop.rs` | Edit-mode grid |

The per-panel drawing code lives in separate files as additional `impl Panel`
blocks, so `Panel` keeps one definition while no single file owns the layout of
every panel.

## `src/telemetry/`

| Module | Owns |
| ------ | ---- |
| `receiver.rs` | The socket and its thread |
| `packet.rs` | Offsets, format detection, zero-copy accessors |
| `model.rs` | The domain frame |
| `snapshot.rs` | Lock-free publication (`ArcSwap`) |
| `display.rs` | Smoothing and the frame the panels draw from |
| `visibility.rs` | When the HUD should be on screen |
| `rev_limit.rs` | Learned shift points and limiters, per car |
| `tire_temperature.rs` | Session range and the temperature colour scale |
| `elevation.rs` | Lap elevation profile |
| `vehicle_catalog.rs` | Ordinal to car name |
| `synthetic.rs` | Building datagrams, for tests and the example |

No module here references Win32 or Direct2D, which is what allows the protocol
crate's tests — and, in principle, these — to run on any platform.

## Naming

Directories and files that this project controls use `lowercase_snake_case`.
Ecosystem-standard names keep their conventional spelling: `Cargo.toml`,
`README.md`, `LICENSE`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`,
`CODE_OF_CONDUCT.md`.

The GitHub repository and the Cargo package are `veloryft-hud` with a hyphen,
which is the Cargo convention; the Rust crate identifier is `veloryft_hud`.

## What is deliberately absent

- **`src-tauri/`.** The application stopped being a Tauri project some time ago.
  The directory is gone, not kept for history — that is what git is for.
- **A `VERSION` file.** The version comes from `Cargo.toml`. See
  [versioning](../distribution/versioning.md).
- **Vendored dependencies.** Everything comes from crates.io, and `deny.toml`
  refuses any other source, which is what stops a private path dependency from
  creeping back in.
