# Configuration files

Everything Veloryft persists lives in one directory:

```
%APPDATA%\forza-hud-native
```

Typically `C:\Users\<you>\AppData\Roaming\forza-hud-native`.

The folder name is `forza-hud-native` for historical reasons — it is what
existing installs already use, and renaming it would orphan everyone's settings.
It has nothing to do with the current binary name.

The installed and portable builds share this directory. Nothing here is
encrypted or obfuscated; it is plain text you can read, edit, back up, or
delete.

## Files

| File | Written when | Contents |
| ---- | ------------ | -------- |
| `layout.txt` | Leaving edit mode, and on exit | Panel positions, sizes, visibility |
| `theme.txt` | A theme change, and on exit | Accent hue, highlight target, glass settings |
| `rev-limits.json` | Periodically while driving, and on exit | Learned shift points and limiters, per car |
| `logs\veloryft.log` | Continuously | Diagnostics |
| `logs\veloryft.log.1` | On rotation | The previous log |
| `worldmap-fh6.json` | Only with `track_mapping` | Recorded world height map |
| `worldmap-fh6.ndjson` | Only with `track_mapping` | Incremental map log |
| `worldpath-fh6.json` | Only with `track_mapping` | Recorded driven path |

## layout.txt

One line per panel:

```
key=x,y,width,height,visible
```

```
cluster=620,840,680,240,1
tires=40,808,256,272,1
gforce=1680,830,200,250,1
elevation=1580,40,300,200,0
```

Coordinates are screen pixels and may be negative on a multi-monitor setup where
a display sits left of or above the primary one. `visible` is `1` or `0`.

Lines that do not parse are skipped rather than discarding the file. A line
without the `visible` field — written by a release from before the visibility
toggles existed — loads as visible.

Delete this file to put every panel back at its default position.

## theme.txt

```
hue=218
target=card
glass_alpha=0.62
glass_tone=-1
```

| Key | Range | Meaning |
| --- | ----- | ------- |
| `hue` | 0–360 | Accent hue in degrees |
| `target` | `gear` or `card` | What the shift highlight tints |
| `glass_alpha` | 0.12–0.95 | Card background opacity |
| `glass_tone` | `-1`, or 0–360 | `-1` is neutral dark; otherwise a tint hue |

Values are clamped when the file is read, so a hand-edited file cannot leave the
overlay in a state the UI cannot undo. Unknown keys are ignored, and a missing
key keeps its default.

## rev-limits.json

Per-car calibration, keyed by the car ordinal the game reports. Each entry holds
the observed limiter, a coarse power curve, and the derived shift point.

The file carries a schema version. A file from an older schema is discarded
rather than migrated, because the data is re-learned within a few laps of
driving — which is cheaper and more accurate than guessing a migration.

Delete it to make Veloryft re-learn every car.

## logs

Plain text, one record per line: a Unix timestamp, a level, and a message.

The active log rotates to `veloryft.log.1` once it passes 1 MiB, and only one
rotation is kept, so the logs never cost more than about 2 MiB.

Telemetry values, machine identifiers, and file contents are never logged. Log
lines describe what failed, not what you were driving. See
[privacy](../security/privacy.md).

## Track-mapping files

Written only by a build with the `track_mapping` feature enabled, which the
released binaries are not. See
[optional features](../architecture/optional_features.md).

## Backing up and resetting

Back up:

```powershell
Copy-Item -Recurse "$env:APPDATA\forza-hud-native" "$env:USERPROFILE\Desktop\veloryft-backup"
```

Reset everything:

```powershell
Remove-Item -Recurse "$env:APPDATA\forza-hud-native"
```

Uninstalling Veloryft does **not** remove this directory, so that reinstalling
keeps your setup.
