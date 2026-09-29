+++
title = "Unclip"
description = "What greenmote unclip measures, the order its rules run in, the six actions, how it writes and what it keeps, and how to tune it."
weight = 30

[extra]
kind = "guide"
+++

Unclip checks one groundcover plugin against the world your load order builds, one reference at a
time, and gives each reference exactly one verdict: keep it, fix it, delete it, or skip it with a
reason. It prints what it would do and changes nothing until you pass `--write`.

```sh
# Report only.
greenmote unclip --plugin Rem_AI.esp
# The same, and one log line per reference.
greenmote unclip --plugin Rem_AI.esp --verbose
# Rewrite it in place, with backups.
greenmote unclip --plugin Rem_AI.esp --write
# Write the rewritten plugin elsewhere and leave the original alone.
greenmote unclip --plugin Rem_AI.esp --write --output-plugin Rem_AI_unclipped.esp
```

## The target

`--plugin`, or `plugin` under `[unclip]` in `greenmote.toml`, names the plugin. A path to an
existing file is used as it is; anything else is looked up by name in your data folders and
archives, the way OpenMW finds a `groundcover=` file.

A plugin outside every data folder is usually a mod you have downloaded and not enabled. Its grass
meshes are somewhere near it, so unclip looks for them, starting at the plugin's folder and going up
to two levels above it. At the first level where that folder, or a folder directly inside it, holds
a `meshes` folder, those folders are added to the search. That finds Fantasia's `meshes` beside its
plugins, Aesthesia's above them, and Remiros' in a sibling option folder. A folder whose name
contains `OpenMW` wins over its other-engine twin. If one of those folders is already a `data=`
folder, nothing is added. The report says what it added:

```text
Added Fantasia to the VFS because the plugin is not in a configured data directory
```

Only exterior references are checked. The `include_grass_ids` and `exclude_grass_ids` filters
narrow them further: full-ID regular expressions, case-insensitive, where an exclusion beats an
inclusion and no inclusions means every reference. A reference the filters leave out is not
measured, counted or changed.

Greenmote's own `groundcover.omwaddon` is refused. Convert rebuilds it from your load order on
every run, so a fix written into it would not survive the next one; unclip the grass mods instead.

## The world it measures against

The **content files** of your `openmw.cfg`, loaded as OpenMW loads them, in the 3 by 3 cells
around every cell the target touches. Other `groundcover=` plugins are not part of that world:
grass does not block grass.

| Measured | How |
|---|---|
| Terrain height | From the `LAND` records of the load order, with OpenMW's alternating triangle split of each 128-unit square, not a smooth blend |
| Terrain texture | The `LAND` texture under the reference, sampled where OpenMW draws it, including the engine's blend-map offset |
| Water | The exterior water plane, at height 0 |
| Statics | Every static reference in those cells, as the load order finally places it: a plugin that moves or deletes a master's rock is honoured, and no copy remains at the old spot |
| Static collision | OpenMW's own rules: a mesh's `RootCollisionNode` when it has one, otherwise its visible geometry, with `NCO`, `NCC` and `MRK` extra data honoured. Overlap is tested against the collision triangles themselves |
| The reference | Its mesh's base: every vertex within 5% of the mesh's height (at least 1 unit) of the lowest one, after the reference's rotation and scale. And its visible bounding box |

Rotations are composed the way OpenMW composes them: about Z, then Y, then X, in the world frame.

Only statics block grass. Activators, containers, doors, lights and actors do not, and neither do
the tree-like statics the default `exclude_occluder_ids` lists, whose canopies would claim the
grass beneath them. A static whose mesh cannot be loaded, or that has no collision, such as
an editor marker, blocks nothing; the report lists the meshes that failed to load, because
clipping into those statics goes unnoticed.

## The rules, in order

Each reference runs through these in order, and the first that decides it wins.

1. **Skip** a reference already deleted, one whose ID is not a static in the load order or the
   target, and one whose mesh does not load.
2. **Delete** a reference whose position lies outside the cell record that holds it. OpenMW draws
   such a reference only from a full cell away or more and drops it up close, so it pops in and
   out. This is not an action and cannot be turned off.
3. **`road-delete`**: delete a reference standing on a road texture.
4. **Skip** a reference with no terrain under its base.
5. **`orient`**, then **`terrain-z`**: work out the tilt and height the reference should have.
6. **`water-delete`**: delete a reference placed at or above the water plane whose terrain now
   lies below it.
7. **`static-delete`** and **`static-move`**: deal with a reference whose bounding box reaches
   into a static.

What is left changes if orient, terrain-z or static-move changed it, and is kept otherwise. A
turned-off action never changes a verdict silently: the reference is kept with a reason that
names it, such as `keep_terrain_z_disabled`. [The report](@/docs/unclip-output.md#verdicts) lists
every verdict.

## The actions

All six run unless `--actions` or `actions` in `greenmote.toml` lists fewer.

### terrain-z

Measures the gap between each base vertex and the terrain under it, and takes the vertex standing
highest above its terrain.

- More than `float_tolerance` (1 unit) above the terrain: the reference floats.
- Below the terrain by more than the larger of `max_sink` (24 units) and `max_sink_fraction` (0.75)
  of the mesh's height: the reference is buried.

Either way it is moved up or down so that vertex sits `sink` (4 units) below the surface. Floating
is the visible fault; burial only counts when most of the plant is underground, because grass
generators bury tall grass on purpose: Remiros by 4 to 16 units, Fantasia by up to 48.

### orient

Computes the tilt the terrain gives the reference with the formula grass generators use, and
applies it when it differs from the current tilt by more than `orientation_epsilon` (1 degree). A
plugin made by such a generator therefore measures as already aligned.

Ground steeper than `max_tilt` (45 degrees) is never oriented to: the reference keeps its rotation
(`keep_too_steep`). Where two landmasses meet, such as a Tamriel Rebuilt coast beside a sea-floor
mod, the generator's slope reads as a near-vertical wall, and grass laid against it would hang off
the edge.

### Terrain seams

Before either fix is applied, unclip checks that the ground is continuous. If the terrain under
the mesh's base varies by more than the larger of the mesh's height and 256 units (two land
vertices), or grounding would put the reference's origin that far from the terrain beneath it, the
spot straddles a seam between two `LAND` records. Nothing sensible can be measured there,
so the reference is kept exactly as it was (`keep_terrain_seam`).

### road-delete

Deletes references whose terrain texture matches a pattern in `road_texture_paths`. The texture
path is the `LTEX` file name, lowercase, with forward slashes, such as `tx_ai_dirtroad_01.tga`;
a pattern must match all of it. The built-in patterns cover vanilla, Tamriel Data and several mods'
roads, cobbles and streets; `--road-texture-path` adds to them and `--no-default-road-textures`
starts from none.

Real grass plugins lose a lot to this: 6,609 of the 91,759 references in Remiros' Ascadian Isles
plugin, 7%, against a vanilla load order, because it was generated without road exclusion.

### water-delete

Deletes references that were placed at or above the water plane but whose terrain now lies below
it: land grass over ground that your load order has sunk. A reference that started under water,
such as kelp, is never deleted for water; the other actions handle it like any other.

### static-move

A reference whose bounding box overlaps a static's collision triangles is moved to the nearest
clear spot. Unclip searches outward in rings, `relocation_step` (32 units) apart, up to
`relocation_steps` (8) rings, trying eight directions on each, the ones pointing away from the
blocking static first. A spot qualifies when it is in the same cell, is not on a road, does not
cross the water plane (unless the reference started under water), is not on a terrain seam, and
is clear of every static. The moved reference is tilted and grounded at its new spot. It never
leaves its cell: OpenMW's groundcover loader drops a reference whose position lies outside its
cell.

### static-delete

Deletes a reference whose bounding box is entirely inside a static: every corner of it enclosed by
the static's triangles, tested by casting rays and counting crossings. Grass inside a closed rock
is found; grass under a mushroom tree's cap is not inside anything. It also deletes a reference
that overlaps a static and cannot be moved clear, or that static-move is turned off for.

## Writing

Without `--write`, nothing is written but the log.

`--write` rewrites the plugin in place:

1. The first time, the plugin is copied to `<plugin>.greenmote-original`. Later runs never touch
   that copy, so it is always the plugin as you downloaded it.
2. Every time, the current plugin is copied to `<plugin>.bak`.
3. The rewritten plugin is written beside it and moved over the original in one step.
4. It is read back and every planned change is checked. The report says `verified`.

A plugin with nothing to change is left alone.

`--write --output-plugin PATH` writes the complete rewritten plugin to `PATH` instead, creating
its folder, and leaves the source untouched. It has the same masters, so you can compare the two
in game by switching one `groundcover=` line. The copy is written even when nothing changed. If
`PATH` exists, it is first copied to `PATH.bak`. `--output-plugin` without `--write` writes
nothing.

Deleted references the plugin created itself are removed. References it inherited from a master
are kept and marked deleted, so the master's copy stays hidden.

Unclip run again on a plugin it has fixed plans nothing. On Fantasia's Bitter Coast plugin, against
a vanilla load order, the first run fixes 42,223 references and deletes 3,136; the second finds
116,144 fine and keeps 80 as too steep.

## Tuning

The defaults were calibrated on Remiros' Groundcover, Fantasia and Aesthesia. Change them in
[greenmote.toml](@/docs/configuration.md#unclip) or per run on the [command
line](@/docs/cli.md#unclip).

| Setting | Default | Raise it to | Lower it to |
|---|---:|---|---|
| `float_tolerance` | 1 | Leave slightly floating grass alone | Ground more of it |
| `max_sink` | 24 | Leave deeper-buried short grass alone | Raise more of it |
| `max_sink_fraction` | 0.75 | Leave deeper-buried tall plants alone | Raise more of them |
| `sink` | 4 | Bury fixed grass deeper | Leave it closer to the surface |
| `orientation_epsilon` | 1 | Leave slightly off tilts alone | Re-tilt more |
| `max_tilt` | 45 | Tilt grass on steeper ground | Keep more rotations |
| `relocation_step` | 32 | Search in coarser steps | Search finer |
| `relocation_steps` | 8 | Search farther before deleting | Delete sooner |

`sink` may not exceed `max_sink`. `relocation_steps` is 1 to 256; `max_tilt` is above 0 and at most
90; `max_sink_fraction` is 0 to 1.

If a static is blocking grass it should not, exclude it by ID with `exclude_occluder_ids` or
`--exclude-occluder-id`. Aesthesia places rock-textured decorations (`grs_rm_grayrock*`) across Red
Mountain; tens of thousands of them sit inside closed rock formations, and deleting them is
correct.

To see why a reference got its verdict, run with `--verbose` and look it up in
`greenmote-unclip.log`. [The report](@/docs/unclip-output.md) explains every field. Set
`GREENMOTE_PROFILE=1` to print how long each phase took to standard error.
