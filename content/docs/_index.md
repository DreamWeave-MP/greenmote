+++
title = "Documentation"
description = "How to convert placed grass into OpenMW groundcover with Greenmote, how to fix groundcover plugins with unclip, and every setting and option."
template = "docs/section.html"
page_template = "docs/page.html"
sort_by = "weight"

[extra]
docs_root = true
docs_project_name = "Greenmote"
docs_short_title = "Greenmote docs"
docs_project_path = "@/home/index.md"
docs_repository_url = "https://github.com/DreamWeave-MP/greenmote/tree/main/content/docs"
docs_sidebar_label = "Documentation"
hide_child_cards = true
kind = "guide"
+++

Greenmote does two jobs against one load order. **Convert** turns grass that your content files
place as ordinary statics into a groundcover plugin. **Unclip** takes a groundcover plugin and
fixes what does not sit right on your terrain. Both read the `openmw.cfg` OpenMW reads, and the
command line and the GUI run the same code, so everything here applies to both.

## Learn it

- **[Start here](@/docs/start-here.md)**: download it, convert your load order's grass, enable the
  result, and check a grass mod with unclip.

## Use it

- **[Convert](@/docs/convert.md)**: what is matched and in which order, the two plugins and the
  meshes it writes, and enabling them.
- **[Unclip](@/docs/unclip.md)**: what is measured, the six actions, writing and backups, and
  tuning the tolerances.
- **[The GUI](@/docs/gui.md)**: the Convert, Unclip and Settings tabs.

## Look it up

- **[greenmote.toml](@/docs/configuration.md)**: every setting, its default, and how the command
  line overrides it.
- **[Command line](@/docs/cli.md)**: every option, and the exit codes.
- **[The unclip report](@/docs/unclip-output.md)**: every line of the report, every verdict, the
  per-reference log and the JSON.
- **[Troubleshooting](@/docs/troubleshooting.md)**: each error, and what to do about it.
- **[Platforms and performance](@/docs/compatibility.md)**: what each download can do, building
  it yourself, the license, what is tested, and how long a run takes.
