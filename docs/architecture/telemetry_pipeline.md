# Telemetry pipeline

From a datagram arriving to a value the panels can draw.

```
UdpSocket::recv_from        receiver.rs    dedicated thread, blocking
        │
        ▼  &[u8]
packet::parse               packet.rs      length picks the layout
        │
        ▼  Packet<'_>       borrowed, zero copy
model::build                model.rs       normalize units, resolve car name
        │
        ▼  Telemetry        owned, immutable
Shared::store               snapshot.rs    ArcSwap: one atomic store
        │
        ▼
DisplaySmoother::tick       display.rs     called by the render loop
        │
        ▼  DisplayFrame     what the panels draw
```

## Reception

`telemetry::receiver::spawn` binds `0.0.0.0:<port>` and blocks in `recv_from` on
its own thread. Blocking is the right call here: the thread has nothing else to
do, and a blocking read is the lowest-latency way to be woken by a packet.

The buffer is a 2 KiB stack array reused for every datagram, so the receive path
allocates nothing per packet beyond the `Arc` that publishes the frame.

`spawn` returns the bind error rather than swallowing it, so the caller can tell
the user *which* port is unavailable. A failing read is reported once and then
suppressed — a broken socket fails on every iteration of a tight loop, and a log
line per iteration would be worse than the failure.

## Decoding

`telemetry::packet` reads by offset from a borrowed slice. No copy, no struct
deserialization, no allocation. Accessors pull little-endian values on demand,
so a field nobody asks for costs nothing.

The datagram length picks the layout. A 323-byte packet is ambiguous, so both
interpretations are scored on whether their fields land in physically sensible
ranges and the better one wins. See
[telemetry formats](../reference/telemetry_formats.md).

Anything shorter than 232 bytes is dropped. Longer datagrams are accepted and
the tail ignored.

## The domain model

`telemetry::model::build` turns a borrowed `Packet` into an owned `Telemetry`:
metres per second become km/h, watts become metric horsepower, raw byte inputs
become percentages, and the car ordinal is resolved to a name.

This is the boundary where borrowing ends. Everything downstream holds an owned,
immutable frame, which is what makes lock-free publication possible.

Fields the current packet cannot supply are `Option`, not zero. A Sled packet
genuinely has no speed, and the difference between "no data" and "zero" is the
difference between hiding the HUD and showing a stopped car.

## Publication

`telemetry::snapshot::Shared` wraps `ArcSwap<Telemetry>`.

- The receive thread calls `store`: one atomic pointer store.
- Readers call `latest`: one atomic load and a refcount bump.

Neither blocks the other. A reader that is mid-frame keeps the frame it took;
the producer never waits for it. Frames are dropped rather than queued, which is
correct for a HUD — a stale frame is worthless, and the newest one is always
what should be on screen.

Each frame carries a monotonic counter, which is how the throttle thread tells a
new frame from a repeat without comparing values.

## Derived state

The raw frame is not what gets drawn. `telemetry::display` maintains the
smoothing and the state that depends on history:

| Module | Derives |
| ------ | ------- |
| `display` | Exponential smoothing per channel, plus the fade in and out |
| `visibility` | Whether the HUD should be on screen at all |
| `rev_limit` | The shift point and limiter learned for this car |
| `tire_temperature` | Session minimum and maximum per corner |
| `elevation` | The lap's elevation profile and the position along it |

Smoothing is deliberate: raw telemetry at 60 Hz makes a bar jitter. Each channel
has its own time constant — the throttle bar attacks faster than it releases,
because a driver lifting is information and a driver flooring it is more urgent.

### Visibility

The HUD appears only while the player is actually driving. `is_race_on` is the
primary signal, but free roam sends zero while driving, so the rule also
considers packet age, speed, engine speed above idle, and driver input. A car
moving at speed with the engine at idle and no input is a scripted camera, not a
driver.

### Rev-limit learning

`rev_limit` watches full-throttle behaviour to find where the limiter actually
cuts — a plateau in engine speed that was *climbed into*, so that a hard
downshift over-rev does not register as a limiter — and builds a coarse power
curve from the observed acceleration and the RPM drop across upshifts.

The recommended shift point is the highest engine speed at which holding the
gear still produces more power than the next gear would.

Calibration persists per car ordinal in `rev-limits.json`. Changing the engine
alters the rev range, which resets it.

## Testing it

The pipeline is tested at every level:

- **Unit** tests in each module.
- **Round-trip** tests between `packet` and `synthetic`, which is what stops the
  two offset tables from drifting apart.
- **Integration** tests in `tests/telemetry_pipeline.rs` that run a real socket
  through the real parser into the real snapshot, including truncated and
  oversized datagrams and every visibility transition.

No captured telemetry is redistributed with the project. Every fixture is
generated from plain numbers by `telemetry::synthetic`.
