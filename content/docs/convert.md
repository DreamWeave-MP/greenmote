+++
title = "Convert"
description = "What greenmote convert reads, which statics it matches and in which order, the two plugins and the meshes it writes, and how to enable them."
weight = 20

[extra]
kind = "guide"
+++

Convert turns grass that your content files place as ordinary objects into one groundcover
plugin, and writes a second plugin that hides the originals. It never edits a plugin it reads.

```sh
# Print the plan, write nothing.
greenmote convert --dry-run
# Write the plugins and copy the meshes.
greenmote convert
# The same, and add the plugins to openmw.cfg.
greenmote convert --auto-enable
```

## What it reads

The load order in your `openmw.cfg`: every `content=` file, in order, found through the `data=`
folders and `fallback-archive=` archives exactly as OpenMW finds it. Files it cannot find are
left out, as are files that are not `.esm`, `.esp`, `.omwaddon` or `.omwgame`. `groundcover=`
plugins are not content and are not read.

Two kinds of plugin are skipped on purpose:

- Greenmote's own output. Its plugins carry `greenmote` as their author and Greenmote's own
  description, and a run skips them so that converting twice never converts the conversion. The
  plan counts them as `skipped generated plugins`.
- Plugins you ignore: any whose file name matches a regular expression in `ignored_plugins` or
  `--ignore`. The match is case-insensitive and can fall anywhere in the name, so
  `--ignore '^Rem_'` skips every Remiros plugin and `--ignore tamriel` skips anything with
  "tamriel" in it.

A plugin that cannot be parsed is reported as `[ WARNING ]: Plugin ... could not be loaded` and
left out; the run goes on.

## What it matches

A record is grass when its ID matches one of the `grass_ids` patterns and none of the `exclude`
patterns. Both are regular expressions, case-insensitive, and match anywhere in the ID: `fern`
matches `flora_bc_fern_02`. The defaults cover vanilla, Tamriel Data and common mod grass:

| `grass_ids` | `exclude` |
|---|---|
| `grass`, `kelp`, `lilypad`, `fern`, `thirrlily`, `spartium`, `in_cave_plant`, `reedgroup`, `t_mw_floratv_treezifa`, `t_mw_florash_bush`, `t_mw_floraow_varga`, `t_cyr_floragc_shrub`, `t_cyr_floragc_bush_02`, `t_glb_flora_cattails`, `t_cyr_florastr_shrub`, `flora_bm_shrub` | `refernce`, `infernace`, `planter`, `_furn_`, `_skelp`, `t_glb_var_skeleton`, `cliffgrass`, `terr`, `grassplane`, `flora_s_m_10_grass`, `cave_mud_rocks_fern`, `ab_in_cavemold`, `rp_mh_rock`, `ex_cave_grass00`, `secret_fern`, `flora_grass_entrance`, `^ash_flora_bc_fern_[0-9]+s$` |

The last exclusion is Verdant Bitter Coast's fern trees, which only match because `fern` is a
grass ID. [greenmote.toml](@/docs/configuration.md#convert) is where you change either list.

Only two record types are candidates:

- **Statics** (`STAT`).
- **Activators without a script** (`ACTI`) that have a mesh. A script means the object does
  something, which groundcover cannot.

Each ID is decided by its newest definition: the load order is read from the last plugin back,
and the first plugin that defines an ID owns it. If that definition is deleted, or is an activator
with a script, the ID is not converted, whatever older plugins say.

## What it converts

Only references in **exterior** cells. OpenMW does not draw groundcover in interiors, so an
interior plant stays exactly as it is.

Every exterior reference to a matched ID is converted, in every plugin that places one. Where
several plugins carry the same cell, the newest plugin's copy of each reference is kept, in one
cell record per grid. OpenMW's groundcover loader would otherwise keep the oldest copy.

A matched ID that no exterior cell uses is counted in `matched records` but not in
`used records`, and writes nothing.

## What it writes

| File | Where | What |
|---|---|---|
| `groundcover.omwaddon` | The output folder | One new static per matched ID, named `gm_` and 16 hex digits, and every converted reference pointing at it |
| `deleted_groundcover.omwaddon` | The output folder | The same references, marked deleted, so the originals vanish |
| `Meshes/grass/...` | The output folder | Every mesh a converted static uses |
| `greenmote.log` | Next to your user `openmw.cfg` | The plan, as `--dry-run` prints it |
| `greenmote.toml` | Next to your user `openmw.cfg` | Your settings, created with the defaults by the first run that is not a dry run |

Each new static is always a `STAT`, even when the original was an activator. Its ID is derived
from the original ID and the plugin that defined it, so the same load order gives the same IDs
every time. Its mesh is the original's, moved under `grass\`: `f\Flora_kelp_01.nif` becomes
`grass\f\Flora_kelp_01.nif`. A mesh already under `grass\` stays where it is.

Both plugins list as masters only the plugins whose references they carry. A plugin that only
defines a matched static does not become a master for that. A master list longer than 255, the
most a reference can address, is an error.

### Meshes

Meshes are found the way OpenMW finds them, through the `data=` folders, later folders winning,
and the `fallback-archive=` archives. They are copied to `Meshes/grass/` in the output folder with
lowercase paths, keeping their subfolders.

Every mesh is found before either plugin is written. A mesh that cannot be found stops the run
with an error and leaves no plugin behind that would point at nothing. A mesh path with an empty,
`.` or `..` component, or a drive letter, is refused.

### The output folder

| When | Output folder |
|---|---|
| `--output DIR` | `DIR`, for that run |
| `openmw.cfg` sets `data-local` | `data-local` |
| Otherwise | The folder you ran Greenmote from |

The last is rarely what you want: OpenMW only loads the plugins from a folder it reads. The run
ends by saying whether the folder is visible to OpenMW, and what to add if it is not.

## Enabling the output

Two lines in `openmw.cfg`:

```ini
groundcover=groundcover.omwaddon
content=deleted_groundcover.omwaddon
```

`deleted_groundcover.omwaddon` must load after every plugin whose references it hides; the end of
the load order is right. And OpenMW draws groundcover only with `enabled = true` under
`[Groundcover]` in `settings.cfg`.

`--auto-enable`, or **Auto-enable generated plugins** in the GUI, adds whichever of the two lines
is missing to your user `openmw.cfg`, after copying it to `openmw.cfg.greenmote.bak`. It refuses,
before anything is generated, unless the output folder is `data-local` or one of the `data=`
folders, because enabling plugins OpenMW cannot find would break the load order:

```text
error: refusing to auto-enable outputs in /home/you/grass because it is not data-local or a configured data directory
```

When both lines are already there, the cfg is not touched:
`OpenMW config already enables generated plugins; no update needed.`

## Running it again

Each run rebuilds both plugins from the current load order, overwriting the previous ones, and
copies the meshes again. Run it again after adding, removing or reordering content, and after
changing your patterns.

## The plan

`--dry-run` prints the plan and writes nothing, not even `greenmote.toml`. A vanilla install with
both expansions:

```text
# greenmote convert 0.3.0
# output directory: /home/you/.local/share/openmw/data
# groundcover output: /home/you/.local/share/openmw/data/groundcover.omwaddon
# deleted output: /home/you/.local/share/openmw/data/deleted_groundcover.omwaddon
# content files: 3
# loaded plugins: 3
# skipped generated plugins: 0
# matched records: 33
# used records: 30
# changed cells: 913
# touched refs: 30916
# meshes to copy: 30
STAT "Flora_kelp_01" from "Morrowind.esm": generated STAT "gm_2f9d3f0ba8ea6550"; mesh "f\\Flora_kelp_01.nif" -> "grass\\f\\Flora_kelp_01.nif"
...
CELL refs from "Morrowind.esm": 29761 refs in 846 exterior cells
CELL refs from "Bloodmoon.esm": 1155 refs in 67 exterior cells
MESH "f\\flora_kelp_01.nif" -> /home/you/.local/share/openmw/data/Meshes/grass/f/flora_kelp_01.nif
...
```

| Line | Counts |
|---|---|
| `content files` | `content=` lines in `openmw.cfg` |
| `loaded plugins` | Content files that were read: found, not ignored, and parsed |
| `skipped generated plugins` | Greenmote's own plugins among them |
| `matched records` | Statics and activators whose newest definition is grass |
| `used records` | Those that at least one exterior reference uses |
| `changed cells` | Exterior cell records that contribute references |
| `touched refs` | References converted |
| `meshes to copy` | Distinct meshes |

A real run writes the same plan to `greenmote.log`. `--debug` also prints it to standard error as
the run goes.
