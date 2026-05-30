# Greenmote

Greenmote is a Rust 2024 tool for Morrowind and `OpenMW` groundcover workflows. It provides:

- `greenmote convert`, a converter that turns vanilla-style placed static references into `OpenMW` groundcover plugins.
- `greenmote unclip`, an inspector and optional fixer for generated groundcover references that clip into terrain or static occluders.
- A desktop GUI, enabled by default, that exposes Convert, Unclip, and shared settings without requiring command-line use.

Greenmote is primarily an application, not a stable library API. The Rust public surface exists to support the binary, GUI, tests, and documentation.

## Quick Start

### GUI

Build and run the default application:

```sh
cargo run --release
```

Launching Greenmote without arguments opens the GUI when the default `gui` feature is enabled.

Typical GUI flow:

1. Open Greenmote.
2. Review or regenerate settings if prompted.
3. Use the Convert tab to run a dry run or conversion.
4. Use the Unclip tab to inspect a generated plugin.
5. Enable Unclip write mode only when you are ready to create a backup and modify the target plugin.

### CLI

Run Convert with discovered `OpenMW` settings:

```sh
cargo run --release -- convert
```

Run Convert without writing output:

```sh
cargo run --release -- convert --dry-run
```

Inspect generated groundcover clipping:

```sh
cargo run --release -- unclip --plugin groundcover.omwaddon --verbose
```

Write Unclip fixes after inspection:

```sh
cargo run --release -- unclip --plugin groundcover.omwaddon --write --write-actions terrain-z,static-move,orient
```

Top-level options such as `--openmw-cfg`, `--config`, `--generate-completion`, and `--generate-manpage` must appear before the subcommand.

## Installation And Builds

Install or build from the repository root:

```sh
cargo build --release
```

Install from a checked-out source tree:

```sh
cargo install --path .
```

The default feature set includes the GUI:

```sh
cargo build --release --all-features
```

Build a CLI-only binary without GUI dependencies:

```sh
cargo build --release --no-default-features
```

Run the CLI-only binary directly:

```sh
cargo run --release --no-default-features -- convert --dry-run
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

Generated defaults are meant to be editable. Greenmote validates regular expressions and schema keys before using the file.

## Convert Workflow

`greenmote convert` creates `OpenMW` groundcover output from matching `STAT` records and exterior cell references.

Important behavior:

- Matching is `STAT`-based. Greenmote searches static IDs using configured include and exclude regular expressions.
- Only exterior `CELL` records are scanned. Interiors are intentionally excluded.
- Matching static definitions are selected in reverse load order, so later content wins.
- Generated output includes only references touched by matching statics.
- Static-only source plugins do not become masters merely because their `STAT` records were copied.
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

`greenmote unclip` inspects a groundcover plugin against `OpenMW` terrain and static occluders. It can run in read-only inspection mode or in write mode.

Inspection mode:

- Selects a target plugin by `--plugin PLUGIN` or `[unclip].plugin` in `greenmote.toml`.
- Accepts a filesystem path or a VFS plugin name.
- Reports aggregate diagnostics by default.
- Writes detailed per-reference diagnostics to `greenmote.log` with `--verbose`.
- Keeps `--instances` as a deprecated alias for `--verbose`.
- Emits machine-readable compact JSON with `--structured`.
- Infers whether each mesh is contact-anchored or generator-style origin-anchored before planning terrain Z fixes.
- Accepts `--meshgenerator-ini INI` as an optional hint for plugins produced by `mw-groundcover-generator`; the target plugin's measured terrain-relative residuals remain the source of truth.

Write mode:

- Enabled by `--write` or `[unclip].write = true`.
- Creates a backup before replacing the target plugin.
- In the GUI, write mode requires a confirmation dialog before modifying the target plugin.
- The GUI does not persist write arming as a runtime action. You must intentionally arm write mode again when needed.

Write actions:

- `terrain-z` adjusts reference Z placement toward terrain.
- `static-delete` deletes references that cannot be safely moved away from static occluders.
- `static-move` searches for nearby positions outside static occluders.
- `orient` aligns groundcover orientation to terrain within policy limits.
- `all` enables all write actions.
- `none` disables all write actions.

Policy knobs:

- `--contact-epsilon` controls mesh contact/terrain Z tolerance.
- `--origin-epsilon` controls reference origin/terrain Z tolerance.
- `--relocation-step` controls horizontal spacing for static-bounds relocation probes.
- `--relocation-steps` controls how many relocation probe rings are attempted.
- `--orientation-epsilon` controls the tilt angle treated as already aligned.
- `--placement-model auto|contact|origin` selects the placement anchor policy. `auto` is the default and infers per mesh; `contact` always uses visible mesh contact; `origin` always uses the reference origin with an inferred terrain-relative Z offset.
- `--meshgenerator-ini` reads `mw-groundcover-generator` mesh lists as optional hints for `auto`; inferred origin-anchored refs preserve `ref.z = terrain_z_at_origin + offset` within the generator-style 4-unit tolerance.
- `--include-grass-id` and `--exclude-grass-id` filter target groundcover reference IDs with case-insensitive regexes.
- `--include-occluder-id` and `--exclude-occluder-id` filter static occluder IDs with case-insensitive regexes.

## GUI Features

The default GUI provides:

- A Convert main view with run controls, progress, status output, and generated-output safeguards.
- An Unclip main view with inspection and guarded write-mode controls.
- Settings sections for OpenMW/config paths, Convert options, Unclip options, filters, and write policy.
- Runtime-only localization for English, Swedish, Russian, Spanish, German, and French.
- Non-persistent language selection. Changing the GUI language affects the current GUI session only.

Localization applies to runtime GUI text only. CLI output, structured Unclip output, generated reports, and `greenmote.log` remain English-only.

Controller support is deferred. `PortMaster` GUI builds are also deferred.

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
dry_run = false
debug = false
auto_enable = false

[unclip]
plugin = "groundcover.omwaddon"
meshgenerator_ini = "mesh_generator_ini_files/groundcover.ini"
placement_model = "auto"
verbose = false
structured = false
write = false
write_actions = ["terrain-z", "static-delete", "static-move", "orient"]
contact_epsilon = 0.5
origin_epsilon = 2.0
relocation_step = 32.0
relocation_steps = 8
orientation_epsilon = 1.0
include_grass_ids = []
exclude_grass_ids = []
include_occluder_ids = []
exclude_occluder_ids = []
```

Key notes:

- `[convert].grass_ids` is the include list for static IDs.
- `[convert].exclude` removes matching static IDs from conversion.
- `[convert].ignored_plugins` removes matching plugin file names from conversion.
- `[convert].dry_run`, `[convert].debug`, and `[convert].auto_enable` persist their corresponding Convert toggles.
- `[unclip].plugin` is the default Unclip target plugin.
- `[unclip].placement_model` defaults to `auto`; use `contact` or `origin` only to override inference deliberately.
- `[unclip].meshgenerator_ini` provides optional `mw-groundcover-generator` mesh-list hints for automatic placement-model inference.
- `[unclip].verbose` writes detailed per-reference diagnostics to `greenmote.log`; `[unclip].instances` is still accepted as a deprecated compatibility alias.
- `[unclip].structured` switches the compact stdout summary to JSON.
- `[unclip].write` arms CLI write mode from config; use this carefully.
- `[unclip].write_actions` selects which write fixes are allowed.
- `[unclip].*_epsilon`, `relocation_step`, and `relocation_steps` tune inspection/write policy.
- Include/exclude ID filters are case-insensitive regex lists.
- Unknown TOML keys are rejected.

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
- Static-only source plugins should not become generated plugin masters unless references from those plugins are actually used.
- `--auto-enable` requires the output directory to be visible to `OpenMW` as `data-local` or a configured `data=` directory.
- Controller support is deferred.
- `PortMaster` GUI builds are deferred.
- CI is intentionally not included in this repository batch; validation commands are listed below.

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

Unexpected Unclip write plan

Run without `--write` first, add `--verbose`, and inspect `greenmote.log` for exact per-reference diagnostics.

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
