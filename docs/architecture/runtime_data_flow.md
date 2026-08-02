# Runtime data flow

Which thread owns what, and how they exchange data.

## Threads

| Thread | Created by | Does | Blocks on |
| ------ | ---------- | ---- | --------- |
| Main | The process | Win32 messages, the render loop, panel drawing | Message pump, then a timed sleep |
| `forza-udp` | `telemetry::receiver::spawn` | Receive, decode, publish | `recv_from` |
| `critical-throttle-d2d` | `overlay::panels::throttle` | Draws the throttle bar at 125 Hz | A timed sleep |
| `worldmap-autosave` | Only with `track_mapping` | Periodic map flush | A timed sleep |
| `worldmap-live-api` | Only with `track_mapping` | Serves the loopback HTTP API | Accept |

The default build has three threads.

## Shared state

| What | Type | Written by | Read by |
| ---- | ---- | ---------- | ------- |
| Latest telemetry frame | `ArcSwap<Telemetry>` | Receiver | Main loop, throttle thread |
| Packet counter | `AtomicU64` | Receiver | Throttle thread |
| Throttle placement | `Mutex<ThrottlePlacement>` | Main loop | Throttle thread |
| Edit mode | `AtomicBool` | Main loop | Throttle thread |
| HUD hidden | `AtomicBool` | Main loop, tray | Throttle thread |
| Tray toggle request | `AtomicBool` | Tray menu | Main loop |
| Theme, panel visibility | `Cell`, thread-local | Main loop | Main loop only |

That is the whole list. There is one mutex, and it is held only long enough to
copy a handful of integers.

Theme and visibility are thread-local by design: only the UI thread mutates
them, so making them shared would add synchronization for no reader.

## Telemetry publication

The receive thread builds an owned `Telemetry`, wraps it in an `Arc`, and stores
the pointer:

```
recv_from → packet::parse → model::build → Arc::new → Shared::store
```

Readers take the current pointer. A reader that is mid-frame keeps the frame it
took while the producer moves on. Frames are **dropped, not queued** — for a
HUD, a stale frame is worthless and the newest one is always what should be on
screen.

The packet counter is how the throttle thread distinguishes a new frame from a
repeat without comparing field values.

## The tray, and why it uses an atomic

The tray menu runs inside a nested Win32 message loop. Acting on a menu command
directly from there would re-enter the renderer while the outer loop is
suspended.

So the menu sets `TRAY_TOGGLE_HIDE`, and the main loop drains it on its next
pass. One atomic, and the reentrancy problem disappears.

## Shutdown

Quitting — through `Ctrl+Alt+Q`, the tray menu, or `WM_QUIT` — runs the same
sequence on the main loop:

1. Save the panel layout.
2. Save the theme.
3. Persist the rev-limit calibration.
4. If track mapping is on, compact the world map.
5. Remove the tray icon.
6. Unregister the hotkeys.
7. Return, and let the process exit.

The receive thread and the throttle thread are not joined. They hold no
unflushed state — everything durable is written by the main loop above — and the
process is exiting, so waiting for them would only add a shutdown delay.

## Error paths

| Failure | Behaviour |
| ------- | --------- |
| Port already in use | Message box naming the port; the overlay runs and stays hidden |
| Direct2D or DirectWrite unavailable | Message box; the process exits with code 3 |
| Tray icon fails | Logged; the overlay runs without a tray icon |
| A hotkey is already taken | That shortcut does nothing; the others still work |
| Panel draw fails | Logged once, then suppressed; the loop continues |
| Socket read fails | Logged once, then suppressed |
| Settings cannot be saved | Logged once, then suppressed |

Nothing on a per-frame path logs more than once, because a failure that recurs
every frame would otherwise fill the disk faster than it explains anything. See
[debugging](../development/debugging.md).
