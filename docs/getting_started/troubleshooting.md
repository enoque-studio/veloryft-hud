# Troubleshooting

Work down this page in order. The first two sections cover most reports.

## The HUD never appears

**1. Is Data Out on?**

In the game: Settings → HUD and Gameplay → Data Out. It must be **On**, IP
`127.0.0.1`, port `20066`. See [game configuration](game_configuration.md).

**2. Is Veloryft actually running?**

Look for the tray icon under the hidden-icons arrow. Press **Ctrl+Alt+E**: edit
mode forces every panel visible regardless of telemetry, so if panels appear,
Veloryft is fine and the problem is the telemetry feed.

**3. Is something else using the port?**

```powershell
Get-NetUDPEndpoint -LocalPort 20066 -ErrorAction SilentlyContinue | ForEach-Object { Get-Process -Id $_.OwningProcess }
```

If another program answers, either close it or move Veloryft to a different port
with `FORZA_UDP_PORT` and set the same port in the game.

Veloryft shows a message box at startup when the port is already taken.

**4. Is the game sending Sled packets?**

Sled is the smallest telemetry format and carries no speed, gear, or pedal data,
so there is nothing to draw and the HUD stays hidden. Check the log:

```
%APPDATA%\forza-hud-native\logs\veloryft.log
```

Switch the game to the dash or full telemetry option.

**5. Is another overlay covering it?**

Some capture and chat overlays draw above everything. Disable them and retry.

## The HUD appears and then vanishes

That is normal. It hides in menus, on pause screens, in replays and photo mode,
during cutscenes, and when telemetry stops for about 1.5 seconds.

If it disappears **while you are driving**, telemetry is being interrupted.
Check whether another program is also bound to the port.

## Panels are off-screen

This happens after changing resolution or unplugging a monitor: the saved
position is now outside the visible area.

Press **Ctrl+Alt+E** and drag them back. If they cannot be reached at all,
delete the layout and restart Veloryft:

```powershell
Remove-Item "$env:APPDATA\forza-hud-native\layout.txt"
```

Only the positions are lost. The theme and the learned rev limits are separate
files.

## The shift point is wrong or missing

It is learned per car, from what the car does at full throttle, so a new car has
no data yet. Drive it hard through the gears for a few laps.

To start over for every car:

```powershell
Remove-Item "$env:APPDATA\forza-hud-native\rev-limits.json"
```

## Shortcuts do nothing

Windows global hotkeys are first-come, first-served. Another application that
registered **Ctrl+Alt+E**, **H**, or **Q** first keeps them.

Close likely candidates — capture tools, launcher overlays, macro software — and
restart Veloryft.

## Nothing happens at all when I run it

Check the log:

```
%APPDATA%\forza-hud-native\logs\veloryft.log
```

Startup failures you can act on are also shown in a message box. For more
detail, raise the log level and run from a terminal:

```powershell
$env:VELORYFT_LOG = "debug"
```

Common causes:

- **Direct2D or DirectWrite unavailable.** Update your graphics driver. A remote
  desktop session, or a virtual machine without GPU acceleration, can also fail
  here.
- **Not 64-bit Windows 10 or later.** Nothing older is supported.

## Windows blocked the download

The releases are unsigned. See [installation](installation.md) for how to verify
the checksum and proceed.

## Starting completely fresh

```powershell
Remove-Item -Recurse "$env:APPDATA\forza-hud-native"
```

This removes the layout, the theme, the learned rev limits, and the logs. The
program itself is untouched.

## Still stuck

Open a [bug report](https://github.com/ESousa97/veloryft-hud/issues/new?template=bug_report.yml).
Include your Veloryft version, your Windows version, which game, and the
relevant part of the log — after reading it, since a file path can contain your
name.
