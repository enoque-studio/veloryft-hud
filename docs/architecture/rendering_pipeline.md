# Rendering pipeline

```
Direct2D  ──▶  premultiplied 32-bit DIB  ──▶  UpdateLayeredWindow  ──▶  DWM
```

Each panel is its own top-level window with
`WS_EX_LAYERED | WS_EX_TRANSPARENT`, sized to the card rather than the screen.
Direct2D renders into a device-independent bitmap, and `UpdateLayeredWindow`
hands that buffer to the desktop compositor, which composites it on the GPU with
per-pixel alpha.

Why this and not a transparent fullscreen surface, and why not
DirectComposition, is in
[ADR 0001](../decisions/adr_0001_native_direct2d_overlay.md).

## Two loops

**The main render loop** (`overlay::runtime`) pumps Win32 messages, reads the
latest telemetry frame, and draws every panel. It targets roughly 60 Hz.

**The critical throttle thread** (`overlay::panels::throttle`) owns a separate
surface and runs at 125 Hz on a `THREAD_PRIORITY_HIGHEST` thread.

The split exists because throttle input is the fastest-changing thing on the
HUD and the one a driver reacts to. Sharing the main loop would tie its latency
to whatever else that loop is doing. The priority is *high*, deliberately not
realtime: the work per tick is short and the thread sleeps between frames, so it
never competes with the game for CPU.

The two share exactly one mutex, holding it only long enough to copy a small
`ThrottlePlacement` — the position the bar should occupy, republished by the
main loop whenever the cluster moves.

## Adaptive degradation

The overlay competes with the game for the compositor. When
`UpdateLayeredWindow` starts taking longer, `overlay::frame_pacing` notices and
reduces the cadence of the auxiliary panels only.

| Pressure | Trigger | Cluster | Tires, g-force | Elevation |
| -------- | ------- | ------- | -------------- | --------- |
| Normal | — | every frame | every frame | every frame |
| Elevated | average present ≥ 4 ms | every frame | reduced | reduced |
| Severe | average present ≥ 9 ms | every frame | 10 Hz | 2 Hz |

**The cluster is never rate-limited, at any pressure level.** It carries RPM and
pedal input, which a driver reads continuously. Tire temperatures lagging by a
tenth of a second is invisible; the shift light lagging is not.

The measure is an exponential moving average of presentation time, and pressure
falls back to normal once it recovers. `overlay::frame_pacing` has tests for
each transition, including recovery.

## Idling

When nothing is animating and the HUD has faded out, the loop sleeps for 50 ms
instead of spinning. When something is animating it targets a 16 ms frame. An
idle overlay sitting in a menu costs almost nothing.

Panels also skip presentation when their content has not changed, which is
tracked with a cheap paint key rather than by comparing bitmaps.

## The performance contract

Inside the render loop and the throttle thread:

- **No allocation.** No `Vec`, `String`, `format!`, or collection growth.
- **No locking** beyond the single placement mutex.
- **No logging.** Failures that can recur every frame are latched through
  `diagnostics::Once`: the first is recorded, the rest are silent.
- **No async runtime.** There is none in the dependency tree, and adding one
  here would be a regression.

Resources that are expensive to create — text formats, brushes, the glass
gradient — are created once and reused. The glass gradient is keyed on the theme
values that affect it and rebuilt only when that key changes, because building a
gradient stop collection every frame would show up in exactly the presentation
time the governor watches.

## If you want to change any of this

Bring measurements. What to measure:

- Presentation time, which is what the governor already tracks.
- End-to-end latency from a packet arriving to the pixel changing. The synthetic
  sender gives you a controlled input for this.
- Frame time under load, with the game actually running — an overlay that is
  fast against an idle desktop proves nothing.

A change that improves an average while worsening the worst case is not an
improvement here. A driver notices the stutter, not the mean.

## Edit mode

Edit mode is the only state in which the overlay accepts mouse input.
`overlay::window::set_interactive` clears `WS_EX_TRANSPARENT` — with
`SWP_FRAMECHANGED`, without which the new style is not re-evaluated during hit
testing — and restores it on exit.

`overlay::editing` owns dragging and resizing. Resizing scales from the corner
opposite the grabbed edge and preserves the design aspect ratio, so a card
changes size but never shape. The anchor is captured once when the drag begins;
deriving it on every mouse move would let rounding drift the panel across a long
drag.

Positions snap to a 40-pixel grid, but only within 12 pixels, so placement stays
free elsewhere.

## Design space

Every panel is authored against a fixed design size in device-independent
pixels — the cluster is 680 × 240. The user can resize the window freely;
`overlay::geometry::panel_canvas` maps the design space onto the actual window,
filling it without letterboxing at a uniform scale.
