# Dependency management

Veloryft has few dependencies on purpose: five direct, twenty-four in the
default graph including the Windows bindings and serde's derive machinery. Every
direct dependency is something that would be worse to write by hand.

## Rules

**crates.io only.** Not a git dependency, not a path outside the workspace.
`deny.toml` enforces this, and it is the specific thing that stops a private
path dependency from reappearing — that is what made this repository
unbuildable from a clean clone in the first place.

**Permissive licenses only.** The allow-list is in `deny.toml`. A copyleft
dependency would change the terms under which the binary can be distributed.

**No unpatched advisories.** `cargo deny check` fails on any.

**Feature-specific dependencies are optional.** `tiny_http` and
`veloryft-protocol` are `optional = true` and pulled in by `track_mapping`
through `dep:`. A dependency compiled but never reached is still attack surface,
still build time, and still something to audit.

## Before adding one

Ask, in order:

1. **Is it needed at all?** `diagnostics` is a hundred lines instead of a
   logging framework, because the program writes a few dozen lines per session.
2. **What does it pull in?** Check `cargo tree` for the crate before adding it.
   One convenient wrapper can bring thirty transitive crates.
3. **Does it touch the hot path?** Nothing that allocates or schedules belongs
   in the render loop. An async runtime is not acceptable there.
4. **Is it maintained?** Recent releases, a responsive issue tracker, more than
   one person who could fix it.
5. **Its license?** It must already be in the `deny.toml` allow-list, or adding
   it must be a deliberate decision recorded there.

Then add it, run `cargo deny check`, and update
[THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md).

## Updating

Dependabot proposes updates weekly. Minor and patch bumps are grouped so a
routine week is one pull request rather than fifteen; security fixes stay
separate so they are never buried.

The `windows-*` family is grouped separately and must move together. Mixed
versions put two copies of the same Win32 bindings in one binary, which inflates
it and can produce type mismatches that are painful to diagnose.

**Do not update everything because updates exist.** For each one:

1. Read the changelog. Understand what changed, not just that something did.
2. Run `cargo test --workspace --all-features`.
3. For anything touching rendering or Win32, run the overlay and look at it. A
   test suite cannot tell you the HUD renders correctly.
4. Note any compatibility risk in the pull request.

Major version bumps are separate pull requests, one at a time, with the
migration explained.

## The lockfile

`Cargo.lock` is committed. This is an application, not a library, and a
reproducible build matters more than picking up patch releases silently.

Release builds use `--locked`, so a release is always built from exactly the
committed graph.

## Auditing

```bash
cargo deny check
```

```bash
cargo audit
```

```bash
cargo tree --duplicates
```

`cargo deny check` is what CI runs. `cargo tree --duplicates` shows crates
present at more than one version — usually harmless, but worth understanding
before it grows.

## Removing one

Removing a dependency is as valuable as adding one is expensive. The 0.2 release
dropped fourteen: cryptography, TLS, an HTTP client, WMI and an async runtime,
all of which existed for a licensing subsystem the beta never ran.

When you remove one, update `THIRD_PARTY_NOTICES.md` and say so in the
changelog.
