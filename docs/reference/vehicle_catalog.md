# Vehicle catalog

Forza identifies a car by an integer called the **car ordinal**, which it sends
in every telemetry packet. It does not send the car's name. Veloryft maps the
ordinal to a readable name using a catalog.

## Where the catalog comes from

The catalog is `data/car-ordinals.json`, embedded into the binary at compile
time. Installed and portable builds therefore always have it, with no file
beside the executable.

The format is a JSON object mapping name to ordinal, as a string:

```json
{
  "1969 Toyota 2000 GT": "247",
  "1964 Ferrari 250 GTO": "249"
}
```

The name is the key, which is backwards from how it is used. That is the shape
the file arrived in, and it is kept so that existing override files continue to
work.

## Overriding it

Point `FORZA_CAR_ORDINALS` at a replacement file to add cars without
recompiling:

```powershell
$env:FORZA_CAR_ORDINALS = "C:\Users\me\my-cars.json"
```

Veloryft also looks for `data/car-ordinals.json` in the working directory and
next to the executable, in that order. The first file that loads and is
non-empty wins; the embedded copy is the fallback.

An override **replaces** the catalog rather than merging with it, so include
every car you want named.

## When a car is unknown

Nothing breaks. An unknown ordinal simply shows no name, and the rest of the HUD
is unaffected. A malformed entry is skipped without discarding the rest of the
file, and a catalog that fails to parse leaves the embedded copy in place.

The number of loaded cars is written to the log at startup, which is the quickest
way to confirm an override was picked up.

## Rev-limit calibration is keyed on the ordinal

The learned shift points in `rev-limits.json` are stored per ordinal, not per
name. Editing the catalog therefore never affects calibration, and an unnamed
car still learns normally.

## Licensing

**The provenance of `data/car-ordinals.json` is unresolved, and that is a
release blocker.** It was added in the repository's initial commit with no
recorded source, author, or license.

See [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) for what is known,
and what has to be established before the file can be published with confidence.

Vehicle manufacturer and model names are trademarks of their respective owners
and are used here descriptively, to identify which car a telemetry packet refers
to.
