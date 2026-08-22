# Debugging

## The log

```
%APPDATA%\forza-hud-native\logs\veloryft.log
```

One record per line: a Unix timestamp, a level, and a message.

```
1754131200 INFO veloryft 0.2.0-beta.1 starting
1754131200 INFO vehicle catalog: no external catalog found; showing car ordinals
1754131200 INFO listening for Forza Horizon 6 telemetry on 0.0.0.0:20066
```

Set the level with `VELORYFT_LOG` — `error`, `warn`, `info` (default), or
`debug`:

```powershell
$env:VELORYFT_LOG = "debug"
```

The log rotates to `veloryft.log.1` past 1 MiB, and only one rotation is kept,
so it never costs more than about 2 MiB.

### What is not in it

Telemetry values, machine identifiers, and file contents are never logged. Log
lines describe what failed, not what you were driving. That is a deliberate
constraint on what may be added — see [privacy](../security/privacy.md).

### Errors that repeat every frame

A draw failure recurs on every frame. Those paths are latched with
`diagnostics::Once`: the first occurrence is recorded with
`(repeats suppressed)`, and the rest are silent.

So one such line means the failure may still be happening. Absence of further
lines is not recovery.

## Debug builds keep a console

```bash
cargo run
```

Release builds target the Windows subsystem and have no console, which is why
the log exists. Debug builds keep one, and `diagnostics::record` mirrors every
record to stderr there.

## Startup failures

Anything that prevents startup and that a user can act on is shown in a message
box, because there is no console to print to:

- The UDP port is already in use — names the port and the environment variable.
- Direct2D or DirectWrite is unavailable — points at the graphics driver.
- The platform is unsupported.

Each also goes to the log, with the technical cause.

## Common situations

### No telemetry arrives

The log says which port was bound. If that succeeded and nothing arrives, the
game is not sending: check Data Out, its IP and its port.

To rule out the game entirely:

```bash
cargo run --example synthetic_telemetry
```

If the HUD appears with the synthetic sender, Veloryft is fine and the problem
is the game's configuration.

### The HUD stays hidden with telemetry arriving

Visibility depends on the format and on what the packet contains. Sled packets
never show the HUD, because they carry no speed, gear, or pedal data.

Check the format:

```bash
cargo run --example synthetic_telemetry -- --format sled
```

and confirm the HUD stays hidden — that is correct behaviour, and it is what
your feed is doing.

`--idle` sends `is_race_on = 0` with the engine at idle, which is what a menu
looks like.

### A panel is not where you expect

Positions come from `layout.txt`, and they can be off-screen after a resolution
or monitor change. See
[configuration files](../reference/configuration_files.md).

### Something looks wrong on screen

There is no automated rendering test. Run the overlay with the synthetic sender
and look at it. `--seconds` bounds the run if you are scripting.

## Attaching a debugger

Debug builds work normally under the Visual Studio or VS Code debugger.

Two things to know:

- **Breakpoints in the render loop stall the compositor.** Windows may mark the
  window unresponsive and the DWM may stop compositing it. Prefer logging or a
  conditional breakpoint outside the loop.
- **Breakpoints in a window procedure re-enter.** The procedure is called from
  Windows, and stopping inside it while messages queue can produce confusing
  stacks. Set them on your own functions, not on the procedure.

## Inspecting the loopback API

Only with the `track_mapping` feature:

```bash
cargo run --features track_mapping
```

```bash
curl http://127.0.0.1:9877/live
```

Rejected requests answer with JSON and a status code; the reason is in the body.
See [local HTTP server](../security/local_http_server.md).

## Reporting a problem

Include your version, your Windows version, which game, and the relevant part of
the log. **Read the log before pasting it** — it is written to avoid personal
data, but a file path can still contain your name.

Use the
[bug report template](https://github.com/enoquesousa/veloryft-hud/issues/new?template=bug_report.yml).
