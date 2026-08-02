# Windows installer

Built with [Inno Setup 6](https://jrsoftware.org/isinfo.php) from
`installer/veloryft.iss`.

## Building it

```powershell
pwsh scripts/build_installer.ps1
```

Output: `dist/Veloryft-Setup-<version>.exe`.

| Flag | Effect |
| ---- | ------ |
| `-SkipBuild` | Package the existing `target/release/veloryft.exe` |
| `-Features track_mapping` | Build with the optional feature enabled |

The script stops any running Veloryft first, because Windows will not let the
installer replace a locked executable.

If `ISCC.exe` is not found, install Inno Setup:

```powershell
winget install --id JRSoftware.InnoSetup
```

## What gets installed

| File | Installed as |
| ---- | ------------ |
| `target/release/veloryft.exe` | `Veloryft.exe` |
| `installer/README.txt` | `Read Me.txt` |
| `installer/EULA.txt` | `License Agreement.txt` |

Plus Start menu shortcuts, an uninstaller, and an optional desktop icon that is
unchecked by default.

The executable is renamed on install. The build output is `veloryft.exe`, the
installed file is `Veloryft.exe`, and the portable package uses the installed
name so a user moving between the two keeps the same shortcut target.

## Install mode

`PrivilegesRequired=lowest`, so it installs per-user without an administrator
prompt. The user can still choose an all-users install in the privileges dialog.

Requires 64-bit Windows 10 or later (`ArchitecturesAllowed=x64compatible`,
`MinVersion=10.0`).

## The AppId

```
{{A7F3C2E1-9B4D-4E6A-8C1F-2D5B7E0A9C34}
```

**This must never change.** It is the identity Windows uses to recognize an
existing installation. Changing it makes every install invisible to the new
installer, so users end up with two copies, two uninstall entries, and no
upgrade path.

`tests/packaging.rs` pins the value.

If you fork the project and distribute your own builds, you **must** change it —
otherwise your installer would overwrite an official install. Generate a fresh
GUID and change the product name too. See
[TRADEMARKS.md](../../TRADEMARKS.md).

## Versions

The script passes two values from `Cargo.toml`:

| Define | Value | Used for |
| ------ | ----- | -------- |
| `MyAppVersion` | `0.2.0-beta.1` | Display, and the output file name |
| `MyAppVersionInfo` | `0.2.0.1` | The Windows version block, which needs four numbers |

The `.iss` refuses to compile if either is missing, so the installer can never
be built with a version that was not derived from the manifest. See
[versioning](versioning.md).

## Artwork

Light and dark wizard images, and the setup icon, from `installer/assets/`.
Inno Setup reads the system theme at startup and keeps the wizard in step with
it.

Those files are **generated** from the sources in `assets/brand/`:

```bash
python scripts/generate_installer_assets.py
```

Requires Pillow. Regenerate after changing the brand artwork; do not hand-edit
the `.bmp` files.

## Upgrades and uninstall

Installing over an existing version replaces the files in place, keeping
shortcuts and the uninstall entry. Both are closed first, by `PrepareToInstall`
and `InitializeUninstall`, which terminate `Veloryft.exe`, `veloryft.exe`, and
the pre-0.2 `forza-hud-native.exe`.

**Uninstalling leaves `%APPDATA%\forza-hud-native` behind, deliberately**, so
that reinstalling keeps the user's layout, theme and learned rev limits. Users
who want a clean slate are told how in
[installation](../getting_started/installation.md).

## The EULA

`installer/EULA.txt`, which the wizard requires the user to accept.

It covers the official prebuilt binary: warranty disclaimers, beta status,
privacy behaviour, and trademark notices. It explicitly does **not** restrict
what Apache-2.0 grants for the source code.

**It has not been reviewed by a lawyer**, and is marked as a draft. See
[the licensing decision](../legal/licensing_decision.md).

## No license gate

The installer never asks for a key, an account, or a connection. The
`[Code]` section only closes a running instance.

Any older document describing an activation step is wrong.
