# veloryft-protocol

Serialization contract for Veloryft world-map data.

These types cross a process boundary. The Veloryft HUD produces them while the
player drives, the optional local viewer API serves them over loopback, and the
Veloryft backend consumes them on upload. They live in their own crate so that
producer and consumer agree on one definition without either repository having to
depend on the other's source tree.

This crate contains **only data definitions and one merge rule**. It performs no
I/O, opens no sockets, and contains no `unsafe` code.

## Compatibility policy

- Fields added after the first release are `#[serde(default)]`, so older map
  files and older clients still deserialize.
- Adding a field is a minor version bump.
- Removing or renaming a field, or changing the meaning of an existing one, is a
  major version bump.
- `SURFACE_KINDS` indices are wire values. Do not reorder them.

The tests in `src/lib.rs` pin the serialized field names and verify that
pre-existing map files still load. Treat a failure there as a compatibility
break, not a test to update.

## Relationship to the HUD

The HUD depends on this crate only when the `track_mapping` feature is enabled.
The default HUD build never compiles it.

See [`docs/decisions/adr_0003_public_protocol_boundary.md`](../../docs/decisions/adr_0003_public_protocol_boundary.md)
for why this crate exists.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
