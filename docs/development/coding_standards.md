# Coding standards

Rust conventions apply. This page covers what is specific to Veloryft.

## Naming

| Kind | Style |
| ---- | ----- |
| Crates, modules, functions, variables, source files | `snake_case` |
| Types and traits | `PascalCase` |
| Constants and statics | `SCREAMING_SNAKE_CASE` |
| Directories | `lowercase_snake_case` |

Everything technical is in English: identifiers, comments, doc comments, test
names, commit messages, documentation.

Names should say what a thing is, not what type it has. Prefer `cause` to `e`,
`frame` to `t`, `catalog` to `map`. The exception is a genuinely conventional
short name in a tight numeric loop.

## Visibility

Private by default. Prefer `pub(crate)` to `pub`.

Only three things are `pub` in the library: `application`, `telemetry`, and the
small `paths`, `diagnostics` and `version` modules. The overlay is `pub(crate)`,
because it is reached through `application::run` and nothing outside should
drive it directly.

## Imports

Explicit. `use windows::Win32::Foundation::{HWND, POINT}`, not
`use windows::Win32::Foundation::*`.

Wildcards make it impossible to tell where a name came from, and in the Win32
crate the namespaces are large and overlapping. A handful of glob imports remain
where a module uses most of a namespace; do not add more.

Group imports: standard library, then external crates, then `crate::` and
`super::`. `cargo fmt` maintains the ordering within a group.

## Comments

Comment **why**, not what. The code says what it does.

```rust
// Without SWP_FRAMECHANGED the new ex-style is not re-evaluated during
// hit testing, so click-through would not actually change.
```

not

```rust
// Call SetWindowPos.
```

Doc comments on anything `pub` or `pub(crate)`. Modules get a `//!` header
explaining what they own and, where it matters, what they deliberately do not.

Comments that would be obvious to anyone reading the line are noise. Comments
that record a decision — why a threshold is 12 and not 20, why a value is
clamped on load rather than on save — are the ones worth writing, because the
next person will otherwise "simplify" them away.

## Unsafe

Win32 and Direct2D are unsafe APIs, so `src/overlay/` contains `unsafe`.

- Keep it in `src/overlay/`. `src/telemetry/` has none;
  `crates/veloryft_protocol` sets `#![forbid(unsafe_code)]`.
- Keep blocks small: wrap the Win32 call, not the logic around it.
- Each module that uses `unsafe` states its shared invariants once, in the
  module header — handle ownership, thread affinity, pointer lifetimes.
- Add a `// SAFETY:` comment on any block whose reasoning goes beyond those
  shared invariants: raw pointers, values stored in and read back out of window
  user data, anything where the argument is not "this is a Win32 call with valid
  handles".
- An `unsafe fn` needs a `# Safety` section stating what the caller must
  guarantee.

## Error handling

The library does not panic on anything a user can cause. Malformed input,
missing files, unavailable resources — all handled.

`expect` is acceptable for an invariant the code itself guarantees, and its
message should state the invariant:

```rust
.expect("the static panel definition has a matching renderer")
```

Failures that reach a user get an actionable message. "The Veloryft overlay
could not be created. This usually means Direct2D is unavailable — update your
graphics driver" is useful. "Error: 0x80004005" is not.

Failures reachable on a per-frame path are latched with `diagnostics::Once`.

## The render hot path

`src/overlay/` runs every frame while someone is driving. Inside the render loop
and the throttle thread:

- No allocation.
- No locking beyond the single placement mutex.
- No logging.
- No async runtime.

Move work out of the loop rather than guarding it inside the loop. See
[rendering pipeline](../architecture/rendering_pipeline.md).

## Tests

Unit tests live beside the code, in `#[cfg(test)] mod tests`. Integration tests
live in `tests/`.

Test names are sentences describing the behaviour:

```rust
fn a_layout_without_the_visible_field_still_loads()
fn soft_snap_grabs_only_inside_the_magnet_radius()
```

not `test_load` or `test_snap`. When one fails, the name should tell you what
broke without opening the file.

Assert the behaviour, not the implementation. Give an assertion a message when
the failure would otherwise be a bare `false`.

See [testing strategy](testing_strategy.md).

## Lints

`cargo clippy --all-targets --all-features -- -D warnings` must pass. Warnings
are errors in CI.

If a lint is genuinely wrong for a line, add a narrow `#[allow(...)]` with a
comment explaining why:

```rust
// One scalar per smoothed channel. Grouping them into a struct would add a
// copy on a path that runs every frame, for no readability gain.
#[allow(clippy::too_many_arguments)]
```

Never a module-wide or crate-wide allow. `render.rs` carried
`#![allow(clippy::too_many_lines)]` for a long time, and it hid exactly the
problem the lint exists to point at.

## Compatibility

Some things are contracts with users and cannot be changed casually:

- The `%APPDATA%\forza-hud-native` directory name and every file in it.
- Settings keys, and the ability to load files written by older releases.
- Environment variable names.
- Packet offsets — see [contributing](../../CONTRIBUTING.md).
- Serialized shapes in `crates/veloryft_protocol`.
- The installer `AppId`.

Each of these has a test. If one fails, you have made a compatibility break;
decide deliberately rather than updating the test.
