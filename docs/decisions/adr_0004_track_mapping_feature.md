# ADR 0004 — Track mapping behind a Cargo feature, off by default

**Status:** Accepted
**Date:** 2026-08

## Context

Track mapping records a sparse world-height map while you drive and serves it to
a 3D viewer over a loopback HTTP API. It is unfinished: the viewer, the backend,
and the upload path are not ready.

It was disabled with a runtime constant:

```rust
pub const TRACK_MAPPING_ENABLED: bool = false;
```

That reads as "off", and it is not. With the constant false:

- The recorder and the HTTP server were **still compiled** into every release.
- `tiny_http` was **still linked**.
- The private `veloryft-core` dependency was needed **only** by this code, so it
  was what made the whole project unbuildable from a clean clone.
- The routes existed in the binary. They were unreachable through the intended
  path, which is a different thing from absent.

Shipping a disabled HTTP server in a consumer application is a poor trade: all
of the attack surface, none of the function.

## Decision

Make it a Cargo feature.

```toml
[features]
default = []
track_mapping = ["dep:tiny_http", "dep:veloryft-protocol"]
```

Its modules are gated in `lib.rs`; its dependencies are `optional = true` and
pulled in through `dep:`. The runtime constant is gone.

`overlay::track_recording` gives the render loop one type with the same four
methods either way. With the feature off, `TrackRecording::start` returns `None`
and the recorder does not exist to be started.

## Why a feature rather than a runtime setting

A runtime setting is right when a user should be able to change their mind
without a new binary. That is not the situation here.

**The code should not be present.** A compiled-but-unreachable HTTP server is
still bytes in the binary, still a dependency to audit, still something a bug
elsewhere could reach. "Not compiled" is a stronger statement than "not called",
and it is the one worth being able to make.

**The dependencies should not be present.** `tiny_http` and the protocol crate
exist for this and nothing else. An optional feature removes them from a default
build's graph entirely.

**The default build should open no socket.** Not a closed one, not a
loopback-bound one. None. That is now a fact about the binary, not a promise
about a code path.

## Alternatives rejected

**Delete it.** The recorder works and is tested; the viewer is what is missing.
Deleting working code because its consumer is late is how projects rebuild the
same thing twice.

**Enable it and hide the UI.** Would ship a listening socket to every user for a
feature none of them can use.

**A separate binary.** Would duplicate the telemetry pipeline, or need a library
boundary between two halves of one application, for a development aid.

## Consequences

**Good.** The default build has no HTTP server, no listening socket, and two
fewer dependencies. The private path dependency became removable, which is what
unblocked building from a clean clone. The security story is a fact, not a
claim.

**Bad.** Two configurations to keep working. Code that compiles only when the
feature is on — or only when it is off — is a break waiting for a release, so CI
runs `check`, `clippy` and `test` twice, and the build takes longer for it.

Anyone working on track mapping must remember `--features track_mapping`, and
will otherwise see confusing "unresolved module" errors. The
[development setup](../architecture/optional_features.md) says so, and editors
should be configured with all features enabled.

## Enabling it

```bash
cargo run --features track_mapping
```

```bash
cargo test --features track_mapping
```

```powershell
pwsh scripts/build_installer.ps1 -Features track_mapping
```

## Security when enabled

The local API binds `127.0.0.1` only, rejects browser `Origin` headers that are
not loopback, and caps request bodies at 16 KiB. It has **no authentication**:
any local process can call it, including the routes that change state.

That is acceptable for a development aid and would not be acceptable in a
release, which is the other half of why this is off by default. The full threat
model is in [local HTTP server](../security/local_http_server.md).

## Revisit if

- The viewer and the upload path ship, at which point the feature becomes a
  user-facing option — and the API needs a real authentication story before it
  is on by default anywhere.
- The recorder stops being useful, at which point delete it rather than leaving
  a feature nobody enables.
