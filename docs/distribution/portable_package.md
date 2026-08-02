# Portable package

A zip containing the executable, quick start, license, and notices. Nothing is
installed.

## Building it

```powershell
pwsh scripts/build_portable.ps1
```

Output: `dist/Veloryft-Portable-<version>.zip`.

| Flag | Effect |
| ---- | ------ |
| `-SkipBuild` | Package the existing `target/release/veloryft.exe` |
| `-Features track_mapping` | Build with the optional feature enabled |

## Contents

```
Veloryft.exe
Read Me.txt
LICENSE.txt
NOTICE.txt
THIRD_PARTY_NOTICES.md
```

That is all of it. The executable is renamed from `veloryft.exe` to match the
installed name, so a user moving between the two packages keeps the same
shortcut target and the same tray identity.

## Why it works with no supporting files

Two build decisions make this possible:

- **Configuration is optional.** Without a user-supplied vehicle catalog, the
  HUD displays car ordinals and needs no `data/` folder.
- **The C runtime is linked statically**, so there is no Visual C++
  Redistributable to install. See
  [ADR 0002](../decisions/adr_0002_static_crt.md).

The same executable therefore runs from a USB stick, a Desktop folder, or
anywhere else.

## Settings are not portable

This is the one thing that surprises people.

Settings go to `%APPDATA%\forza-hud-native`, the same place the installed
version uses. A portable copy is **not** isolated:

- Running it does not carry your layout to another machine.
- It shares settings with an installed copy on the same machine.
- Deleting the folder does not remove your settings.

That is deliberate. Someone who tries the portable build and then installs
properly keeps everything, which is the common case. See
[configuration files](../reference/configuration_files.md) for how to move
settings between machines by hand.

## Portable or installer

| | Portable | Installer |
| --- | --- | --- |
| Start menu entry | No | Yes |
| Uninstaller | No | Yes |
| Runs from removable media | Yes | No |
| Administrator rights | No | No |
| Settings location | Same | Same |
| The executable | Identical | Identical |

## Verifying it

Releases are unsigned, so verify the checksum:

```powershell
(Get-FileHash .\Veloryft-Portable-0.2.0-beta.1.zip).Hash.ToLower()
```

Compare with the matching line in `SHA256SUMS`. See
[code signing](code_signing.md).

## Removing it

Delete the folder. To remove settings too:

```powershell
Remove-Item -Recurse "$env:APPDATA\forza-hud-native"
```
