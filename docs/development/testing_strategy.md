# Testing strategy

```bash
cargo test --workspace --all-features
```

## Categories

| Category | Where | Covers |
| -------- | ----- | ------ |
| Unit | `#[cfg(test)]` beside the code | One module's logic |
| Integration | `tests/telemetry_pipeline.rs` | The pipeline end to end, over a real socket |
| Regression | Throughout, named for the behaviour | Formats and behaviours that must not change |
| Feature-specific | `tests/track_mapping.rs` | The optional feature, when enabled |
| Packaging smoke | `tests/packaging.rs` | The build and release inputs |

## Fixtures

`telemetry::synthetic` builds Sled, Dash and Horizon datagrams from plain
numbers. Every test that needs telemetry uses it.

It sits next to the parser deliberately: both read the same offsets, and
round-trip tests fail if the two ever disagree. That is the mechanism that stops
a change to `packet.rs` from silently skipping the encoder.

**No captured telemetry ships with this project, and none may be added.** A
capture identifies a person's driving, and we cannot license someone else's.
Generated fixtures are deterministic, reviewable, and carry no personal data.

## Integration tests

`tests/telemetry_pipeline.rs` runs a real UDP socket through the real parser
into the real snapshot. It exists to catch what unit tests structurally cannot —
a datagram that parses but never reaches the snapshot, a counter that does not
advance, a malformed packet that takes the receive thread down with it.

Each test asks the OS for a free port and retries if the receiver loses the race
to bind it, so the suite runs in parallel and on a busy machine without flaking.
Waiting is bounded by a generous timeout: these assert liveness, not latency.

Assertions about UDP tolerate loss. Loopback rarely drops, but "at least three
of five arrived" is what the transport actually promises, and a test that
assumes otherwise fails for the wrong reason.

## Regression tests

Some tests exist purely because changing the thing they check would break
existing users. They say so in a comment:

- Panel settings keys (`cluster`, `tires`, `gforce`, `elevation`) — changing one
  silently discards a user's layout.
- Loading a `layout.txt` written before the `visible` field existed.
- Clamping of hand-edited theme values, so a bad file cannot leave the overlay
  in a state the UI cannot undo.
- Serialized field names in `crates/veloryft_protocol`, which every saved map
  file and the backend depend on.
- The installer `AppId`, which is the identity Windows uses for upgrades.

If one fails, do not update it to match the new behaviour. Decide whether the
break is intended, and if it is, ship a migration.

## Packaging smoke tests

`tests/packaging.rs` does not build an installer — that needs Inno Setup and a
release binary. It checks what silently rots between releases: the installer
pointing at a binary name that changed, an asset that was renamed, the
PowerShell and Rust version rules drifting apart, a leftover `src-tauri` path.

These would otherwise surface as a broken release artifact, on a tag, at the
worst possible moment.

## Feature-specific tests

`tests/track_mapping.rs` begins with `#![cfg(feature = "track_mapping")]`, so
the whole file compiles away in a default build. That is part of what is being
verified.

CI runs the suite twice, with default features and with `--all-features`.

## What is not tested automatically

**Rendering.** Nothing asserts what appears on screen. There are no pixel
comparisons, and adding them would mean maintaining reference images across
driver and Windows versions.

Visual changes are verified by hand:

```bash
cargo run
```

```bash
cargo run --example synthetic_telemetry
```

If you change anything in `src/overlay/`, run this and look at it. Say in the
pull request that you did.

**The installer.** Building it needs Inno Setup, so CI does not, and the release
workflow is the first thing that runs it end to end. The smoke tests cover its
inputs; the artifact itself is verified by installing it.

**Multi-monitor and DPI behaviour.** No runner has two monitors at different
scale factors. `soft_snap` is tested with negative coordinates because that is
where the arithmetic goes wrong, but the rest needs a person.

These gaps are real. They are written down rather than papered over with tests
that would assert nothing.

## Writing a test

Name it for the behaviour, in a sentence:

```rust
fn a_hand_edited_theme_cannot_make_the_glass_unusable()
```

Assert what a user would notice, not how the code is arranged. Give an assertion
a message when a bare `false` would not explain itself:

```rust
assert!(
    !visibility::is_hud_visible(&snapshot, 0.0),
    "movement without driver input must not show the HUD"
);
```

If a behaviour cannot be tested, that usually means a boundary is in the wrong
place. Say so in the pull request rather than skipping it quietly.
