# ADR 0003 — An in-repository protocol crate instead of a private path dependency

**Status:** Accepted
**Date:** 2026-08

## Context

`Cargo.toml` carried:

```toml
veloryft-core = { path = "../../veloryft-back/crates/core" }
```

`veloryft-back` is a private repository. The consequences were not subtle:

- `cargo build` failed on a clean clone. Nobody outside could compile the
  project at all.
- CI cloned the private repository with a personal access token, so no fork
  could run CI and no outsider could reproduce a release.
- Contributors would have had to edit `Cargo.toml` to build, which then shows up
  in every diff.

The whole of `veloryft-core` was not needed. The HUD imported exactly three
items — `HeightCell`, `WorldHeightmap`, `SURFACE_KINDS` — all serialization
types for the world map, and all used only by track mapping. The rest of the
crate (JWS signing, license payloads, lap upload DTOs) was never referenced.

## Decision

Move those world-map types into a new crate in this repository:

```
crates/veloryft_protocol/
```

published as `veloryft-protocol`, with its own version, its own compatibility
policy, and tests that pin the serialized field names.

It is an optional dependency, enabled by the `track_mapping` feature, so a
default build does not compile it at all.

## Why a separate crate rather than a module

The [structure guidance](../architecture/repository_structure.md) says a
workspace needs justification. This is the justification:

**The types have a second consumer.** The Veloryft backend deserializes the same
world map when a client uploads one. Producer and consumer must agree, and the
agreement has to live somewhere both can point at.

**It needs to version independently.** A change to how the HUD draws is not a
change to the wire format. Conflating them would mean either version churn in
the contract or an ambiguous signal to the backend about whether anything
changed.

**It has a genuinely different character.** No I/O, no platform code,
`#![forbid(unsafe_code)]`, and it compiles anywhere. The application is
Windows-only and full of `unsafe`.

A plain module inside the application would have satisfied the HUD and left the
backend with nothing to depend on except a copy.

## Alternatives rejected

**Publish `veloryft-core` as a public crate.** Would have exposed the licensing
and lap-upload types, which belong to a private product. Publishing a crate to
solve a build problem, and taking on its maintenance and its name on crates.io,
is a large commitment for three structs.

**Duplicate the types in both repositories.** Two definitions that must agree
and no mechanism to make them. This is the option that looks cheapest and costs
the most, and it is why the compatibility policy below exists instead.

**Keep the path dependency and document the workaround.** Does not fix anything.
A public repository that cannot be built from a clean clone is not a public
repository.

## Compatibility policy

The crate is the contract, so the rules are explicit and tested:

- Fields added after first release carry `#[serde(default)]`, so an older map
  file or an older client still deserializes.
- Adding a field is a minor version bump.
- Removing or renaming a field, or changing its meaning, is a major bump.
- `SURFACE_KINDS` indices are wire values and are never reordered.

Two tests enforce this: one asserts the exact serialized field names of
`HeightCell`, another loads a map written before the optional fields existed. A
failure there is a compatibility break, not a test to update.

## How the backend stays in step

The backend depends on `veloryft-protocol` by version, from this repository.
When the contract changes, the version changes, and the backend updates
deliberately rather than by having a file recompiled underneath it.

If the crate is ever published to crates.io, the backend pins an immutable
version and nothing else changes.

## Consequences

**Good.** A clean clone builds with no other repository and no token. CI runs on
forks. The contract has one definition, a stated policy, and tests that enforce
it. Fourteen dependencies left with the licensing subsystem the same change
removed.

**Bad.** A workspace where a single crate would otherwise do. The cost is one
extra `Cargo.toml` and a second `cargo doc` target, which is small but not zero.

Changing the contract now requires touching two repositories in the right order:
publish or bump here, then update the backend. That is slower than editing a
shared file — and it is the point. The friction is what makes a breaking change
visible.

## Revisit if

- The backend stops consuming these types, in which case they become a plain
  module here.
- Track mapping is removed, in which case the crate goes with it.
- More genuinely shared contract types appear, which would justify publishing
  the crate rather than depending on it by path.
