# ADR 0001 — Native Direct2D overlay instead of WebView2

**Status:** Accepted
**Date:** 2026-06

## Context

Veloryft draws a HUD on top of a running game. Two things are non-negotiable:

1. **The mouse must reach the game.** A driver's clicks belong to the game, not
   to the overlay. Cross-process click-through is a hard requirement.
2. **It must not judder.** A HUD that stutters is worse than no HUD, because the
   eye is drawn to motion that does not match the motion on screen.

The project began as a Tauri application, with the HUD in a transparent
fullscreen WebView2 window. Web technology for the UI would have been
convenient: layout, text, and animation come for free.

## The problem with WebView2

A transparent fullscreen WebView2 window goes through the **redirection
bitmap**. The desktop compositor blends the alpha layer every frame, over the
whole screen, whether or not anything changed.

The result judders visibly during play. It is not a rendering bug in the page
and it is not fixable from inside: `requestAnimationFrame` tuning, `will-change`
hints, compositing layers, and reducing paint area all fail, because the cost is
in how the window itself reaches the compositor, not in how the page paints.

Cross-process click-through was the second problem. `HTTRANSPARENT` from a hit
test does not cross a process boundary, so clicks landed on the overlay rather
than the game.

## Decision

Give **each panel its own small `WS_EX_LAYERED | WS_EX_TRANSPARENT` window**,
sized to the card rather than the screen. Render with Direct2D into a
premultiplied 32-bit DIB and hand that buffer to the compositor with
`UpdateLayeredWindow`.

- `WS_EX_LAYERED` with `UpdateLayeredWindow` gives per-pixel alpha composited on
  the GPU, with no redirection bitmap.
- `WS_EX_TRANSPARENT` is what actually makes clicks pass to the game across the
  process boundary.
- Small windows mean the compositor blends a few hundred thousand pixels, not
  the whole screen. Most of the screen has no overlay window over it at all.
- Direct2D keeps the visual style — gradients, rounded corners, anti-aliased
  text — that the web version had.

## Why not DirectComposition

DirectComposition composites smoothly and is the more modern API. It was
rejected because it is **incompatible with what makes this work**:
`WS_EX_LAYERED` is required for cross-process click-through, and a layered
window's per-pixel alpha does not coexist with DComp's own compositing model.

Dropping `WS_EX_LAYERED` to use DComp would mean giving up click-through, which
is the requirement that started this.

If a future Windows API offers both, this decision is worth revisiting.

## Consequences

**Good.** No judder. Real cross-process click-through. GPU compositing with
per-pixel alpha. Small binary, few dependencies, no browser runtime. Panels can
be repositioned independently because each is its own window.

**Bad.** The overlay is Windows-only, permanently. There is no cross-platform
path from here, and pretending otherwise would waste contributors' time.

Layout is code. There is no stylesheet: changing where a number sits means
editing Rust and rebuilding. That is the cost of the pixels being ours.

`src/overlay/` contains `unsafe`, because Win32 and Direct2D are unsafe APIs. It
is confined there — `telemetry` has none — but it is real, and it is why the
[coding standards](../development/coding_standards.md) say what they say about
`unsafe`.

Text rendering needed care. ClearType does not work over per-pixel alpha; it
fringes and smears on a layered window. Grayscale anti-aliasing gives a clean
outline against any background and is what the panels use.

## Revisit if

- Windows gains an API providing per-pixel alpha *and* cross-process
  click-through without `WS_EX_LAYERED`.
- WebView2 gains a composition path that avoids the redirection bitmap for
  transparent windows.
- The overlay grows enough UI that hand-written layout becomes the bottleneck —
  though the answer then is probably a separate configuration window, not a
  change to how the HUD draws.

## See also

- [Rendering pipeline](../architecture/rendering_pipeline.md)
- `src/overlay/window.rs`
