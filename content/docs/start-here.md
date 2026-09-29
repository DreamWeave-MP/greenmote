+++
title = "Start here"
description = "Download Greenmote, convert your load order's grass into groundcover, enable it, and check a grass mod with unclip."
weight = 10

[extra]
kind = "tutorial"
+++

You need OpenMW with a working load order: an `openmw.cfg` that lists your data folders and
content files. Greenmote reads it the way OpenMW does and changes nothing in it unless you ask.

## Download it

Take the archive for your system from the [project page](@/home/index.md) and unzip it anywhere.
It holds one program, `greenmote`, with its README and license. There is nothing to install.

- **macOS**: the builds are not notarized. If macOS will not open it, run
  `xattr -d com.apple.quarantine greenmote` in its folder, or allow it under System Settings,
  Privacy & Security.
- **Linux and macOS**: if the shell says permission denied, `chmod +x greenmote`.
- **Android and PortMaster**: these builds are the command line only. Everything below under
  "on the command line" works there.

## Check which openmw.cfg it found

Greenmote looks for `openmw.cfg` where OpenMW looks: next to the program, then the system
location, then your user folder.

| System | Your user `openmw.cfg` |
|---|---|
| Linux | `~/.config/openmw/openmw.cfg` |
| Windows | `Documents\My Games\OpenMW\openmw.cfg` |
| macOS | `~/Library/Preferences/openmw/openmw.cfg` |

If you keep several profiles, name one: `greenmote --openmw-cfg /path/to/profile convert`, or
**Select OpenMW Config** on the GUI's Settings tab. A folder works as well as the file. Greenmote
keeps its own settings, `greenmote.toml`, and its logs next to that `openmw.cfg`.

## Convert your load order's grass

Morrowind, Tribunal, Bloodmoon and many mods place grass as ordinary statics. Convert collects
them into a groundcover plugin.

In the GUI, run `greenmote` with no arguments; on Windows, double-click it. On the **Convert**
tab, tick **Dry run** and press **Start conversion**. The output lists every static that matched
and how many references each plugin contributes. Untick **Dry run** and start it again to write
the files.

On the command line, the same two steps:

```sh
greenmote convert --dry-run
greenmote convert
```

A vanilla install with both expansions matches 33 statics and moves 30,916 references into
groundcover. The run ends by saying where it wrote and what to enable:

```text
Generated groundcover.omwaddon and deleted_groundcover.omwaddon in /home/you/.local/share/openmw/data
Copied 30 meshes under Meshes/grass
Wrote log to /home/you/.config/openmw/greenmote.log
Output directory is already visible to OpenMW; no data-local or data= change is needed.
Add groundcover.omwaddon as groundcover= and deleted_groundcover.omwaddon as content= in openmw.cfg.
```

## Enable it

Add the two lines it asks for to the end of your `openmw.cfg`:

```ini
groundcover=groundcover.omwaddon
content=deleted_groundcover.omwaddon
```

`deleted_groundcover.omwaddon` hides the original statics, so it loads after every plugin that
placed them: the end of the load order is right. Or let Greenmote add both lines, with a backup of
the cfg first: tick **Auto-enable generated plugins**, or run `greenmote convert --auto-enable`.

OpenMW only draws groundcover when it is turned on. In `settings.cfg`:

```ini
[Groundcover]
enabled = true
```

Start OpenMW. The grass is where it was, now drawn as groundcover.

Convert again whenever your load order changes: it rebuilds both plugins from scratch each time.
[Convert](@/docs/convert.md) explains what it matches and writes.

## Check a grass mod with unclip

Grass mods such as Remiros' Groundcover, Aesthesia or Fantasia are generated against one
landscape. When your load order moves the ground, as Tamriel Rebuilt, a landscape overhaul or a
city mod does, some of that grass floats, sinks, sits on a road or in a rock. Unclip finds it.

In the GUI, the **Unclip** tab lists every `groundcover=` plugin in your `openmw.cfg`, each ticked
when its file is found. Untick **Write changes** and press **Inspect only** to see what would
change. Tick it again and press **Fix plugins**: after you confirm, each checked plugin is
rewritten in place, and the untouched original is kept beside it as
`<plugin>.greenmote-original`.

On the command line, unclip only reports until you pass `--write`:

```sh
greenmote unclip --plugin Rem_AI.esp
greenmote unclip --plugin Rem_AI.esp --write
```

`--plugin` takes a file name OpenMW can find in your data folders, or a path to any plugin, so a
grass mod you have downloaded but not enabled yet can be checked in place. The report says what
it found:

```text
Refs: 10591 matched, 7850 fine, 2361 to fix, 380 to delete, 0 skipped
  delete_inside_static             322
  delete_no_relocation             47
  delete_road                      11
  fix_ground                       1436
  fix_move                         925
```

[Unclip](@/docs/unclip.md) explains each action, and [the report](@/docs/unclip-output.md) each
line.
Unclip run again on a plugin it has fixed plans nothing.

## If it stops

- **`openmw.cfg has no content files`**: the cfg Greenmote found is not your load order. Point it
  at the right one with `--openmw-cfg`.
- **`refusing to auto-enable outputs in ...`**: the output folder is not one OpenMW reads. Set it
  as `data-local`, add it as `data=`, or write elsewhere with `--output`.
- **`... was generated by greenmote convert and cannot be unclipped`**: unclip the grass mods
  instead. Convert's output is rebuilt from your load order every run, so fixes written into it
  would be lost.

[Troubleshooting](@/docs/troubleshooting.md) has the rest.
