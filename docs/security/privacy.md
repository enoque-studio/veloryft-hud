# Privacy

## The short version

**Veloryft sends nothing anywhere.** Everything it reads stays on your machine.

There is no account, no analytics, no crash reporting, no update check, and no
outbound network code in the released binary.

## What it reads

| Source | What | Where it goes |
| ------ | ---- | ------------- |
| The game, over UDP | Engine speed, gear, speed, pedals, tire temperatures, position, car ordinal | Memory, and the screen |
| Your settings files | Panel layout, theme | Memory |
| Environment variables | Port and log-level overrides | Memory |

## What it writes

All of it in `%APPDATA%\forza-hud-native`:

| File | Contains |
| ---- | -------- |
| `layout.txt` | Panel positions and sizes |
| `theme.txt` | Colours and opacity |
| `rev-limits.json` | Shift points learned per car ordinal |
| `logs\veloryft.log` | Diagnostics |

Nothing leaves that folder. Nothing is uploaded.

## How the claim is verifiable

This is not a promise you have to take on trust.

- The source is public. `grep` for a socket: the only one in a default build is
  the UDP receiver, and it only ever receives.
- The dependency graph has no HTTP client and no TLS stack. `cargo tree -e normal`
  shows twenty-four crates, most of them the Windows bindings.
- `deny.toml` refuses dependencies from anywhere but crates.io, so nothing can
  arrive from an unaudited source.
- You can watch it with a firewall or Wireshark. You will see UDP arriving on
  20066 and nothing leaving.

## The log

The log exists because release builds have no console, and a user whose HUD does
not appear needs something to look at.

It records **what failed, not what you were driving**. Excluded by design:

- Telemetry values — no speed, no position, no car.
- Machine identifiers — no hostname, no hardware fingerprint, no MAC address.
- File contents.
- Anything resembling a credential.

What it does contain: timestamps, levels, which port was bound, how many cars
loaded, and error text from Windows when something failed.

Windows error text sometimes includes a file path, and a Windows profile path
contains your user name. **Read the log before pasting it into an issue.** The
issue template says so too.

Two files, 1 MiB each at most. Delete them any time.

## The UDP port

Veloryft binds `0.0.0.0:20066`, meaning it accepts telemetry from your own
machine and from anything on your network that can reach that port. That is
deliberate — some people run the game on one PC and the HUD on another.

Nothing is sent in response. The socket only receives.

If you would rather it were not reachable from the network, block the port
inbound in Windows Firewall. The game sends to `127.0.0.1`, which a firewall
rule for external traffic does not affect.

## Track mapping

The optional `track_mapping` feature, which the released binaries **do not**
include, records world positions and serves them over a loopback HTTP API for a
local 3D viewer.

Even then, the data stays local: the recorder writes to your profile directory
and the API binds `127.0.0.1`. There is no upload path in this repository.

If a future version adds one, it will be opt-in, it will say what it sends, and
this page will change first.

## Uninstalling

Removing Veloryft leaves your settings behind on purpose, so reinstalling keeps
your setup. To remove everything:

```powershell
Remove-Item -Recurse "$env:APPDATA\forza-hud-native"
```

## Distribution terms

The source and official binaries are distributed under Apache-2.0. The
installer ships `LICENSE`, `NOTICE`, and `THIRD_PARTY_NOTICES.md` beside the
executable; there is no separate click-through EULA.

## GDPR, LGPD, and similar

No personal data is collected, processed remotely, or transferred to anyone.
There is no controller and no processor because there is no processing outside
your own machine.

Data protection law is largely inapplicable to a program that keeps everything
locally, which is a good reason to build one that way.
