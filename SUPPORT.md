# Getting help

Veloryft is a small open-beta project. Everything happens on GitHub.

## Before asking

Most reports turn out to be one of a handful of things. The
[troubleshooting guide](docs/getting_started/troubleshooting.md) covers them:

- The HUD never appears → Data Out is off, or set to the wrong IP or port.
- The HUD appears and then vanishes → that is deliberate; it hides in menus,
  pauses and replays.
- Nothing at all happens → check the log at
  `%APPDATA%\forza-hud-native\logs\veloryft.log`. Startup failures are also
  shown in a message box.
- Windows blocked the download → releases are unsigned; see
  [installation](docs/getting_started/installation.md).

## Where to go

| I want to… | Go to |
| ---------- | ----- |
| Report a bug | [Bug report](https://github.com/ESousa97/veloryft-hud/issues/new?template=bug_report.yml) |
| Suggest a feature | [Feature request](https://github.com/ESousa97/veloryft-hud/issues/new?template=feature_request.yml) |
| Ask a question | [Issues](https://github.com/ESousa97/veloryft-hud/issues) |
| Report a vulnerability | [SECURITY.md](SECURITY.md) — **not** a public issue |
| Contribute code | [CONTRIBUTING.md](CONTRIBUTING.md) |

## What helps a bug report

- The version, from the installer file name or the executable's Properties.
- Your Windows version and whether you run more than one monitor.
- Which Forza Horizon 6 build, and the Data Out settings you used.
- What you expected and what happened.
- The relevant part of `%APPDATA%\forza-hud-native\logs\veloryft.log`.

Read the log before pasting it. It is written to avoid personal data, but check
anyway — a file path can contain your name.

## What is not supported

- Games other than Forza Horizon 6. Other titles that
  speak the same UDP format may work; nothing is promised.
- Windows versions before Windows 10 64-bit.
- Builds you compiled yourself with modifications. Reproduce on an official
  release first.

## Response times

One person maintains this in their own time. There is no service commitment.
Clear, reproducible reports get answered first, because they can be acted on.
