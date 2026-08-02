# Installing Veloryft

Veloryft is a Windows application. You do not need Rust, Visual Studio, or any
build tools to use it.

## Requirements

- 64-bit Windows 10 or later
- A copy of a supported Forza title
- Nothing else. The binary is built with a static C runtime, so there is no
  Visual C++ Redistributable to install.

## Choose a package

Both contain the same executable.

| | Installer | Portable |
| --- | --- | --- |
| File | `Veloryft-Setup-<version>.exe` | `Veloryft-Portable-<version>.zip` |
| Adds a Start menu entry | Yes | No |
| Adds an uninstaller | Yes | No |
| Needs administrator rights | No | No |
| Runs from a USB stick | No | Yes |

Download either from the
[Releases page](https://github.com/ESousa97/veloryft-hud/releases).

## Verify what you downloaded

Every release publishes a `SHA256SUMS` file. Releases are **not code-signed**,
so this is how you confirm the file is the one that was built.

```powershell
(Get-FileHash .\Veloryft-Setup-0.2.0-beta.1.exe).Hash.ToLower()
```

Compare the output with the matching line in `SHA256SUMS`. If it differs, do not
run the file.

## Windows will warn you

Because the downloads are unsigned, SmartScreen shows **"Windows protected your
PC"**, and Edge or Chrome may call the download unsafe. That is what an unsigned
binary from a small publisher looks like; it is not a statement about the file's
contents.

If you have verified the checksum and want to continue:

1. Click **More info**.
2. Click **Run anyway**.

See [code signing](../distribution/code_signing.md) for why the artifacts are
unsigned and what would change if that were fixed.

## Installer

Run the `.exe` and follow the wizard. It installs per-user by default, so no
administrator prompt appears. You can choose an all-users install in the
privileges dialog if you prefer.

The installer never asks for a key, account, or separate license acceptance.
The open beta is distributed under Apache-2.0 and has no license check.

## Portable

Unzip anywhere and run `Veloryft.exe`. No configuration files need to sit beside
it. An optional user-supplied vehicle catalog can add car names; without one,
the HUD displays the car ordinal. See
[vehicle catalog](../reference/vehicle_catalog.md).

Settings still go to `%APPDATA%\forza-hud-native`, the same place the installed
version uses — a portable copy is not isolated from an installed one.

## Where your files go

| What | Where |
| ---- | ----- |
| Program (installer) | `%LOCALAPPDATA%\Programs\Veloryft` or `%ProgramFiles%\Veloryft` |
| Settings, layout, theme | `%APPDATA%\forza-hud-native` |
| Learned rev limits | `%APPDATA%\forza-hud-native\rev-limits.json` |
| Logs | `%APPDATA%\forza-hud-native\logs` |

The full list is in [configuration files](../reference/configuration_files.md).

## Uninstalling

**Installer:** Settings → Apps → Installed apps → Veloryft → Uninstall, or use
the Start menu shortcut.

**Portable:** delete the folder.

Either way, **your settings are left behind on purpose**, so that reinstalling
or upgrading keeps your layout, your theme, and the shift points Veloryft
learned for your cars. To remove those too:

```powershell
Remove-Item -Recurse "$env:APPDATA\forza-hud-native"
```

## Next

[Configure the game](game_configuration.md) so that it sends telemetry.
