# The local HTTP server

**This server does not exist in the released binaries.** It is compiled only
when the `track_mapping` feature is enabled, and it is off by default.

If you are running an official release, there is nothing here to worry about:
the code is not in the binary. This page describes what it is for anyone
building with the feature on.

## What it is

`track_mapping::local_api` serves the recorded world map to a 3D viewer running
on the same machine.

| | |
| --- | --- |
| Address | `127.0.0.1:9877` |
| Override | `VELORYFT_LIVE_PORT` |
| Transport | Plain HTTP |
| Authentication | None |

## Routes

| Method | Path | Effect |
| ------ | ---- | ------ |
| GET | `/live` | Current position and recording state |
| GET | `/map` | The accumulated height map |
| GET | `/path` | The driven path |
| POST | `/path/delta` | Only what changed since the caller's last fetch |
| POST | `/recording` | Start or stop recording |
| POST | `/fix` | Freeze the path |
| POST | `/reset` | Clear the local map |

Four of these change state.

## Threat model

### Who can reach it

**Any process running on the same machine**, at any privilege level, including
one running as a different user.

It binds `127.0.0.1`, so nothing on the network can reach it — but loopback is
not a security boundary between local processes. It is not a firewall.

### What an attacker on the machine can do

- Read the recorded map: where you have driven in the game.
- Start or stop recording.
- Reset the map, destroying local recordings.

### What they cannot do

- Reach any other part of Veloryft. The API touches only the recorder.
- Read files, run code, or affect the overlay.
- Cause unbounded memory use: request bodies are capped at 16 KiB and rejected
  with 413 above it.

### Why there is no authentication

Deliberate. The API is a development aid for a viewer on the same machine, and
the data is a map of virtual terrain. Adding a token would mean sharing it with
the viewer through a file or an environment variable — both readable by the same
processes the token would supposedly exclude. It would look like a boundary
without being one.

The real boundary is that **the code is not compiled into a release**.

## Protections that are there

**Loopback only.** `Server::http("127.0.0.1:port")`. Not `0.0.0.0`, ever.

**Origin allow-list.** A browser sending an `Origin` header gets 403 unless the
origin is `http://localhost` or `http://127.0.0.1`, optionally with a numeric
port. This is what stops a web page you have open from issuing state-changing
requests to the server through your browser.

The check is exact, and tested against the near misses:

| Origin | Allowed |
| ------ | ------- |
| `http://localhost:5173` | Yes |
| `http://127.0.0.1:4173` | Yes |
| `http://localhost.evil.example:5173` | No — a registrable hostname that merely ends in `localhost` |
| `http://127.0.0.1.evil.example` | No |
| `https://localhost:5173` | No — the viewer is plain HTTP; accepting TLS widens the surface for nothing |
| `http://localhost:notaport` | No |

**Body size cap.** 16 KiB, enforced by reading at most that plus one byte, so a
caller cannot make the server allocate by claiming a large body.

**Segment cap.** `/path/delta` refuses more than 8,192 claimed segments. In
practice the body limit binds first — a 16 KiB body cannot encode that many
pairs — and both are tested, so the cheap check rejects before the expensive one
is reached.

**Response headers.** `Cache-Control: no-store` and
`X-Content-Type-Options: nosniff` on every JSON response.

## If you enable it

- Do not change the bind address. `0.0.0.0` would expose the routes above to
  your network, including the ones that change state.
- Do not port-forward it.
- Treat the recorded map as data any local process can read.
- Prefer a port that nothing else uses; the server logs a clear error if the
  bind fails.

## Testing it

```bash
cargo test --features track_mapping
```

The origin check and the size limits are covered by unit tests in
`track_mapping::local_api`; the recorder is covered end to end in
`tests/track_mapping.rs`.

`read_limited_body` is split from its HTTP wrapper specifically so the cap can
be tested without standing up a server, because that cap is the only thing
between this endpoint and a local process exhausting memory.

## See also

- [Optional features](../architecture/optional_features.md)
- [ADR 0004](../decisions/adr_0004_track_mapping_feature.md)
