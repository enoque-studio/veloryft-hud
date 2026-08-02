# Telemetry formats

Forza titles broadcast a fixed-layout binary datagram over UDP when "Data Out"
is enabled. There is no header, no version field, and no framing: the **length**
of the datagram is the only thing that identifies the layout.

All values are little-endian.

## The three layouts

| Format | Size | Blocks |
| ------ | ---- | ------ |
| Sled | 232 bytes | BASE |
| Dash | 311 bytes | BASE + DASH |
| Horizon | 323 bytes | BASE + 12 bytes + DASH |

`BASE` is the physics block: engine speeds, accelerations, per-wheel signals,
and the car ordinal. `DASH` adds what a dashboard needs: position, speed, gear,
pedals, tire temperatures, fuel.

The Horizon layout inserts twelve bytes between the two — a `u32` car group and
two reserved floats — which shifts the whole `DASH` block. That is the only
structural difference, and getting it wrong yields plausible-looking nonsense
rather than an error, which is why format detection matters.

## How Veloryft decides

A 323-byte datagram could be either a Horizon packet or a Dash packet with
trailing bytes. `telemetry::packet` decodes it both ways and scores each
interpretation on whether the fields land in physically sensible ranges:

- speed between 0 and 180 m/s
- fuel between 0 and 1
- gear at most 12
- tire temperatures between −40 and 500
- position within a million units of the origin

The higher score wins. In practice a misaligned read fails several of these at
once, so the decision is not marginal.

Datagrams shorter than 232 bytes are dropped. Longer ones are accepted and the
tail ignored, because reads are by offset.

## What the HUD needs

| Panel | Needs |
| ----- | ----- |
| Cluster | `DASH` — speed, gear, pedals |
| Tires | `DASH` — tire temperatures |
| G-force | `BASE` — accelerations |
| Elevation | `DASH` — position |

Sled packets carry only `BASE`, so **the HUD stays hidden on a Sled feed**.
There is nothing to draw. If that is what you are getting, change the telemetry
option in the game.

## Units on the wire

| Field | Unit as sent | Shown as |
| ----- | ------------ | -------- |
| Engine speed | RPM | RPM |
| Speed | metres per second | km/h |
| Acceleration | metres per second squared | g |
| Power | watts | metric horsepower (cv) |
| Position | metres | metres |
| Throttle, brake, clutch | 0–255 | 0–100 % |
| Gear | 0 is reverse, 1 is first | as sent |
| Steering | −127 to 127 | −100 to 100 |
| Fuel | 0.0 to 1.0 | — |
| Tire temperature | Fahrenheit | see the note below |

> **Known issue.** The tires panel labels its readout `°C` while showing the raw
> value the game sends, which is Fahrenheit. The colour bands (blue below 150,
> green to 205, amber to 245, red above) are Fahrenheit thresholds and are
> correct; only the unit label is wrong. This is tracked as a bug and left
> unchanged here because fixing it alters what the HUD displays.

## Where this lives in the code

| Concern | Module |
| ------- | ------ |
| Offsets, format detection, zero-copy accessors | `telemetry::packet` |
| The domain frame the rest of the app uses | `telemetry::model` |
| Building datagrams, for tests and the example | `telemetry::synthetic` |

`telemetry::synthetic` is the inverse of the parser and sits beside it
deliberately: round-trip tests fail if the two offset tables ever disagree, so a
change to one cannot silently skip the other.

## Generating telemetry without the game

```bash
cargo run --example synthetic_telemetry -- --format fh6
```

`--format` accepts `sled`, `dash`, or `fh6`, which makes it easy to see how the
HUD behaves on each. See [testing strategy](../development/testing_strategy.md).

## Provenance

The offsets were derived from the community-documented Forza "Data Out" layout
and verified against live captures during development. They are not published by
Microsoft and are not guaranteed stable across titles.

Do not change an offset without evidence — see
[contributing](../../CONTRIBUTING.md).
