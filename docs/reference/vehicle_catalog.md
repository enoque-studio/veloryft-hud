# Vehicle catalog

Forza identifies a car by an integer called the **car ordinal**, which it sends
in every telemetry packet. It does not send the car's name. Veloryft maps the
ordinal to a readable name using a catalog.

## No catalog is bundled

Veloryft does not redistribute a vehicle catalog. The community dataset from
which the original copy was derived has an identified author but no declared
reuse license, so embedding it would make a permission claim the project cannot
support.

Without a catalog, an ordinal such as `247` is displayed as `#247`. Telemetry,
shift-point learning, and every other HUD feature continue to work.

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

## Supplying your own catalog

Point `FORZA_CAR_ORDINALS` at a compatible local file:

```powershell
$env:FORZA_CAR_ORDINALS = "C:\Users\me\my-cars.json"
```

Veloryft also looks for `data/car_ordinals.json` in the working directory and
next to the executable, in that order. The first valid, non-empty file wins.

The file is local configuration and is not uploaded. Include every car you want
named; catalogs are not merged.

## When a car is unknown

Nothing breaks. An unknown ordinal is shown with a `#` prefix, and the rest of
the HUD is unaffected. A malformed entry is skipped without discarding the rest
of the file. If the file cannot be parsed, names remain unavailable and the HUD
uses ordinals.

The number of loaded cars is written to the log at startup, which is the quickest
way to confirm an override was picked up.

## Rev-limit calibration is keyed on the ordinal

The learned shift points in `rev-limits.json` are stored per ordinal, not per
name. Editing the catalog therefore never affects calibration, and an unnamed
car still learns normally.

## Known community source

The removed catalog was traced to Martin Refseth (`HDR`):
[Forza Horizon 6 Car Ordinals](https://gist.github.com/HDR/0659d1717bc61504bf83750628963f4f).
The author states that the identifiers were extracted from the game's vehicle
files. The gist does not declare a reuse license, so Veloryft links to it for
attribution but neither mirrors nor downloads it.

If you choose to use external data, review its terms yourself. A future catalog
may be bundled only when it is independently produced for Veloryft or published
under an explicit compatible license.

Vehicle manufacturer and model names are trademarks of their respective owners
and are used here descriptively, to identify which car a telemetry packet refers
to.
