# Security model

What Veloryft does, what it does not do, and what it can and cannot protect you
from.

## What it is

One Windows executable that:

- Binds a UDP port and reads datagrams the game broadcasts.
- Draws windows.
- Reads and writes plain-text files in your own profile directory.

That is the whole footprint of a default build.

## What it does not do

- **Does not read or write the game's memory**, and does not inject code. It
  reads only what the game sends through its own Data Out feature, which you
  switch on.
- **Does not make outbound network connections.** The default build contains no
  HTTP client, no TLS stack, and no code that opens a socket to anywhere.
- **Does not open a listening socket** other than the UDP receiver. The loopback
  HTTP server exists only with the `track_mapping` feature, which the released
  binaries do not have. See [local HTTP server](local_http_server.md).
- **Does not require an account, a key, or a connection.** There is no license
  check in the open beta and no code that would perform one.
- **Does not collect analytics or telemetry about you.**
- **Does not run with elevated privileges.** The installer is per-user by
  default and the application never asks for elevation.

## Trust boundaries

| Boundary | What crosses it | How it is handled |
| -------- | --------------- | ----------------- |
| UDP socket | Datagrams from anything that can reach the port | Length-checked, offset-read, never allocated from |
| Settings files | Text the user or another local program can edit | Parsed leniently, values clamped |
| Vehicle catalog | JSON from a file or the embedded copy | Malformed entries skipped; failure falls back |
| Loopback HTTP API | Requests from local processes | Only with `track_mapping`; loopback-bound, origin-checked, body-capped |

### The UDP receiver

The socket binds `0.0.0.0`, so anything that can route to your machine on that
port can send to it. That is the same as any UDP listener and is what allows
telemetry from a second PC, which some setups use.

Datagrams are never trusted:

- Anything under 232 bytes is dropped.
- Everything is read by fixed offset from a length-checked slice, with no
  deserialization into a structure and no allocation driven by packet contents.
- Values that fail plausibility checks cause the format detection to prefer
  another interpretation rather than producing nonsense.

The worst a malicious datagram achieves is a wrong number on screen for one
frame.

If you do not want the port reachable from your network, bind it behind a
firewall rule or use a port your firewall already blocks inbound.

### Settings files

Plain text in your profile. Any program running as you can read or modify them —
which is true of everything in `%APPDATA%` and is not a vulnerability in
Veloryft.

Parsing assumes hostile input regardless: unparsable lines are skipped rather
than discarding the file, and every value is clamped on load. A hand-edited or
corrupted theme cannot put the overlay into a state the UI cannot undo.

## Threats it does not defend against

Stated plainly, because a security document that only lists strengths is not
useful:

- **Another program running as you.** It can read your settings, read the logs,
  send fake telemetry to the port, or replace the executable. Nothing
  user-space can prevent that; it is the operating system's boundary, not the
  application's.
- **A tampered download.** Releases are unsigned. Checksums prove transport
  integrity, not authorship. See [code signing](../distribution/code_signing.md).
- **A malicious build from a third party.** Verify checksums, and get releases
  from the official repository.
- **Telemetry sent from another machine.** If you deliberately point a second
  PC's Data Out at this one, you are trusting that machine.

## Hardening choices

**Static CRT.** No dependency on a system-wide redistributable that could be
replaced. See [ADR 0002](../decisions/adr_0002_static_crt.md).

**No unsafe outside the overlay.** `telemetry` has none, and
`crates/veloryft_protocol` sets `#![forbid(unsafe_code)]`. Everything that
parses untrusted bytes is safe Rust.

**A minimal dependency graph.** Five direct dependencies, twenty-four crates in
all. Every dependency is code you inherit, and there is not much of it.

**crates.io only, enforced.** `deny.toml` refuses git and out-of-workspace path
dependencies, so nothing enters the graph from a source that cannot be audited.

**No certificate validation weakened.** An earlier version of this repository
disabled Cargo's certificate revocation checking for everyone who cloned it. It
does not any more; the workaround is documented as a local, opt-in choice in
[development setup](../development/development_setup.md).

## Logging

The log records what failed, never what you were driving. Telemetry values,
machine identifiers, and file contents are excluded by design, and that is a
constraint on what may be added to it.

See [privacy](privacy.md).

## Reporting

Privately, through GitHub Security Advisories. Never in a public issue. See
[SECURITY.md](../../SECURITY.md).
