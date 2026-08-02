# Versioning

## One source of truth

The `version` field in `Cargo.toml`. Nothing else.

```toml
[package]
version = "0.2.0-beta.1"
```

Everything that needs a version derives it from there:

| Consumer | How |
| -------- | --- |
| The binary | `CARGO_PKG_VERSION` |
| The Windows version resource | `build.rs`, via `version::windows_resource_version` |
| Installer and portable file names | `cargo metadata`, via `scripts/common.ps1` |
| The installer's version block | Passed to Inno Setup by `scripts/build_installer.ps1` |
| The release tag check | `.github/workflows/release.yml` |

There used to be a separate `VERSION` file holding a four-component number,
maintained by hand alongside the Cargo version. It is gone. Two numbers that
must agree, edited in two places, eventually disagree.

## Semantic versioning

[SemVer 2.0.0](https://semver.org/spec/v2.0.0.html). While the major version is
`0`, the minor version carries breaking changes.

A change is breaking if it would surprise an existing user:

- Renaming or removing a settings key, or the configuration directory.
- Renaming or removing an environment variable.
- Changing a serialized shape in `crates/veloryft_protocol`.
- Removing a panel or a shortcut.

Prereleases use `-beta.N` or `-rc.N`.

## The Windows version resource

Windows version resources are four numbers and have no concept of a prerelease.
The mapping is:

```
major.minor.patch[-prerelease.N]  →  major.minor.patch.N
```

with `0` in the fourth component for a final release.

| Cargo version | Windows resource |
| ------------- | ---------------- |
| `0.2.0` | `0.2.0.0` |
| `0.2.0-beta.1` | `0.2.0.1` |
| `1.0.0-rc.4` | `1.0.0.4` |
| `1.0.0-alpha` | `1.0.0.0` |

### The ordering caveat

`0.2.0-beta.1` becomes `0.2.0.1`, which sorts **above** the eventual `0.2.0`
(`0.2.0.0`). SemVer says the opposite.

This is cosmetic. Inno Setup identifies an installation by its `AppId`, not by
this number, so upgrades are unaffected — the only place the inversion is
visible is the file's Properties dialog.

The rule is implemented once, in `src/version.rs`, which `build.rs` includes so
that both share an implementation and a test suite. `scripts/common.ps1` has a
PowerShell mirror for the packaging scripts, and `tests/packaging.rs` pins the
cases where the two could drift apart.

## Tags

Immutable, one per version, prefixed with `v`:

```
v0.1.0
v0.2.0-beta.1
v0.2.0
```

**A tag is never moved.** If a release is wrong, publish a new version. Moving a
tag means someone who downloaded yesterday and someone who downloads today have
different files under the same name, and neither can tell.

There is no rolling `beta` tag. There used to be, and one mutable tag cannot be
a release history.

The release workflow refuses to run if the tag does not match the version in
`Cargo.toml`, so an artifact can never be named after a version nobody can
reproduce from source.

## Prereleases and "latest"

A version with a prerelease suffix is published as a GitHub prerelease, so
GitHub's "latest release" keeps pointing at the newest stable build.

If the website needs a stable download URL, use one of:

- GitHub's `releases/latest` endpoint for stable builds.
- The API, filtered by `prerelease`, for the newest beta.
- A small version manifest published alongside the release.
- A redirect the website controls.

Do not sacrifice immutable release history to keep one fixed URL. A rolling
alias in addition to immutable releases is fine; a rolling alias *instead of*
them is not.

## Cutting a release

1. Update `version` in `Cargo.toml`.
2. Update `CHANGELOG.md`: move Unreleased into the new version.
3. Commit.
4. Tag `v<version>` and push the tag.

The rest is [the release process](release_process.md).
