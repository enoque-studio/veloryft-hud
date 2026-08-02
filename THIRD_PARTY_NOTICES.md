# Third-party notices

Veloryft itself is licensed under Apache-2.0 (see [LICENSE](LICENSE)). This file
records everything else the project redistributes or depends on.

## Rust dependencies

These crates are compiled into the released binary. The list covers the default
build **and** the optional `track_mapping` feature, because the same source tree
can produce either.

Regenerate it with:

```bash
cargo tree --all-features -e normal --prefix none
```

Every license below is permissive and compatible with redistributing a binary
under Apache-2.0. `cargo deny check` enforces that on every pull request; the
allowed set lives in [deny.toml](deny.toml).

| Crate | Version | License |
| ----- | ------- | ------- |
| [arc-swap](https://github.com/vorner/arc-swap) | 1.9.2 | MIT OR Apache-2.0 |
| [ascii](https://github.com/tomprogrammer/rust-ascii) | 1.1.0 | Apache-2.0 OR MIT |
| [chunked_transfer](https://github.com/frewsxcv/rust-chunked-transfer) | 1.5.0 | MIT OR Apache-2.0 |
| [httpdate](https://github.com/pyfisch/httpdate) | 1.0.3 | MIT OR Apache-2.0 |
| [itoa](https://github.com/dtolnay/itoa) | 1.0.18 | MIT OR Apache-2.0 |
| [log](https://github.com/rust-lang/log) | 0.4.33 | MIT OR Apache-2.0 |
| [memchr](https://github.com/BurntSushi/memchr) | 2.8.3 | Unlicense OR MIT |
| [proc-macro2](https://github.com/dtolnay/proc-macro2) | 1.0.107 | MIT OR Apache-2.0 |
| [quote](https://github.com/dtolnay/quote) | 1.0.47 | MIT OR Apache-2.0 |
| [rustversion](https://github.com/dtolnay/rustversion) | 1.0.23 | MIT OR Apache-2.0 |
| [serde](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_core](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_derive](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | 1.0.151 | MIT OR Apache-2.0 |
| [syn](https://github.com/dtolnay/syn) | 3.0.3 | MIT OR Apache-2.0 |
| [tiny_http](https://github.com/tiny-http/tiny-http) | 0.12.0 | MIT OR Apache-2.0 |
| [unicode-ident](https://github.com/dtolnay/unicode-ident) | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [windows](https://github.com/microsoft/windows-rs) | 0.61.3 | MIT OR Apache-2.0 |
| [windows-collections](https://github.com/microsoft/windows-rs) | 0.2.0 | MIT OR Apache-2.0 |
| [windows-core](https://github.com/microsoft/windows-rs) | 0.61.2 | MIT OR Apache-2.0 |
| [windows-future](https://github.com/microsoft/windows-rs) | 0.2.1 | MIT OR Apache-2.0 |
| [windows-implement](https://github.com/microsoft/windows-rs) | 0.60.2 | MIT OR Apache-2.0 |
| [windows-interface](https://github.com/microsoft/windows-rs) | 0.59.3 | MIT OR Apache-2.0 |
| [windows-link](https://github.com/microsoft/windows-rs) | 0.2.1 | MIT OR Apache-2.0 |
| [windows-numerics](https://github.com/microsoft/windows-rs) | 0.2.0 | MIT OR Apache-2.0 |
| [windows-result](https://github.com/microsoft/windows-rs) | 0.3.4 | MIT OR Apache-2.0 |
| [windows-strings](https://github.com/microsoft/windows-rs) | 0.4.2 | MIT OR Apache-2.0 |
| [windows-threading](https://github.com/microsoft/windows-rs) | 0.1.0 | MIT OR Apache-2.0 |
| [zmij](https://github.com/dtolnay/zmij) | 1.0.23 | MIT |

Dual licenses such as `MIT OR Apache-2.0` are used under Apache-2.0 for
consistency with this project's own license.

`unicode-ident` additionally carries the Unicode-3.0 license for its embedded
Unicode character tables, which permits redistribution with attribution.

## Data

### data/car-ordinals.json

**Status: provenance unresolved — this is a release blocker.**

The file maps 638 Forza `car_ordinal` values to human-readable car names. It was
added in the repository's initial commit with no recorded source, author, or
license, and no upstream can be identified from the repository history.

What is known:

- The mapping is *derived from* the game: an ordinal is an identifier the game
  transmits over its own telemetry output, and the names describe real vehicles
  by year, manufacturer, and model.
- Ordinal-to-name tables of this kind circulate in the Forza telemetry community
  in several near-identical forms, so this file may be a copy, a derivative, or
  original work.
- The names themselves are factual descriptions of real vehicles. Manufacturer
  and model names are trademarks of their respective owners, used here only to
  identify the vehicle a telemetry packet refers to.

Before the first public release, the owner must establish one of the following
and record it here:

1. The file is original work of this project — state that, and it is covered by
   this repository's Apache-2.0 license.
2. The file came from an identifiable source — record the project, the author,
   the license, and any transformation applied, and add the required attribution.
3. Provenance cannot be established — regenerate the mapping from first-hand
   observation, or ship without it. The application already degrades gracefully:
   `telemetry::vehicle_catalog` falls back to showing the raw ordinal when no
   name is known.

Until this is resolved, treat the file as **requires_review**. It is documented
here rather than removed silently, and no claim is made about its license.

## Brand assets

The artwork in `assets/brand/` and the installer artwork generated from it
(`installer/assets/`) are project trademarks, not Apache-2.0 content. See
[TRADEMARKS.md](TRADEMARKS.md).

## Third-party trademarks

"Forza", "Forza Horizon", "Forza Motorsport", "Xbox", and "Microsoft" are
trademarks of Microsoft Corporation. "Turn 10 Studios" and "Playground Games"
are names of their respective owners. Vehicle manufacturer and model names in
`data/car-ordinals.json` are trademarks of their respective owners.

Veloryft is not affiliated with, sponsored by, or endorsed by any of them. Every
reference is descriptive: it identifies what the software is compatible with.

Veloryft reads only the telemetry the game itself transmits through its own
"Data Out" feature when the user enables it. It does not modify the game, read
or write its memory, or inject code.
