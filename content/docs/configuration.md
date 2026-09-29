+++
title = "greenmote.toml"
description = "Where Greenmote keeps its settings, every key under [convert] and [unclip] with its default, and how the command line overrides each one."
weight = 50

[extra]
kind = "reference"
+++

`greenmote.toml` holds the settings convert and unclip use on every run, and what the GUI's
Settings tab edits. The command line overrides it for one run and never writes to it.

## Where it is

Next to your user `openmw.cfg`: the folder of the last `openmw.cfg` in the chain OpenMW reads,
such as `~/.config/openmw/greenmote.toml` on Linux. `--config PATH` uses another file:

```sh
greenmote --config ~/grass/strict.toml unclip --plugin Rem_AI.esp
```

`--config`, like `--openmw-cfg`, goes before the subcommand.

The first convert that is not a dry run creates the file with every default written out, and so
does opening the GUI. Unclip reads it but never creates it; without it, unclip uses the defaults.

A file that is not valid TOML stops the command line with the parser's error. The GUI offers to
move it aside, as `greenmote.toml.bak` (or `.bak.1`, `.bak.2` when that exists), and write a fresh
one.

## The file as generated

```toml
[convert]
grass_ids = [
    "grass",
    "kelp",
    "lilypad",
    "fern",
    "thirrlily",
    "spartium",
    "in_cave_plant",
    "reedgroup",
    "t_mw_floratv_treezifa",
    "t_mw_florash_bush",
    "t_mw_floraow_varga",
    "t_cyr_floragc_shrub",
    "t_cyr_floragc_bush_02",
    "t_glb_flora_cattails",
    "t_cyr_florastr_shrub",
    "flora_bm_shrub",
]
exclude = [
    "refernce",
    "infernace",
    "planter",
    "_furn_",
    "_skelp",
    "t_glb_var_skeleton",
    "cliffgrass",
    "terr",
    "grassplane",
    "flora_s_m_10_grass",
    "cave_mud_rocks_fern",
    "ab_in_cavemold",
    "rp_mh_rock",
    "ex_cave_grass00",
    "secret_fern",
    "flora_grass_entrance",
    "^ash_flora_bc_fern_[0-9]+s$",
]
ignored_plugins = []
debug = false
auto_enable = false

[unclip]
actions = [
    "terrain-z",
    "water-delete",
    "road-delete",
    "static-delete",
    "static-move",
    "orient",
]
float_tolerance = 1.0
max_sink = 24.0
max_sink_fraction = 0.75
sink = 4.0
relocation_step = 32.0
relocation_steps = 8
orientation_epsilon = 1.0
max_tilt = 45.0
include_grass_ids = []
exclude_grass_ids = []
include_occluder_ids = []
exclude_occluder_ids = [
    "flora_(tree|ashtree|treestump|treedead|root)_.*",
    "flora_(ash_)?log_.*",
    "flora_bm_(treebranch|treestump|snowbranch|snowstump|(snow_)?log)_.*",
    "flora_bc_(tree|knee|log)_.*",
    "ex_t_(bigroot|root).*",
    "t_.*flora.*(tree|branch|root|stump|log|palm).*",
    "t_cyr_flora(gc|str)_bush_.*",
]
road_texture_paths = [
    ".*(road|mainroad|dirtroad|gravelroad|beatenpath).*",
    ".*(cobble|cobblestone).*",
    ".*(street|whiteroad).*",
    ".*t_.*_terrroad.*",
    ".*t_imp_highway_txroad.*",
    ".*t_hr_.*road.*",
    ".*t_ham_.*road.*",
    ".*tx_sky.*road.*",
    ".*tr_alm_street.*",
    ".*nec_whiteroad.*",
]
```

Every key is optional. A list that is left out means the built-in list; an empty list, `[]`, means
no patterns at all. Patterns are regular expressions in [Rust's
syntax](https://docs.rs/regex/latest/regex/#syntax), always case-insensitive. In TOML, a backslash
in a double-quoted string is written `\\`.

## [convert]

| Key | Default | Meaning |
|---|---|---|
| `grass_ids` | The list above | A static or activator whose ID matches any of these is grass. Matches anywhere in the ID |
| `exclude` | The list above | An ID that matches any of these is not, whatever `grass_ids` says. Matches anywhere in the ID |
| `ignored_plugins` | `[]` | Content files whose file name matches any of these are not read. Matches anywhere in the name |
| `debug` | `false` | Print the plan to standard error during a real run |
| `auto_enable` | `false` | Add the output to `openmw.cfg`. See [enabling the output](@/docs/convert.md#enabling-the-output) |

Unknown keys under `[convert]`, and keys outside both tables, are ignored without a word. The
output folder is not a setting: it is `data-local`, or `--output` for one run.

| Command line | Against the file |
|---|---|
| `--ignore REGEX[,REGEX...]` | Added to `ignored_plugins` for this run |
| `--debug`, `--auto-enable` | Turn the setting on for this run. They cannot turn it off |
| `--output DIR` | Replaces the output folder for this run |
| `--dry-run`, `--validate-config` | For this run only; never saved |

## [unclip]

| Key | Default | Meaning |
|---|---|---|
| `plugin` | none | The plugin to check when `--plugin` is not given |
| `actions` | all six | Which of `terrain-z`, `water-delete`, `road-delete`, `static-delete`, `static-move` and `orient` to plan. `write_actions` is read as the same key |
| `float_tolerance` | `1.0` | Units the mesh base may float above the terrain |
| `max_sink` | `24.0` | Units the mesh base may be buried |
| `max_sink_fraction` | `0.75` | The same, as a fraction of the mesh's height, 0 to 1. The larger limit applies |
| `sink` | `4.0` | Units below the surface a fixed mesh base is placed. At most `max_sink` |
| `orientation_epsilon` | `1.0` | Degrees a tilt may differ from the slope's before it is changed |
| `max_tilt` | `45.0` | Steepest ground, in degrees, grass is tilted to. Above 0, at most 90 |
| `relocation_step` | `32.0` | Units between the rings static-move searches. Above 0 |
| `relocation_steps` | `8` | How many rings it searches, 1 to 256 |
| `include_grass_ids` | `[]` | Only references whose ID matches one of these are checked. Empty means all |
| `exclude_grass_ids` | `[]` | References whose ID matches one of these are not checked. Beats an inclusion |
| `include_occluder_ids` | `[]` | Only statics whose ID matches one of these block grass. Empty means all |
| `exclude_occluder_ids` | Tree-like statics, above | Statics whose ID matches one of these never block grass |
| `road_texture_paths` | Road textures, above | Terrain texture paths road-delete deletes on |

The ID and texture patterns under `[unclip]` must match the whole ID or path, unlike the patterns
under `[convert]`: `flora_grass_.*` matches `flora_grass_01`, `grass` alone does not. Texture paths
are the `LTEX` file name in lowercase with forward slashes, such as `tx_ai_dirtroad_01.tga`.

Every number must be finite and not negative. A value out of range, or a pattern that is not a
valid regular expression, stops the run and names the key. Unknown keys under `[unclip]`, such as
settings from older versions, are reported and ignored:

```text
warning: ignoring unknown [unclip] keys in /home/you/.config/openmw/greenmote.toml: dry_run, origin_epsilon
```

The GUI drops them the next time it saves.

| Command line | Against the file |
|---|---|
| `--plugin`, and each number | Replace the setting for this run |
| `--actions` | Replaces `actions` for this run |
| `--include-grass-id`, `--exclude-grass-id`, `--include-occluder-id` | Replace that list for this run. Repeat the option for several |
| `--exclude-occluder-id`, `--road-texture-path` | Added to that list for this run |
| `--no-default-occluder-excludes`, `--no-default-road-textures` | Start that list empty for this run, without the file's patterns or the built-in ones; the options above still add to it |
| `--write`, `--output-plugin`, `--verbose`, `--structured` | For this run only; there is no setting for them |
