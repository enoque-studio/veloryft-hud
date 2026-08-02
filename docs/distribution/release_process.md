# Release process

## Cutting a release

1. Update `version` in `Cargo.toml`.
2. Move the Unreleased section of `CHANGELOG.md` into the new version.
3. Commit.
4. Tag and push:

```bash
git tag v0.2.0-beta.1
```

```bash
git push origin v0.2.0-beta.1
```

That is the whole manual part. `.github/workflows/release.yml` does the rest.

## What the workflow does

| Step | Why |
| ---- | --- |
| 1. Validate version consistency | The tag must match `Cargo.toml`, or the artifacts would be named after a version nobody can reproduce from source |
| 2. `cargo fmt --check` | The same gate a pull request passes |
| 3. `cargo clippy -D warnings` | Same |
| 4. `cargo test --all-features` | Same |
| 5. `cargo build --release --locked` | Built from the committed lockfile |
| 6. Build the installer | `scripts/build_installer.ps1 -SkipBuild` |
| 7. Build the portable package | `scripts/build_portable.ps1 -SkipBuild` |
| 8. Generate `SHA256SUMS` | So users can verify an unsigned download |
| 9. Sign | Not yet — see [code signing](code_signing.md) |
| 10. Publish an immutable release | One per version tag; prereleases marked as such |
| 11. Attach release notes | Generated, plus verification instructions |

A release never ships something that would have been rejected on the way in.
Steps 2 to 4 are the reason.

## Artifacts

```
Veloryft-Setup-<version>.exe
Veloryft-Portable-<version>.zip
SHA256SUMS
```

Names carry the version and no channel label. There is no `-beta` or `-final`
suffix: the version says what it is, and the GitHub release is flagged as a
prerelease when it is one.

`SHA256SUMS` is in coreutils format — `<hash>  <name>`, two spaces, LF endings,
no BOM — so both `sha256sum --check` and PowerShell work.

## Requirements the workflow does not need

- No private repository.
- No personal access token.
- No license secrets.
- No files from outside this repository.

It runs with the automatic `GITHUB_TOKEN`, and `contents: write` only on the
job that creates the release. Everything else is read-only.

This matters beyond convenience: the previous workflows cloned a private
repository with a PAT, which meant nobody outside could reproduce a release, and
a fork could not run CI at all.

## Building a release locally

```powershell
pwsh scripts/build_installer.ps1
```

```powershell
pwsh scripts/build_portable.ps1 -SkipBuild
```

```powershell
pwsh scripts/generate_checksums.ps1
```

Artifacts land in `dist/`. `-SkipBuild` on the second call reuses the binary the
first one built.

To include the optional feature:

```powershell
pwsh scripts/build_installer.ps1 -Features track_mapping
```

## Dry run

Run the workflow manually with `dry_run` checked. Everything builds and the
artifacts are uploaded to the workflow run, but no release is published.

## After publishing

1. Download both artifacts from the release.
2. Verify their checksums against `SHA256SUMS`.
3. Install on a machine that does not have the toolchain, to confirm the static
   CRT is doing its job.
4. Run it against the game, or against the synthetic sender.
5. Confirm an upgrade over the previous version keeps the user's layout, theme
   and learned rev limits.

The last one is the important one. The installer `AppId` is what makes an
upgrade an upgrade rather than a second copy, and `tests/packaging.rs` pins it —
but the pin only proves the value did not change, not that the upgrade works.

## If a release is wrong

Publish a new version. Do not move the tag, and do not replace the assets on an
existing release.

Someone downloaded it. If the file behind a name changes, they have no way to
know, and no checksum they were given still means anything.

Mark the bad release as a prerelease or add a note pointing at its replacement.
