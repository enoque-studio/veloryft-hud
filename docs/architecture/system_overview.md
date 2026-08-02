# System overview

Veloryft is one Windows executable. There is no service, no browser, no local
web server in the default build, and no background updater.

```
Forza  ──UDP 20066──▶  telemetry::receiver     dedicated thread
                              │
                              ▼
                       telemetry::packet       decode by offset, zero copy
                              │
                              ▼
                       telemetry::model        the domain frame
                              │
                              ▼
                       telemetry::snapshot     ArcSwap: lock-free publish
                              │
                    ┌─────────┴─────────┐
                    ▼                   ▼
        overlay::runtime          panels::throttle
        main render loop          dedicated thread, 125 Hz
                    │                   │
                    ▼                   ▼
          Direct2D ─▶ DIB ─▶ UpdateLayeredWindow ─▶ DWM
```

## The shape of it

**One producer, two consumers.** The receive thread parses packets and publishes
an immutable frame. The render loop and the throttle thread each read the latest
frame whenever they are ready. Neither blocks the other and neither blocks the
receiver.

**Nothing is pushed.** Consumers pull the current frame at their own cadence.
That is what lets the throttle bar run at 125 Hz while the elevation panel
redraws a few times a second, without any coordination between them.

**Publication is a pointer swap.** `telemetry::snapshot` wraps `ArcSwap`.
Storing a frame is one atomic store; reading one is one atomic load and a
refcount bump. There is no lock on the path from a packet arriving to a pixel
changing.

## Layers

| Layer | Module | Knows about |
| ----- | ------ | ----------- |
| Startup | `application` | The other layers, and nothing about how they work |
| Telemetry | `telemetry` | Bytes and physics. **No Win32, no Direct2D.** |
| Presentation | `overlay` | Windows and pixels. **Does not parse packets.** |
| Optional | `track_mapping` | Telemetry and its own persistence |
| Contract | `veloryft-protocol` | Only serialization. No I/O, no `unsafe`. |

The rule that keeps this honest: `telemetry` compiles and its tests run on any
platform. If a Win32 type ever leaks into it, that stops being true, and CI
notices.

## Why the overlay is native

A transparent fullscreen WebView2 window goes through the redirection bitmap,
and the compositor blends the alpha layer every frame. The result judders, and
no amount of CSS or `requestAnimationFrame` tuning fixes it.

Veloryft instead gives each panel a **small** `WS_EX_LAYERED | WS_EX_TRANSPARENT`
window — the size of the card, not the screen. Direct2D renders into a
premultiplied DIB, and `UpdateLayeredWindow` hands that buffer to the DWM, which
composites it on the GPU with per-pixel alpha.

`WS_EX_TRANSPARENT` is also what makes clicks reach the game: hit-test tricks
like `HTTRANSPARENT` do not cross a process boundary.

The full reasoning, including why DirectComposition was not used, is in
[ADR 0001](../decisions/adr_0001_native_direct2d_overlay.md).

## Frame pacing

The overlay competes with the game for the compositor. When presentation starts
taking longer, slowing everything down would be the wrong answer: the cluster
carries RPM and pedal input, which a driver reads continuously, while tire
temperatures can lag without anyone noticing.

`overlay::frame_pacing` measures how long presentation takes and throttles only
the auxiliary panels. **The cluster is never rate-limited.**

The critical throttle bar is not governed at all — it owns a separate surface
and a `THREAD_PRIORITY_HIGHEST` thread at 125 Hz, so its latency does not depend
on what the main loop is doing. See
[rendering pipeline](rendering_pipeline.md).

## Startup

1. `application::run` opens the log and asks `StartupAuthorization` whether it
   may proceed. The public build's implementation always says yes — the open
   beta has no license check.
2. `overlay::run` registers window classes, restores the saved layout and theme,
   and creates the panels.
3. The throttle thread starts.
4. The telemetry receiver binds its port. If the port is taken, the user is told
   which port and what to do; the overlay keeps running and stays hidden.
5. The tray icon and the global hotkeys are registered.
6. The message and render loop runs until the user quits.

Failures that prevent startup surface in a message box, because release builds
target the Windows subsystem and have no console. Everything else goes to the
log. See [debugging](../development/debugging.md).

## What it does not do

- Read or write the game's memory, or inject code into it. It reads only what
  the game broadcasts through its own Data Out feature.
- Send anything over the network. The default build has no outbound network code
  and opens no listening socket. See [privacy](../security/privacy.md).
- Require an account, a key, or a connection.

## Further reading

- [Repository structure](repository_structure.md)
- [Telemetry pipeline](telemetry_pipeline.md)
- [Rendering pipeline](rendering_pipeline.md)
- [Runtime data flow](runtime_data_flow.md)
- [Optional features](optional_features.md)
- [Technology stack](technology_stack.md)
