# Localization

English is Veloryft's source language and its only shipped locale today. The
application is internationalized: user-facing text is separated from rendering,
windowing, telemetry, and diagnostics so another complete locale can be added
without changing those systems.

## Source-language policy

Use English for:

- Rust and script identifiers;
- comments, docstrings, tests, assertions, logs, and error details;
- repository documentation and contribution material;
- the fallback text shown when a translation is unavailable.

Brand names, protocol field names, persisted configuration keys, and Forza's
official menu labels remain unchanged where compatibility or accurate setup
instructions require them.

## Typed catalog

[`src/localization/mod.rs`](../../src/localization/mod.rs) defines the catalog
contract and locale-aware value formatting. The complete English catalog lives
in [`src/localization/en_us.rs`](../../src/localization/en_us.rs).

Visible labels must be obtained through `localization::text()`. Dynamic values
with localized units use the formatting functions in the same module. Startup
dialogs and actionable network errors are composed there as well. Logs remain
technical English and are not translated, which keeps support searches and bug
reports consistent.

The catalog stores `&'static str` values. Looking up a label does not allocate,
lock, perform I/O, or add work to the render path. UTF-16 needed by the critical
throttle surface is prepared once at surface creation rather than every frame.

## Adding a locale

Do not ship a partial translation. A new locale requires:

1. A catalog module that initializes every `Catalog` field.
2. A BCP 47 language tag suitable for DirectWrite.
3. Locale selection with English as the explicit fallback.
4. Review by a fluent speaker in the actual HUD at supported DPI scales.
5. Tests for selection, fallback, units, and any text whose length affects layout.
6. Updated installer and end-user documentation in the same pull request.

Keep translations semantic rather than word-for-word. Context matters: `GEAR`
in the cluster, `GLASS` in the appearance editor, and `MIN` in the elevation
panel are distinct translation units even when another language happens to use
the same word.

## What must not be localized

Do not translate or rename:

- environment variables such as `FORZA_UDP_PORT`;
- keys already persisted in `layout.txt` and `theme.txt`;
- HTTP routes and public JSON field names;
- packet-format terminology used by Forza;
- filenames or paths that form a compatibility contract.

Changing those values is a migration, not a translation.
