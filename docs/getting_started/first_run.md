# First run

## Starting it

Launch Veloryft from the Start menu, or run `Veloryft.exe` from the portable
folder. Nothing appears on screen — that is correct.

Veloryft shows a HUD only while you are driving. Before then, the only sign it
is running is the tray icon.

## The tray icon

Windows files new tray icons under the hidden-icons arrow next to the clock.
Click the arrow to find Veloryft; drag it onto the taskbar to keep it visible.

| Action | Result |
| ------ | ------ |
| Double-click | Show or hide the overlay |
| Right-click | Menu with **Show/Hide** and **Exit** |

## Driving

Start a session and drive. Four panels fade in:

| Panel | Default position | Shows |
| ----- | ---------------- | ----- |
| Cluster | Bottom centre | RPM bar, gear, speed, throttle and brake |
| Tires | Bottom left | Per-corner temperature and the session range |
| G-force | Bottom right | Lateral and longitudinal load |
| Elevation | Top right | The lap's elevation profile and your position on it |

The mouse passes straight through all of them into the game.

## Shift points are learned, not guessed

The white tick on the RPM bar is the recommended upshift point. It is not a
fixed percentage of the redline: Veloryft watches what your car actually does at
full throttle and works out where holding the gear stops paying.

**During your first few laps in a new car the tick may be missing or wrong.**
That is the calibration in progress. Drive the car hard through the gears and it
settles. A red band appears once Veloryft has seen the limiter cut in.

The calibration is stored per car, so coming back to a car keeps what was
learned. Changing the engine resets it, because the rev range changed.

## Arranging the panels

Press **Ctrl+Alt+E** for edit mode:

- A grid appears and the panels become draggable. Drag from the middle.
- Drag an edge or a corner to resize. The aspect ratio is preserved, so a
  resized card looks identical, only bigger.
- Panels snap to the grid when close to a line, and stay exactly where you put
  them otherwise.
- A toolbar appears, with the accent color, the shift-highlight target, a
  visibility toggle per panel, and the **GLASS** and **TONE** sliders.

Press **Ctrl+Alt+E** again to leave. Everything is saved automatically.

While edit mode is on, clicks land on the overlay rather than the game — that is
what makes dragging possible.

## Shortcuts

| Shortcut | Action |
| -------- | ------ |
| **Ctrl + Alt + E** | Edit mode on or off |
| **Ctrl + Alt + H** | Hide or show the overlay |
| **Ctrl + Alt + Q** | Quit |

They are registered as Windows global hotkeys, so they work while the game has
focus. See [keyboard shortcuts](../reference/keyboard_shortcuts.md).

## When the HUD hides itself

By design: in menus, on pause screens, in replays and photo mode, during
cutscenes, and when telemetry stops arriving for about 1.5 seconds.

If you want it visible anyway, edit mode forces it.

## If nothing happens

See [troubleshooting](troubleshooting.md). The first thing to check is Data Out
in the game.
