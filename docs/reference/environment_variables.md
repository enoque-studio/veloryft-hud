# Environment variables

Every variable Veloryft reads is listed here. All are optional; the defaults are
what the application uses when they are unset.

## Always available

| Variable | Default | Effect |
| -------- | ------- | ------ |
| `FORZA_UDP_PORT` | `20066` | UDP port the telemetry receiver binds. Must match the game's Data Out port. |
| `FORZA_CAR_ORDINALS` | unset | Path to a car-name catalog that overrides the copy embedded in the binary. |
| `VELORYFT_LOG` | `info` | Minimum log level written to the log file: `error`, `warn`, `info`, or `debug`. |

`APPDATA` is also read, to locate the configuration directory. It is set by
Windows; if it is somehow missing, Veloryft falls back to a `forza-hud-native`
folder in the working directory.

## Only with the `track_mapping` feature

These have no effect on a default build, because the code that reads them is not
compiled into it. See
[optional features](../architecture/optional_features.md).

| Variable | Default | Effect |
| -------- | ------- | ------ |
| `VELORYFT_LIVE_PORT` | `9877` | TCP port for the loopback viewer API. Bound on `127.0.0.1` only. |
| `VELORYFT_WORLDMAP_CELL_M` | see the recorder | Grid cell size in metres for the world-height map. |

## Setting them

For one run, from PowerShell:

```powershell
$env:FORZA_UDP_PORT = "20077"
```

Then start Veloryft from that same terminal. A variable set this way is gone
when the terminal closes.

To make it permanent for your account:

```powershell
[Environment]::SetEnvironmentVariable('FORZA_UDP_PORT', '20077', 'User')
```

Log out and back in, or restart Explorer, for programs started from the Start
menu to see it.

To remove it again:

```powershell
[Environment]::SetEnvironmentVariable('FORZA_UDP_PORT', $null, 'User')
```

## Stability

These names are a public contract. They will not be renamed without a migration
path and a note in the [changelog](../../CHANGELOG.md).

Variables the pre-0.2 licensing code once read — `VELORYFT_LICENSE_ORIGIN`,
`VELORYFT_LICENSE_APP_HMAC_SECRET`, `VELORYFT_LICENSE_RESPONSE_PUBLIC_KEYS`,
`VELORYFT_LICENSE_SPKI_PINS` — are gone. They were build-time inputs, never read
at runtime, and the subsystem that used them is no longer part of this
repository. Setting them now has no effect. Any older document that describes
them as required for a release build is wrong.
