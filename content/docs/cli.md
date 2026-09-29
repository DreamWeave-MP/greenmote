+++
title = "Command line"
description = "Every greenmote option, for the program, convert and unclip, what goes to which stream, and the exit codes."
weight = 60

[extra]
kind = "reference"
+++

```text
greenmote [--openmw-cfg PATH] [--config PATH] convert [options]
greenmote [--openmw-cfg PATH] [--config PATH] unclip [options]
greenmote --generate-completion SHELL
greenmote --generate-manpage
```

With no arguments at all, the Windows, macOS and Linux builds open the [GUI](@/docs/gui.md). The
Android and PortMaster builds have no GUI, and with no arguments run `convert` with its defaults.
Any argument, even `--help`, means the command line.

## Program options

These go before `convert` or `unclip`: `greenmote --openmw-cfg profile convert`, not
`greenmote convert --openmw-cfg profile`.

| Option | Meaning |
|---|---|
| `-c`, `--openmw-cfg PATH` | The `openmw.cfg` to read, or a folder holding one. Without it, Greenmote finds one as OpenMW does |
| `--config PATH` | The `greenmote.toml` to use. Default: next to your user `openmw.cfg` |
| `--generate-completion SHELL` | Print a completion script for `bash`, `elvish`, `fish`, `powershell` or `zsh`, and exit |
| `--generate-manpage` | Print the manual page, in roff, and exit |
| `-h`, `--help` | Print help. `greenmote convert --help` and `greenmote unclip --help` describe each command |
| `-V`, `--version` | Print `greenmote` and the version |

```sh
greenmote --generate-completion bash > ~/.local/share/bash-completion/completions/greenmote
greenmote --generate-manpage > ~/.local/share/man/man1/greenmote.1
```

### Finding openmw.cfg

Without `--openmw-cfg`, Greenmote reads the file OpenMW would start from: the one named by
`OPENMW_CONFIG`, else one in a folder listed in `OPENMW_CONFIG_DIR`, else one next to the program,
else the system-wide one an OpenMW install provides, following its `config=` lines to yours. With
none of those, it reads the default user `openmw.cfg` directly. Standard error says which it used
when it is not your user one:

```text
Using OpenMW configuration found next to Greenmote:
  /games/openmw/openmw.cfg
```

When `--openmw-cfg` names nothing usable, Greenmote says why and asks on standard error whether to
use the default user `openmw.cfg` instead. Anything but `y` or `yes` stops it with an error that
shows how to pass a profile.

## convert

```text
greenmote convert [-o DIR] [--ignore REGEX,...] [--dry-run | --validate-config] [-e] [-d]
```

| Option | Meaning |
|---|---|
| `-o`, `--output DIR` | Write the plugins and meshes to `DIR`. Default: `data-local`, else the current folder |
| `--ignore REGEX[,REGEX...]` | Also skip content files whose name matches. Added to `ignored_plugins` |
| `--dry-run[=BOOL]` | Print the plan and write nothing |
| `--validate-config[=BOOL]` | Check `greenmote.toml` and its patterns, and write nothing. Needs the file to exist |
| `-e`, `--auto-enable` | Add the output to `openmw.cfg` after writing it |
| `-d`, `--debug` | Print the plan to standard error as the run goes |

`--dry-run` and `--validate-config` cannot be combined. A successful validation prints
`Validated PATH successfully`. [Convert](@/docs/convert.md) covers what each run reads and writes.

## unclip

```text
greenmote unclip [-p PLUGIN] [--write [--output-plugin PATH]] [--verbose] [--structured] [policy options]
```

| Option | Meaning |
|---|---|
| `-p`, `--plugin PLUGIN` | The groundcover plugin to check: a path, or a name found in the data folders. Default: `plugin` in `greenmote.toml` |
| `--write[=BOOL]` | Write the changes. Without it, unclip only reports |
| `--output-plugin PATH` | With `--write`, write the rewritten plugin to `PATH` and leave the source alone |
| `--verbose[=BOOL]` | Add one line per reference to `greenmote-unclip.log`, and every reference to `--structured` output |
| `--structured[=BOOL]` | Print the report as JSON |

Policy options, each overriding its [`greenmote.toml` setting](@/docs/configuration.md#unclip) for
the run:

| Option | Default | Meaning |
|---|---:|---|
| `--actions ACTION[,ACTION...]` | all | Which of `terrain-z`, `water-delete`, `road-delete`, `static-delete`, `static-move`, `orient` to plan. `--write-actions` is the same option |
| `--float-tolerance N` | 1 | Units the mesh base may float |
| `--max-sink N` | 24 | Units the mesh base may be buried |
| `--max-sink-fraction N` | 0.75 | The same, as a fraction of the mesh's height, 0 to 1. The larger limit applies |
| `--sink N` | 4 | Units below the surface a fixed base is placed |
| `--relocation-step N` | 32 | Units between static-move's search rings |
| `--relocation-steps N` | 8 | Search rings, 1 to 256 |
| `--orientation-epsilon N` | 1 | Degrees of tilt treated as aligned |
| `--max-tilt N` | 45 | Steepest ground, in degrees, grass is tilted to |
| `--include-grass-id REGEX` | | Only check references whose whole ID matches. Repeatable |
| `--exclude-grass-id REGEX` | | Do not check references whose whole ID matches. Repeatable |
| `--include-occluder-id REGEX` | | Only statics whose whole ID matches block grass. Repeatable |
| `--exclude-occluder-id REGEX` | tree-like statics | Statics whose whole ID matches never block grass. Adds to the list. Repeatable |
| `--no-default-occluder-excludes` | | Start the occluder exclusions empty |
| `--road-texture-path REGEX` | road textures | Terrain texture paths road-delete deletes on. Adds to the list. Repeatable |
| `--no-default-road-textures` | | Start the road patterns empty |

The `[=BOOL]` options also take `=true` or `=false`. [Unclip](@/docs/unclip.md) explains every
action and number.

Options from older versions are refused: `--dry-run` (reporting is the default), `--in-place`,
`--instances`, `--meshgenerator-ini`, `--origin-epsilon` and `--contact-epsilon`.
`--ignore-missing-meshes` is still accepted and does nothing.

## Output

| Command | Standard output | Standard error |
|---|---|---|
| `convert` | The plan with `--dry-run`; otherwise what was written and what to enable | Plugin load warnings, the plan with `--debug`, errors |
| `convert --validate-config` | `Validated PATH successfully` | Errors |
| `unclip` | The report, or its JSON with `--structured` | Warnings about unknown settings, phase timings with `GREENMOTE_PROFILE=1`, errors |

Both also write a log next to your user `openmw.cfg`: `greenmote.log` for a convert that writes,
`greenmote-unclip.log` for every unclip. [The unclip report](@/docs/unclip-output.md) describes
its lines and JSON.

Errors are one message on standard error, starting `error:`. Some span several lines, with the
command to run next.

## Exit codes

| Code | When |
|---|---|
| `0` | Success, including a dry run with changes to make, and a closed output pipe |
| `1` | The command failed: no `openmw.cfg`, a missing mesh, an invalid setting, a plugin that cannot be read or written |
| `2` | The command line is wrong: an unknown option, a missing or malformed value, `--dry-run` with `--validate-config` |

## Examples

```sh
# Plan a conversion of a specific profile's grass, and write nothing.
greenmote --openmw-cfg ~/.config/openmw/modded convert --dry-run

# Convert, skipping two mods, into a folder OpenMW already reads, and enable the result.
greenmote convert --ignore '^Rem_,^FGM_' --output ~/.local/share/openmw/data --auto-enable

# Check a downloaded grass mod before enabling it, one log line per reference.
greenmote unclip --plugin "Downloads/Remiros Groundcover/00 Core OpenMW/Rem_AI.esp" --verbose

# Only ground floating grass; leave everything else alone.
greenmote unclip --plugin Rem_AI.esp --actions terrain-z --write

# Write the fixed plugin next to the original to compare them in game.
greenmote unclip --plugin FGM_BC.esp --write --output-plugin FGM_BC_unclipped.esp

# Treat one more family of statics as see-through.
greenmote unclip --plugin Rem_AI.esp --exclude-occluder-id 'ex_ashl_.*'

# Machine-readable counts.
greenmote unclip --plugin Rem_AI.esp --structured | jq .counts
```
