# Greenmote

Greenmote is a Rust 2024 tool for Morrowind and `OpenMW` groundcover workflows. It provides:

- `greenmote convert`, a converter that turns vanilla-style placed static references into `OpenMW` groundcover plugins.
- `greenmote unclip`, an inspector and optional fixer for generated groundcover references that clip into terrain or static occluders.
- A desktop GUI, enabled by default, that exposes Convert, Unclip, and shared settings without requiring command-line use.

## Quick Start

### GUI

Launch the default application:

```sh
greenmote
```

Launching Greenmote without arguments opens the GUI when the default `gui` feature is enabled.

Typical GUI flow:

1. Open Greenmote.
2. Review or regenerate settings if prompted.
3. Use the Convert tab to run a dry run or conversion.
4. Use the Unclip tab to add groundcover plugins (file picker or drag and drop) and press Fix plugins. They are rewritten in place after a confirmation; the untouched original is kept beside each one.
5. Untick "Write changes" to get a report only.

### CLI

Run Convert with discovered `OpenMW` settings:

```sh
greenmote convert
```

Run Convert without writing output:

```sh
greenmote convert --dry-run
```

Inspect generated groundcover clipping:

```sh
greenmote unclip --plugin groundcover.omwaddon --verbose
```

Rewrite the plugin in place (with backups) after inspecting the report:

```sh
greenmote unclip --plugin groundcover.omwaddon --write
```

Top-level options such as `--openmw-cfg`, `--config`, `--generate-completion`, and `--generate-manpage` must appear before the subcommand.

## Installation And Builds

Build from the repository root:

```sh
cargo build --release
```

Install from the checked-out source tree:

```sh
cargo install --path .
```

Build a CLI-only binary without GUI dependencies:

```sh
cargo build --release --no-default-features
```

The binary is written under `target/release/greenmote` unless you use Cargo installation or packaging commands.

## `OpenMW` Configuration Discovery

Greenmote reads `OpenMW` configuration so it can discover configured data directories, enabled content files, and the effective `data-local` location.

- `--openmw-cfg PATH` points Greenmote at a specific `openmw.cfg` file or at a directory containing `openmw.cfg`.
- Without `--openmw-cfg`, Greenmote uses the platform default `OpenMW` user configuration discovery provided by `openmw-config`.
- `greenmote.toml` defaults to the `OpenMW` user config directory, next to the discovered user `openmw.cfg` location.
- `--config PATH` overrides the `greenmote.toml` path.
- Convert output defaults to the effective `OpenMW` `data-local` directory.
- If `OpenMW` does not provide `data-local`, Greenmote falls back to `OpenMW`'s default data-local path helper.
- CLI `convert --output PATH` overrides `[convert].output_directory` behavior for that run.

Generated defaults are meant to be editable. Greenmote validates regular expressions and numeric policy values before using the file.

## Convert Workflow

`greenmote convert` creates `OpenMW` groundcover output from matching `STAT` records, scriptless `ACTI` records, and exterior cell references.

Important behavior:

- Matching is record-ID-based. Greenmote searches `STAT` IDs and scriptless `ACTI` IDs using configured include and exclude regular expressions.
- Scripted activators are not converted. A script means gameplay behavior, not decorative groundcover.
- Generated placed records are always `STAT`, even when the source record was a scriptless `ACTI`.
- Only exterior `CELL` records are scanned. Interiors are intentionally excluded.
- Matching definitions are selected in reverse load order, so later content wins.
- Generated output includes only references touched by matching source records.
- Definition-only source plugins do not become masters merely because their records were copied.
- Missing meshes are fatal before plugin writes, so Greenmote should not leave broken output plugins after discovering a missing mesh.

Generated files:

- `groundcover.omwaddon`, the generated groundcover plugin.
- `deleted_groundcover.omwaddon`, a companion plugin that deletes the original placed static references.
- `greenmote.log`, a run summary and diagnostic log.
- `greenmote.toml`, created in the `OpenMW` user config directory when missing and when the command is not a dry run or config-only validation.

Mesh behavior:

- Source meshes are resolved through the `OpenMW` data directory VFS.
- Copied meshes are written under `Meshes/grass/...` in the output directory.
- Mesh lookup paths are normalized to lowercase backslash paths.
- Unsafe path components such as `..`, `.`, empty components, and drive-prefixed components are rejected.

Useful Convert flags:

- `--dry-run[=BOOL]` builds and prints the conversion plan without writing files.
- `--validate-config[=BOOL]` validates `greenmote.toml` and regex settings without loading plugins or writing output.
- `--debug` prints extra diagnostics.
- `--auto-enable` adds generated plugins to `openmw.cfg` after successful generation.
- `--ignore REGEX[,REGEX...]` ignores matching plugin file names for conversion planning.
- `--output PATH` writes generated plugins and meshes to a specific output directory.

`--auto-enable` only edits `openmw.cfg` when the output directory is visible to `OpenMW` as `data-local` or as a configured `data=` directory.

## Unclip Workflow

`greenmote unclip` measures every reference in a groundcover plugin against the terrain, water, road textures, and solid statics of your `OpenMW` load order, then reports one verdict per reference. It never writes unless you pass `--write`.

```sh
greenmote unclip --plugin Rem_AI.esp                 # report only
greenmote unclip --plugin Rem_AI.esp --verbose       # plus a per-reference table in greenmote-unclip.log
greenmote unclip --plugin Rem_AI.esp --write         # rewrite Rem_AI.esp in place, keeping backups
greenmote unclip --plugin Rem_AI.esp --write --output-plugin Rem_AI_unclipped.esp   # write the rewritten plugin elsewhere
```

How a reference is judged, in order:

1. References that are deleted, have no `STAT` record, have an unloadable mesh, or have no terrain under them are skipped with that reason.
2. References whose position lies outside the cell record that holds them are always deleted. `OpenMW` draws such a reference only in chunks a full cell or larger and drops it in the near, sub-cell chunks, so it pops in from afar and vanishes as you approach.
3. `road-delete` deletes references whose `LAND` texture matches a road pattern. The texture is sampled where `OpenMW` renders it, including the engine's blendmap offset.
4. `water-delete` deletes references standing on ground below the exterior water plane.
5. `orient` tilts the reference to the terrain slope using the same formula groundcover generators use, so a generator's own output measures as already aligned.
6. `terrain-z` looks at the mesh's base vertices under the final tilt. If the highest base vertex floats more than `--float-tolerance` above the terrain, or is buried deeper than the larger of `--max-sink` units and `--max-sink-fraction` of the mesh height, the reference is lowered or raised so that vertex sits `--sink` units below the surface. Floating is the visible defect; burial only counts when most of the plant is underground, because generators bury tall grass deliberately (Remiros by 4 to 16 units, Fantasia by up to 48).
7. `static-move` moves references whose visible volume overlaps a static's collision shape to the nearest clear spot in the same cell, re-grounding them there. `static-delete` removes references that are entirely inside a static, or that overlap one and cannot be moved.

A disabled action never changes a verdict silently: the report says why a reference was left alone, for example `keep_terrain_z_disabled`.

Geometry facts the measurements rely on:

- Reference rotations are composed the way `OpenMW` composes them (`Misc::Convert::makeOsgQuat`): about Z, then Y, then X, in the world frame.
- Terrain heights use `OpenMW`'s alternating triangle split of each 128-unit quad, not bilinear interpolation.
- Static collision follows `OpenMW`'s Bullet loader: `RootCollisionNode` shapes when present, otherwise the visible geometry, with `NCO`/`NCC`/`MRK` extra data honoured. Overlap is tested against the actual collision triangles, and "inside" means every corner of the grass volume is enclosed by the mesh (ray parity), so a mushroom tree's cap does not block the ground under it and grass inside a closed rock is recognised.
- Static references are resolved by their load-order identity, so a plugin that moves or deletes a master's rock is honoured and no stale copy remains at the old position.

Output:

- `--write` rewrites the plugin in place. The first write keeps `<plugin>.greenmote-original`; every write refreshes `<plugin>.bak`. References the plugin inherited from a master are marked deleted rather than dropped so the master's placement stays hidden.
- `--output-plugin PATH` writes the complete rewritten plugin to `PATH` instead and leaves the source untouched. The copy is a drop-in replacement with the same masters, so you can A/B by swapping one `groundcover=` line.
- Every write is reloaded and checked against the planned changes before unclip reports success. Running unclip again on its own output plans zero changes.
- `greenmote-unclip.log` next to `openmw.cfg` holds the text report and, with `--verbose`, one line per reference.
- `--structured` prints the report as JSON; with `--verbose` it includes every verdict.
- Set `GREENMOTE_PROFILE=1` to print phase timings to stderr.

Flags and `[unclip]` keys:

- `--actions ACTION[,ACTION...]` limits the actions to plan. Default: all six.
- `--float-tolerance` (1), `--max-sink` (24), `--max-sink-fraction` (0.75), `--sink` (4), `--orientation-epsilon` degrees (1), `--relocation-step` (32), `--relocation-steps` (8).
- `--include-grass-id` / `--exclude-grass-id` select target references by full ID regex.
- `--include-occluder-id` / `--exclude-occluder-id` select statics that count as solid. Built-in exclusions cover tree-like statics; `--no-default-occluder-excludes` drops them.
- `--road-texture-path` adds road texture regexes to the built-in list; `--no-default-road-textures` drops the built-ins.
- `--ignore-missing-meshes` continues when a static's mesh cannot be loaded. Without it unclip stops, because clipping into those statics could not be detected.
- Unknown `[unclip]` keys are reported as warnings and dropped when the GUI saves settings.
## GUI Features

The default GUI provides:

- A Convert main view with run controls, progress, status output, and generated-output safeguards.
- An Unclip main view: a plugin list with drag and drop, one write toggle (on by default), and a confirmed in-place rewrite.
- Settings sections for OpenMW/config paths, Convert options, Unclip options, filters, road texture filters, and write policy.
- Runtime-only localization for English, Swedish, Russian, Spanish, German, and French.
- Non-persistent language selection. Changing the GUI language affects the current GUI session only.

Localization applies to runtime GUI text only. CLI output, structured Unclip output, generated reports, and `greenmote.log` remain English-only.

## `greenmote.toml`

Example configuration:

```toml
[convert]
grass_ids = [
  "grass",
  "kelp",
  "lilypad",
  "fern",
]
exclude = [
  "planter",
  "_furn_",
  "terr",
]
ignored_plugins = [
  "^example-disabled-plugin\\.esp$",
]
debug = false
auto_enable = false

[unclip]
plugin = "groundcover.omwaddon"
actions = ["terrain-z", "water-delete", "road-delete", "static-delete", "static-move", "orient"]
float_tolerance = 1.0
max_sink = 24.0
max_sink_fraction = 0.75
sink = 4.0
relocation_step = 32.0
relocation_steps = 8
orientation_epsilon = 1.0
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

Key notes:

- `[convert].grass_ids` is the include list for `STAT` IDs and scriptless `ACTI` IDs.
- `[convert].exclude` removes matching source record IDs from conversion.
- `[convert].ignored_plugins` removes matching plugin file names from conversion.
- `[convert].debug` and `[convert].auto_enable` persist their corresponding Convert toggles. Convert dry-run is runtime-only via `--dry-run`.
- `[unclip].plugin` is the default Unclip target plugin.
- Unclip `--write`, `--output-plugin`, `--verbose`, and `--structured` are runtime-only CLI options and are not read from or written to `greenmote.toml`.
- `[unclip].actions` selects which fixes are planned.
- `[unclip].float_tolerance`, `max_sink`, `max_sink_fraction`, `sink`, `orientation_epsilon`, `relocation_step`, and `relocation_steps` tune the policy.
- Include/exclude ID filters are case-insensitive regex lists.
- Road texture path filters are case-insensitive regex lists used by the `road-delete` write action.
- Default occluder excludes skip common vanilla, Bloodmoon, and `Tamriel_Data` tree statics whose broad canopy bounds often produce false static-occlusion hits. Set `exclude_occluder_ids = []` to opt back into treating them as blockers.
- Unknown TOML keys are ignored for stale-config compatibility.

## Generated Shell Completions And Manpage

Greenmote can generate top-level shell completions and a roff manpage to standard output:

```sh
greenmote --generate-completion bash > greenmote.bash
greenmote --generate-completion zsh > _greenmote
greenmote --generate-completion fish > greenmote.fish
greenmote --generate-manpage > greenmote.1
```

These flags are top-level application flags. Place them before any subcommand and do not combine them with `convert` or `unclip`.

## Known Limitations And Release Notes

- BSA-backed mesh lookup is incomplete until archive handling is wired to VFS fallback archives.
- Convert intentionally processes exterior `CELL` records only; interiors are not supported.
- `--auto-enable` requires the output directory to be visible to `OpenMW` as `data-local` or a configured `data=` directory.

## Troubleshooting

`unclip requires --plugin or [unclip].plugin in greenmote.toml`

Pass `greenmote unclip --plugin groundcover.omwaddon` or set `[unclip].plugin`.

`config file ... does not exist`

`convert --validate-config` requires an existing config file. Run Convert normally once, use the GUI settings regeneration, or create the file manually.

Missing mesh errors

Verify the source plugin load order, `OpenMW` `data=` directories, loose mesh files, and BSA availability. BSA-backed lookup has the limitation noted above.

`--auto-enable` refuses to update `OpenMW` config

Write output to `OpenMW` `data-local` or to a directory already present as `data=` in `openmw.cfg`.

No matching statics

Review `[convert].grass_ids`, `[convert].exclude`, `--ignore`, and the active `OpenMW` content list.

Unexpected Unclip changes

Run without `--write` first, add `--verbose`, and inspect `greenmote-unclip.log` for one line per reference with the measured gap, tilt, and the reason for its verdict.

## Development And Validation

Before handoff, run:

```sh
cargo fmt
cargo test -p greenmote
cargo test --workspace
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -W clippy::pedantic -D warnings
```

Documentation validation:

```sh
cargo doc --workspace --all-features --no-deps
```

## License

Greenmote is licensed under GPL-3.0-only. See [LICENSE](LICENSE).
