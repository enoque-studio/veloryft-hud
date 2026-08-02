# Optional features

Veloryft has one Cargo feature. The default build enables nothing.

```toml
[features]
default = []
track_mapping = ["dep:tiny_http", "dep:veloryft-protocol"]
```

There is no `native_hud` feature, because the overlay is the application — a
feature that can never be turned off would be a lie in the manifest.

## `track_mapping`

Records a sparse world-height map while you drive, and serves it to a 3D viewer
over a loopback HTTP API.

**Off by default. The released binaries do not contain it.**

### What it adds when enabled

| | |
| --- | --- |
| Modules | `track_mapping::world_map`, `track_mapping::local_api` |
| Dependencies | `tiny_http`, `veloryft-protocol` |
| Threads | `worldmap-autosave`, `worldmap-live-api` |
| Listening socket | `127.0.0.1:9877` (override with `VELORYFT_LIVE_PORT`) |
| Files written | `worldmap-fh6.json`, `worldmap-fh6.ndjson`, `worldpath-fh6.json` |

### What the default build therefore does

Nothing of the above. **The default build opens no listening socket at all** —
not a closed one, not one bound to loopback. The code that would open it is not
in the binary.

That is the point of using a feature rather than a runtime flag. A runtime
`if enabled` still compiles the server, still links `tiny_http`, and still
leaves the code present for anyone to reach through a bug. This was previously a
`const TRACK_MAPPING_ENABLED: bool = false`, and that is exactly what it did.

### Enabling it

```bash
cargo run --features track_mapping
```

```bash
cargo test --features track_mapping
```

```powershell
pwsh scripts/build_installer.ps1 -Features track_mapping
```

### Security

The local API has no authentication, by design: it is a development aid for a
viewer running on the same machine. Its protections are that it binds loopback
only, rejects browser `Origin` headers that are not `localhost` or `127.0.0.1`,
and caps request bodies at 16 KiB.

Any local process can call it. The threat model is written up in
[local HTTP server](../security/local_http_server.md), and
[ADR 0004](../decisions/adr_0004_track_mapping_feature.md) records why it is off
by default.

## Testing both configurations

CI runs `check`, `clippy` and `test` twice: once with default features and once
with `--all-features`. A feature that compiles only when enabled, or only when
disabled, is a break waiting for a release, and running one configuration would
not catch it.

`tests/track_mapping.rs` opens with `#![cfg(feature = "track_mapping")]`, so the
whole file compiles away in a default build — which is itself part of what is
being verified.

## Adding a feature

Before adding one, be sure a feature is what you want:

- **A feature** is right when code, dependencies, or an attack surface should be
  absent from a build. Compiling something and never running it is not the same
  as not compiling it.
- **A setting** is right when the user should be able to change their mind
  without a new binary.
- **Neither** is right for something everyone should have. An option nobody
  turns off is a way of avoiding a decision.

If you add one, make its dependencies optional with `dep:`, gate its modules in
`lib.rs`, add its tests behind the same `cfg`, and document it here.
