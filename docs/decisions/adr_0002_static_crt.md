# ADR 0002 — Static C runtime

**Status:** Accepted
**Date:** 2026-06

## Context

An MSVC-built Rust binary links the Visual C++ runtime dynamically by default,
which means it needs `vcruntime140.dll` and friends on the target machine.

That DLL is not part of a clean Windows install. It arrives with the Visual C++
Redistributable, which most machines have because something else installed it —
but "most" is not "all", and the failure mode is a dialog about a missing DLL
before any of our code runs. There is no opportunity to explain anything.

Veloryft is distributed two ways, and this affects both:

- An installer that should not have to bundle and install a redistributable.
- A portable zip that must run from a USB stick, where installing anything
  defeats the purpose.

## Decision

Link the C runtime statically:

```toml
# .cargo/config.toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

The executable then depends only on DLLs that ship with Windows itself:
`kernel32`, `user32`, `gdi32`, `d2d1`, `dwrite`, `shell32`.

## Alternatives rejected

**Bundle the redistributable in the installer.** Makes the installer larger,
adds a step that can fail, needs elevation, and does nothing for the portable
package.

**Document it as a prerequisite.** Pushes the problem onto users, who would hit
a missing-DLL dialog and have no way to connect it to a documentation page they
never read. A HUD is not the kind of software people troubleshoot patiently.

**Check at startup and prompt.** Cannot work: the check would be in a binary
that cannot start.

## Consequences

**Good.** The executable runs on a clean Windows 10 or 11 install. The portable
package is genuinely portable. The installer has one step. There is no
dependency on a system-wide component another program could downgrade or remove.

**Bad.** A larger binary — the runtime is inside it rather than shared.
Acceptable for a single-executable application; it would not be for a suite of
them.

Security fixes to the C runtime do not arrive through Windows Update. If one
matters, Veloryft must be rebuilt and re-released. That is a real obligation,
and it is why it is written down here rather than left implicit.

Every Rust dependency must tolerate a static CRT. In practice this rules out
crates that link C libraries expecting the dynamic runtime. Nothing in the
current graph links C, so the constraint costs nothing today — but it is a
constraint on what may be added.

Mixing static and dynamic CRT in one binary produces link errors or, worse,
allocations freed by the wrong allocator. If a dependency ever brings a
dynamically-linked C library, that is a genuine incompatibility and not something
to work around.

## Validation

`cargo build --release` must succeed and produce a runnable executable, which CI
checks on every push.

Before a release, the artifact is installed on a machine without the Visual C++
Redistributable and without a Rust toolchain. That is the only test that proves
this decision still holds, and it is in the
[release checklist](../distribution/release_process.md).

The setting applies to `x86_64-pc-windows-msvc`. Building for a different target
will not pick it up.

## Revisit if

- Veloryft ever ships more than one executable, where sharing the runtime would
  actually save something.
- A dependency worth having requires a dynamically-linked C library.
- The binary size becomes a real problem, which at present it is not.
