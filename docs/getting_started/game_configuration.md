# Configuring the game

Veloryft does not read the game's memory and does not attach to its process. It
listens for the telemetry the game broadcasts through its own "Data Out"
feature, which you have to switch on.

## Forza Horizon 6

1. Open **Settings**.
2. Go to **HUD and Gameplay**.
3. Scroll to the bottom, to **Data Out**.
4. Set:

   | Setting | Value |
   | ------- | ----- |
   | Data Out | **On** |
   | Data Out IP Address | `127.0.0.1` |
   | Data Out IP Port | `20066` |

Some titles label this section **Telemetry Output** instead. The three settings
are the same.

## Forza Motorsport

The same three settings live under **Settings → Gameplay & HUD**, as **UDP Race
Telemetry** or **Data Out** depending on the title. Use the same values.

## Why 127.0.0.1

`127.0.0.1` is your own computer. Telemetry never leaves the machine: the game
sends it to a socket Veloryft opens locally, and Veloryft sends it nowhere. See
[privacy](../security/privacy.md).

If you have previously pointed Data Out at another program or another PC, note
that Forza Horizon 6 sends to **one** address. Changing it here stops the other program
from receiving anything.

## Why 20066

That is the port Veloryft listens on by default. There is nothing special about
it; the two sides simply have to agree.

If something else on your machine already uses 20066, pick another port, set the
same number in the game, and tell Veloryft with the `FORZA_UDP_PORT`
environment variable:

```powershell
$env:FORZA_UDP_PORT = "20077"
```

See [environment variables](../reference/environment_variables.md) for how to
make that permanent.

## Packet formats

Forza Horizon 6 sends one of three layouts, depending on the setting.
Veloryft detects which one arrived and adapts:

| Format | Size | What it carries |
| ------ | ---- | --------------- |
| Sled | 232 bytes | Engine and physics only — no speed, gear, or pedals |
| Dash | 311 bytes | Everything the HUD shows |
| Horizon | 323 bytes | Dash plus the car group |

**The HUD stays hidden on Sled packets**, because there is nothing to draw. If
your HUD never appears and the log reports `Sled`, switch the game to the dash
or full telemetry option.

Details are in [telemetry formats](../reference/telemetry_formats.md).

## Checking it works

Start the game and begin driving. The HUD appears within a second.

It stays hidden in menus, on pause screens, during replays, and in cutscenes —
that is deliberate. Press **Ctrl+Alt+E** at any time to force everything visible
and confirm Veloryft is running.

If nothing appears, see [troubleshooting](troubleshooting.md).
