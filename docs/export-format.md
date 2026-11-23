# World export format (JSON)

**Export → JSON** in the app writes the whole world as one JSON document.
The generator writes it in `core/src/export.rs` (`WorldExport`). Everything a
story needs is in it: the physical summary, the peoples, settlements, realms
and landmarks at year 0, and the complete event log, from which any year can
be rebuilt.

Current schema version: **1**. It changes whenever a field is added, removed
or renamed; readers should check `schema_version` first.

## Conventions

- **Cells.** The world is a `width × height` grid. A cell index is
  `y * width + x`, with (0, 0) at the top left.
- **Ids.** Ids are indices into the matching list. For example,
  `settlements[3]` has `id` 3, and realms, peoples, landmarks and rivers
  work the same way. Settlements founded during history continue the
  settlement numbering, so their ids appear in the event log only.
- **Heights.** Heights are normalised to `0..1`. Land is at or above
  `terrain.sea_level`. The elevation in metres is
  `(h - sea_level) / (1 - sea_level) * max_elevation_m`.
- **Grids.** Per-cell grids are base64 (RFC 4648, padded) of little-endian
  values, row by row.
- **Bit grids.** A bit grid (`blight`) is a `u32` count followed by one bit
  per cell, lowest bit first.

## Top level

| Field | Type | Meaning |
|---|---|---|
| `schema_version` | u32 | Version of this format. |
| `generator_version` | u32 | Generator that made the world. The same seed and config only give the same world under the same generator version. |
| `share_code` | string | Paste into the app's share box to regenerate this exact world. |
| `config` | object | The full `WorldConfig` (see `core/src/config.rs`). |
| `terrain` | object | Summary of the terrain; see below. |
| `biomes` | array | `{biome, share}`: share of land in each biome, largest first. |
| `peoples` | array | See below. |
| `settlements` | array | Settlements at year 0. |
| `realms` | array | Realms at year 0. |
| `landmarks` | array | Landmarks at year 0. Ruins left by the Blight and by disasters appear in the history. |
| `river_names` | array of string | Name of each river, by river id. |
| `deposits` | array | `{cell, resource, richness}`. `resource` is one of `Ore`, `Timber`, `Grain`, `Salt`, `Gems`; `richness` runs `0..1`. |
| `roads` | array | `{a, b, kind, cost}`: a link between settlements `a` and `b`. `kind` is `road`, `river` or `sea`; `cost` is the travel cost. |
| `history` | object | The event log; see below. |
| `grids` | object or null | Per-cell grids, present only when "include grids" was ticked. |

### `terrain`

| Field | Meaning |
|---|---|
| `width`, `height` | Grid size. |
| `sea_level` | Normalised sea level. |
| `max_elevation_m` | Metres at the highest possible height. |
| `land_fraction` | Share of cells above the sea. |
| `highest_cell`, `highest_m` | Where the highest peak is, and its height in metres. |
| `lakes`, `rivers` | Counts. |

### `peoples[]`

`id`, `name`, `trait_name` (for example "river folk"), `origin` (cell), `cells`
(homeland size), `expansionism`, `seafaring` and `insularity` (each `0..1`),
`language_seed` and `language_id`.
`worldgen_core::names::Language::generate(language_seed, language_id)`
rebuilds the language, so new names can be made in it.

### `settlements[]`

`id`, `name`, `cell`, `x`, `y`, `people`, `rank` (`Village`, `Town`, `City`,
`Capital`), `population`, and the flags `port`, `river_crossing`,
`mountain_pass` and `oasis`.

### `realms[]`

| Field | Meaning |
|---|---|
| `id`, `name`, `short_name` | |
| `government` | For example "Kingdom". |
| `capital` | Settlement id. |
| `people` | |
| `house` | Ruling house. |
| `founder` | First ruler's name. |
| `arms` | Blazon, for example "Azure, a Tower Or". |
| `colour` | Map palette index. |
| `cells` | Land size. |

### `landmarks[]`

`id`, `kind`, `name`, `hook` (a story hook naming real nearby places) and
`cell`. `kind` is one of `DragonDen`, `ElderRuin`, `LeylineNexus`,
`DrownedCity`, `BlightedRuin` or `FallenTown`.

### `history`

| Field | Meaning |
|---|---|
| `years` | Years simulated after year 0. |
| `snapshot_interval` | Years between snapshots. The app uses these for speed; they are rebuilt on import. |
| `initial` | The state before any event: a `HistoryState`, with `owner` (`u16` grid, 65535 for no realm) and `blight` (bit grid) encoded as above. |
| `events` | Every event, ordered by `(year, seq)`. Each event is `{year, seq, kind}`, where `kind` is one object keyed by the event's name, for example `{"RulerSucceeded": {"realm": 2, "ruler": {...}, "cause": "OldAge"}}`. |

The state at year *Y* is `initial` with every event up to and including year
*Y* applied in order. `HistoryState::apply` in `core/src/history/state.rs`
is the reference. `WorldExport::history()` rebuilds a full `History` from the
log, and a test checks that it matches the original for every year.

Causes chain. `"cause": {"Event": 1234}` means the event at index 1234 of
`events` caused this one. For example, a famine points to the drought's
hard times, and a migration points to that famine. Other causes are plain
strings such as `"OldAge"`, `"Tectonics"` or `"Weather"`.

The event kinds and their fields are the variants of `EventKind` in
`core/src/history/event.rs`, which documents each one. They are grouped
here.

| Group | Events |
|---|---|
| Realms | `RealmFounded`, `RulerSucceeded`, `BorderChanged`, `RealmFell`, `RealmSplit`, `RealmAnnexed` |
| Settlements | `SettlementFounded`, `SettlementGrew`, `SettlementShrank`, `SettlementRenamed`, `SettlementStruck`, `SettlementStranded` |
| Trade | `TradeSeason`, `TradeRouteOpened`, `TradeRouteClosed`, `TradeHub`, `Shortage` |
| Diplomacy | `OpinionsShifted`, `AllianceFormed`, `AllianceBroken`, `RivalryDeclared`, `TradePactSigned`, `MarriageArranged`, `VassalSworn`, `VassalFreed` |
| War | `WarDeclared`, `WarJoined`, `Battle`, `Siege`, `PeaceSigned` |
| Landmarks | `DragonAwakened`, `DragonRaid`, `DragonSlept`, `DragonSlain`, `RuinDiscovered`, `NexusClaimed`, `BlightSpread`, `BlightPurged`, `BlightReachedSettlement` |
| Disasters | `VolcanoErupted`, `Earthquake`, `Drought`, `HardTimes`, `Famine`, `Migration`, `Flood`, `RiverAvulsion`, `PlagueOutbreak`, `PlagueSpread`, `PlagueToll` |

### `grids` (optional)

| Field | Values |
|---|---|
| `height` | `f32` normalised height after erosion. |
| `biome` | `u8` index into `biome_names`. |
| `biome_names` | Biome names in index order. |
| `homeland` | `u16` people id, 65535 for none. |
| `realm` | `u16` realm id at year 0, 65535 for none. |

## Reading it

In Python, for example:

```python
import base64, json, struct
world = json.load(open("world.json"))
w, h = world["terrain"]["width"], world["terrain"]["height"]
heights = struct.unpack(f"<{w*h}f", base64.b64decode(world["grids"]["height"]))
for e in world["history"]["events"]:
    (kind, fields), = e["kind"].items()
    if kind == "Battle":
        print(e["year"], fields["name"])
```
