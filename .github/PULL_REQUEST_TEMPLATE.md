# What this changes

<!-- What the change does, and why. The diff shows what; explain why. -->

Closes #

## How it was verified

<!--
Say what you actually ran. "Should work" is not verification.

For anything visible in the HUD, the synthetic sender is enough — the game is
not required:

    cargo run
    cargo run --example synthetic_telemetry
-->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features`
- [ ] Ran the overlay and confirmed the change on screen

## Checklist

- [ ] One concern per pull request
- [ ] New behaviour has a test, or the pull request explains why it cannot
- [ ] Documentation updated if user-visible behaviour changed
- [ ] `CHANGELOG.md` updated under Unreleased, if a user would notice

## Compatibility

<!-- Tick anything this touches, and say how compatibility is preserved. -->

- [ ] Changes a persisted settings file or key
- [ ] Changes or adds an environment variable
- [ ] Changes packet offsets or the telemetry model
- [ ] Changes a serialized type in `crates/veloryft_protocol`
- [ ] Changes the installer, its `AppId`, or the release artifacts
- [ ] None of the above

## Render hot path

<!--
Only if this touches src/overlay/. See CONTRIBUTING.md.
Delete this section otherwise.
-->

- [ ] No allocation added to the render loop or the throttle thread
- [ ] No new locking on the throttle path
- [ ] No logging added per frame
- [ ] Frame pacing and the critical throttle cadence are unchanged, or the
      change is justified by measurements included below
