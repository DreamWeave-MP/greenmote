+++
title = "Greenmote"
description = "Turn placed grass into OpenMW groundcover, and fix groundcover that floats, sinks, sits on roads or water, or clips into rocks."

[taxonomies]
tags = ["OpenMW", "Morrowind", "Groundcover", "Rust"]
+++

Morrowind places its grass, ferns, kelp and lilypads one static at a time, like any rock. OpenMW
has a better home for them: plugins loaded with `groundcover=` are drawn as groundcover, in bulk,
without collision, and thinned by the player's density setting. Greenmote moves grass into that
home and keeps it standing on the ground your load order actually has.

{{ schematic(data_path="data/schematics/greenmote.json") }}

**Convert** reads your load order, finds the grass statics every content file places in exterior
cells, and writes two plugins: `groundcover.omwaddon`, which places that grass again as
groundcover, and `deleted_groundcover.omwaddon`, which hides the originals. The meshes are copied
beside them.

**Unclip** takes a groundcover plugin, such as a grass mod generated for somebody else's
landscape, and measures every reference in it against your terrain, water, roads and statics. Grass
that floats is lowered, grass buried too deep is raised, tilts follow the slope, and grass on a
road, under new water or inside a rock is moved or deleted. It reports first and writes only when
asked, keeping the original beside the plugin.

```sh
greenmote convert --dry-run
greenmote unclip --plugin Rem_AI.esp
greenmote unclip --plugin Rem_AI.esp --write
```

Run `greenmote` with no arguments and both are in a window, with a Settings tab for
`greenmote.toml`. The GUI speaks English, French, German, Russian, Spanish and Swedish.

## Documentation

- **[Start here](@/docs/start-here.md)**: a first conversion and a first unclip, in the GUI or on
  the command line.
- **[Convert](@/docs/convert.md)**: what is matched, what is written, and how to enable it.
- **[Unclip](@/docs/unclip.md)**: what is measured, each action, and reading the report.
- **[greenmote.toml](@/docs/configuration.md)** and **[Command line](@/docs/cli.md)**: every
  setting and option.
