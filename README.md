# Veloryft

A native telemetry HUD for Forza on Windows. Written in Rust, drawn with
Direct2D, and you can click straight through it into the game.

> **Open beta.** It works and people use it, but expect rough edges. There is no
> license check, no account, and no key — download it and run it.

## What it does

- **RPM, gear, speed and pedals** in a cluster you can put where you like.
- **A shift point learned from your car**, not guessed from a percentage of the
  redline. Veloryft watches what the car does at full throttle and works out
  where holding the gear stops paying.
- **Tire temperatures** per corner, with the range covered this session.
- **G-force** — lateral and longitudinal load, with a decaying trail.
- **The lap's elevation profile**, with a marker showing where you are on it.
- **Real click-through.** The mouse reaches the game, not the overlay.
- **It hides itself** in menus, on pause screens, in replays and during
  cutscenes, and comes back when you start driving.

## Privacy

**Nothing leaves your machine.** No account, no analytics, no update check, no
outbound network code at all. Veloryft reads the telemetry the game broadcasts
to `127.0.0.1` and writes settings to your own profile folder.

You do not have to take that on trust: the source is public, the dependency
graph contains no HTTP client and no TLS stack, and a firewall will show you
nothing leaving. See [privacy](docs/security/privacy.md).

## Requirements

- 64-bit Windows 10 or later
- A Forza title with "Data Out" telemetry
- Nothing else — no Visual C++ Redistributable, no runtime

---

## For users

### 1. Download

From [Releases](https://github.com/ESousa97/veloryft-hud/releases):

| | |
| --- | --- |
| `Veloryft-Setup-<version>.exe` | Installer, with a Start menu entry and an uninstaller |
| `Veloryft-Portable-<version>.zip` | Unzip and run; nothing is installed |

### 2. Verify it

Releases are **not code-signed**, so Windows SmartScreen will warn you. Check
the file against the published `SHA256SUMS` before running it:

```powershell
(Get-FileHash .\Veloryft-Setup-0.2.0-beta.1.exe).Hash.ToLower()
```

If it matches, click **More info** → **Run anyway**. If it does not, do not run
it. See [installation](docs/getting_started/installation.md).

### 3. Turn on telemetry in the game

**Settings → HUD and Gameplay → Data Out**

| Setting | Value |
| ------- | ----- |
| Data Out | On |
| IP address | `127.0.0.1` |
| Port | `20066` |

Some titles call this "Telemetry Output". See
[game configuration](docs/getting_started/game_configuration.md).

### 4. Drive

The HUD appears within a second of moving. Nothing shows up in menus — that is
deliberate.

| Shortcut | Action |
| -------- | ------ |
| **Ctrl + Alt + E** | Edit mode: drag, resize, change colours |
| **Ctrl + Alt + H** | Hide or show |
| **Ctrl + Alt + Q** | Quit |

Veloryft also sits in the system tray, usually under the hidden-icons arrow.

### Uninstalling

Settings → Apps → Veloryft → Uninstall, or delete the portable folder.

**Your settings are kept**, so reinstalling restores your layout and everything
Veloryft learned about your cars. To remove them too:

```powershell
Remove-Item -Recurse "$env:APPDATA\forza-hud-native"
```

### Something is wrong

[Troubleshooting](docs/getting_started/troubleshooting.md) covers the common
cases — the HUD not appearing (usually Data Out), panels off-screen after a
resolution change, shortcuts taken by another program.

Logs are at `%APPDATA%\forza-hud-native\logs\veloryft.log`.

---

## For contributors

### Build it

Clone this repository and nothing else. There are no private dependencies and no
tokens.

```bash
git clone https://github.com/ESousa97/veloryft-hud
cd veloryft-hud
cargo build --release
```

You need 64-bit Windows, Rust with the MSVC toolchain, and the Visual Studio
Build Tools C++ workload.

### Work on it without owning Forza

```bash
cargo run
```

```bash
cargo run --example synthetic_telemetry
```

The HUD appears within a second, showing a car working through the gears around
a closed loop. `--format sled|dash|fh6` exercises each wire format; `--idle`
checks that the HUD correctly hides itself.

### Before you push

```bash
cargo fmt --all -- --check
```

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

```bash
cargo test --workspace --all-features
```

That is exactly what CI runs. Start with
[CONTRIBUTING.md](CONTRIBUTING.md) — particularly the rules for the render hot
path, which is not ordinary code.

---

## How it works

```
Forza  ──UDP──▶  parse  ──▶  lock-free snapshot  ──▶  Direct2D  ──▶  layered window
```

Each panel is its own small `WS_EX_LAYERED | WS_EX_TRANSPARENT` window. Direct2D
renders into a premultiplied DIB, and `UpdateLayeredWindow` hands it to the
compositor — per-pixel alpha on the GPU, and clicks that cross the process
boundary into the game. A fullscreen transparent surface judders; this does not.
See [ADR 0001](docs/decisions/adr_0001_native_direct2d_overlay.md).

The throttle bar has its own surface and its own high-priority thread at 125 Hz,
because it is the fastest-changing thing on screen and the one a driver reacts
to. Under compositor pressure the auxiliary panels slow down; the cluster never
does.

```
veloryft-hud/
├── src/
│   ├── application/     startup and the authorization boundary
│   ├── telemetry/       UDP, decoding, the domain model, derived state
│   ├── overlay/         Win32 and Direct2D presentation
│   └── track_mapping/   optional, off by default
├── crates/veloryft_protocol/   contract shared with the backend
├── tests/  examples/  data/  assets/  installer/  scripts/  docs/
```

More: [system overview](docs/architecture/system_overview.md) ·
[repository structure](docs/architecture/repository_structure.md)

## Documentation

Everything is in [`docs/`](docs/README.md) — user guides, architecture,
development, distribution, security, and the decision records.

## Contributing

Issues and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers
prerequisites, what CI checks, and the constraints on hot-path and `unsafe` code.

Need help? [SUPPORT.md](SUPPORT.md).

## Security

Report vulnerabilities **privately**, never in a public issue. See
[SECURITY.md](SECURITY.md).

## License

[Apache-2.0](LICENSE). Third-party components are listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

The **Veloryft** name and logo are trademarks and are not covered by that
license — fork freely, but rename your build. See
[TRADEMARKS.md](TRADEMARKS.md).

## Not affiliated with anyone

"Forza", "Forza Horizon", "Forza Motorsport", "Xbox" and "Microsoft" are
trademarks of Microsoft Corporation. "Turn 10 Studios" and "Playground Games"
are names of their respective owners.

**Veloryft is not affiliated with, sponsored by, or endorsed by any of them.**
It reads only the telemetry the game itself broadcasts when you enable it. It
does not modify the game, read its memory, or inject code.
