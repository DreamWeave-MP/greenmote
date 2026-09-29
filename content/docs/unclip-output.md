+++
title = "The unclip report"
description = "Every line of the unclip report, every verdict label, the per-reference log, and the JSON that --structured prints."
weight = 70

[extra]
kind = "reference"
+++

Every unclip run prints a report to standard output and writes the same report to
`greenmote-unclip.log`, next to your user `openmw.cfg`, replacing the previous one. With
`--verbose`, the log also gets one line per reference. With `--structured`, standard output is
JSON instead of text; the log stays text.

## The text report

Fantasia's Molag Amur plugin, checked before it is enabled, against a vanilla load order:

```text
Unclip report for Fantasia/FGM_MA.esp
Added Fantasia to the VFS because the plugin is not in a configured data directory
Mode: dry run (pass --write to rewrite Fantasia/FGM_MA.esp)
Actions: terrain-z, water-delete, road-delete, static-delete, static-move, orient
Tolerances: float 1 / max sink 24 units or 75% of height / sink to 4 / tilt 1 deg / max tilt 45 deg

Cells: 119 target, 194 active, 194 with terrain, 0 missing terrain
Occluders: 16546 statics (11275 collision meshes, 2052 visual fallbacks, 3219 without collision), 0 refs excluded as grass, 3080 excluded by filter, 4822 unresolved

Refs: 10591 matched, 7850 fine, 2361 to fix, 380 to delete, 0 skipped
  delete_inside_static             322
  delete_no_relocation             47
  delete_road                      11
  fix_ground                       1436
  fix_move                         925

Nothing written (dry run).
```

| Line | Says |
|---|---|
| `Unclip report for` | The plugin, as found |
| `Added ... to the VFS` | A mod folder added to find the plugin's meshes. See [the target](@/docs/unclip.md#the-target) |
| `Mode` | `dry run`, and what `--write` would rewrite; or `rewrite` and the file being written |
| `Actions` | The actions planned |
| `Tolerances` | `float_tolerance`, `max_sink`, `max_sink_fraction` as a percentage, `sink`, `orientation_epsilon` and `max_tilt` |
| `Cells` | Exterior cells holding target references; cells loaded around them, 3 by 3 around each; how many of those have terrain; how many do not |
| `missing terrain` | Up to 12 cells without terrain. References there are skipped |
| `Occluders` | Static references measured in those cells, by where their collision came from: a `RootCollisionNode`, the visible geometry, or none. Then references left out: the target's own grass statics placed by content files, those `exclude_occluder_ids` removes, and references to anything that is not a static |
| `WARNING: ... static occluder meshes could not be loaded` | Up to 20 of them, with the error. Clipping into those statics was not detected |
| `Refs` | Target references measured, and their verdicts. `fine` counts every `keep` verdict; the lines under it count each verdict other than plain `keep` |
| `WARNING: ... refs were skipped because their mesh could not be loaded` | Up to 10 distinct errors, with how many references each hit. Usually the plugin's mod folder is not a data folder |

The last line is the outcome:

| Line | When |
|---|---|
| `Nothing written (dry run).` | There are changes, and no `--write` |
| `Nothing to write.` | There are no changes |
| `Wrote PATH (F fixed, D deleted, C cells), verified` | `--write`, followed by one `backup:` line per backup made |

## Verdicts

Every reference gets exactly one. [Unclip](@/docs/unclip.md#the-rules-in-order) explains the rules
that give them.

| Verdict | Meaning |
|---|---|
| `keep` | Nothing is wrong with it |
| `keep_road_delete_disabled` | It stands on a road texture, and road-delete is off |
| `keep_water_delete_disabled` | It crosses the water plane, and water-delete is off |
| `keep_orient_disabled` | Its tilt is off by more than `orientation_epsilon`, and orient is off |
| `keep_too_steep` | Its tilt is off, but the ground is steeper than `max_tilt` |
| `keep_terrain_seam` | It stands on a seam between two landmasses, where nothing can be measured |
| `keep_terrain_z_disabled` | It floats or is buried too deep, and terrain-z is off |
| `keep_static_actions_disabled` | It clips into a static, and static-move and static-delete are off |
| `keep_no_relocation_found` | It clips into a static, no clear spot was found, and static-delete is off |
| `fix_ground` | Moved up or down to rest on the terrain |
| `fix_orient` | Tilted to the slope |
| `fix_ground_and_orient` | Both |
| `fix_move` | Moved sideways out of a static, and tilted or grounded at its new spot as needed |
| `delete_road` | It stands on a road texture |
| `delete_water` | It was placed above the water plane, and its terrain is now below it |
| `delete_inside_static` | It is entirely inside a static |
| `delete_no_relocation` | It clips into a static and could not be moved, or static-move is off |
| `delete_outside_cell` | Its position lies outside the cell that holds it |
| `skip_deleted_ref` | It is already deleted |
| `skip_unresolved_static` | Its ID is not a static in the load order or the plugin |
| `skip_mesh_error` | Its mesh does not load |
| `skip_no_terrain` | There is no terrain under its base |

A kept reference with several reasons shows the first one found, in the order of the rules.

## The per-reference log

With `--verbose`, after the report, `greenmote-unclip.log` holds a tab-separated table with one line
per reference:

```text
cell	key	id	verdict	detail
(-9, -2)	0:22	AZBC13p0k11r85tto	fix_ground	to=(-65848.6, -8571.8, 301.9) rot=(-0.155, 0.386, 1.475) gap 8.12 -> -4.00 [terrain_z=287.7 height=72 gap=8.12 min_gap=-4.75 tilt_delta=0.0deg]
(-9, -2)	0:26	AZBCg4h9knjbrnbz	keep	{"kind":"fine"} [terrain_z=268.1 height=71 gap=-25.15 min_gap=-25.35 tilt_delta=0.0deg]
(-9, -2)	0:34	AZBC1pbd6hg5eedyw	delete_inside_static	{"kind":"inside_static","occluder":"terrain_rock_bc_17 (ref 0:237977 in cell -9,-2)"} [terrain_z=129.1 height=61 gap=5.71 min_gap=1.12 tilt_delta=0.0deg]
```

| Column | Holds |
|---|---|
| `cell` | The exterior cell's grid |
| `key` | The reference's master index and reference number in the plugin. `0` is the plugin itself |
| `id` | The object it places |
| `verdict` | One of the verdicts above |
| `detail` | For a fix: the new position `to=`, the new rotation `rot=` in radians, the gap before and after, and the static it moved away from. For anything else: the reason, as JSON. Then, in brackets, what was measured |

The bracketed measurements are the terrain height under the reference, the mesh's height, `gap`
(how far the base vertex standing highest above its terrain is above it; negative means buried),
`min_gap` (the same for the most buried base vertex), and how far its tilt is from the slope's. A
reference skipped before it was measured has none.

## JSON

`--structured` prints the report as one JSON object. Its keys:

| Key | Holds |
|---|---|
| `target` | The plugin |
| `added_data_directories` | Mod folders added to find its meshes. Absent when there are none |
| `mode` | `{"mode": "dry_run", "would_write": PATH}` or `{"mode": "write", "path": PATH}` |
| `policy` | `actions`, `float_tolerance`, `max_sink`, `max_sink_fraction`, `sink`, `orientation_epsilon_degrees`, `max_tilt_degrees`, `relocation_step`, `relocation_steps`, and the five pattern lists |
| `cells` | `target_exterior_refs`, `target_cells`, `active_cells`, `terrain_cells_loaded`, `terrain_cells_missing` |
| `occluders` | The counts behind the `Occluders` line, with `missing_meshes` listing each mesh that failed to load |
| `counts` | `total`, `keep`, `fix`, `delete`, `skip`, and `by_label`: every verdict and its count |
| `mesh_errors` | Each distinct error among `skip_mesh_error` references, with its count |
| `write` | After `--write`: `path`, `replaced_source`, `backups`, `refs_fixed`, `refs_deleted`, `cells`, `verified` |
| `refs` | With `--verbose`: every reference |

`target_exterior_refs` counts every exterior reference in the plugin, `total` only those the grass
filters let through. Each entry of `refs` is shaped like this:

```json
{
  "cell": [1, -3],
  "key": [0, 13],
  "id": "AZMA255yznw5i80xs",
  "verdict": {
    "verdict": "fix",
    "translation": [11400.58, -20338.023, 759.9275],
    "rotation": [-0.062177498, -0.062177498, 4.8583956],
    "grounded": false,
    "oriented": false,
    "moved": true,
    "gap_before": -3.5842896,
    "gap_after": -7.5842896,
    "moved_from": [11432.58, -20338.023],
    "occluder": "terrain_rock_ma_55 (ref 0:195533 in cell 1,-3)"
  },
  "measured": {
    "terrain_z": 740.67456,
    "mesh_height": 70.70999,
    "contact": { "gap": -3.5842896, "min_gap": -5.5234985, "base_points": 12 },
    "tilt_delta_degrees": 0.0
  }
}
```

`verdict.verdict` is `keep`, `fix`, `delete` or `skip`. A fix carries its placement, as above.
The others carry a `reason`, whose `kind` is the verdict label without its prefix, with what was
found: `{"kind": "road", "texture": "tx_ashlands_road_01.tga"}` for `delete_road`. Two differ:
plain `keep` is `{"kind": "fine"}`, and `keep_too_steep` is `too_steep_to_orient`.
