+++
title = "The GUI"
description = "The Greenmote window: the Convert, Unclip and Settings tabs, what each control does, and the languages it speaks."
weight = 40

[extra]
kind = "guide"
+++

Run `greenmote` with no arguments on Windows, macOS or Linux and it opens a window with three
tabs: **Convert**, **Unclip** and **Settings**. The window runs the same convert and unclip as the
command line, with the settings in `greenmote.toml`. The Android and PortMaster builds have no
window.

## Starting up

The window finds your `openmw.cfg` as the command line does and loads `greenmote.toml` beside it,
creating it with the defaults if it is missing. Two things can stop it:

- **OpenMW config not found**: no `openmw.cfg` was found or it could not be read. **Select OpenMW
  Config** opens a file picker for one.
- **Malformed config**: `greenmote.toml` is not valid. **Back up, regenerate, and continue** moves
  it aside as `greenmote.toml.bak` and writes the defaults.

The status line beside the run button says which, as `Loaded OpenMW config: PATH`.

## Convert

**Run options** apply to the next conversion:

| Option | Does |
|---|---|
| **Dry run** | Print the plan and write nothing. Turns **Debug diagnostics** off |
| **Debug diagnostics** | Print the plan as the run goes. Turns **Dry run** off |
| **Auto-enable generated plugins** | Add the output to `openmw.cfg`. Not available in a dry run |

**Save as defaults** writes **Debug diagnostics** and **Auto-enable** to `greenmote.toml` as
`debug` and `auto_enable`; **Dry run** is never saved. **Reset from saved** puts the options back
to what the file says, and a line says whether they differ.

**Start conversion** runs it. A progress bar follows each phase: loading plugins, planning,
scanning cells, finding meshes, writing plugins, copying meshes, enabling and writing the log. The
output below it is what the command line would print. Under the output:

| Button | Does |
|---|---|
| **Cancel** | Stop the run. If plugins were already being written, the log says how far it got |
| **Clear output** | Empty the output |
| **Copy output** | Copy it to the clipboard |
| **Open output dir** | Open convert's output folder |
| **Open log** | Open `greenmote.log` |

[Convert](@/docs/convert.md) explains what a run matches and writes.

## Unclip

The tab lists every `groundcover=` plugin in your `openmw.cfg`, in load order. A plugin whose file
is found is ticked; one that is not is greyed out and marked `not found`. Greenmote's own
`groundcover.omwaddon` is never listed: fix the plugins it is built from instead. **Reload list**
reads `openmw.cfg` again, as does changing the OpenMW config or saving settings.

**Write changes** is on when the tab opens:

- **On**: the button reads **Fix plugins**. After you confirm, each ticked plugin is rewritten in
  place, and the untouched original is kept beside it as `<plugin>.greenmote-original`.
- **Off**: the button reads **Inspect only**. Each ticked plugin is checked and reported, and
  nothing is written.

The plugins run one after another, and each shows its state beside its name: `Pending`,
`Running`, `Succeeded`, `Failed`, `Skipped` or `Cancelled`. When inspecting, a plugin that fails
does not stop the others. When writing, one that fails while it is being written stops the batch,
and the rest are skipped; one that fails before writing began, such as a plugin that will not
load, does not.

The actions, numbers and filters come from `greenmote.toml`; change them on the Settings tab. The
report for each plugin appears in the output, as the command line prints it, under the same
buttons as on the Convert tab; here **Open log** opens `greenmote-unclip.log`, which holds the
last plugin's report. With no write actions
turned on, **Fix plugins** refuses to start. [Unclip](@/docs/unclip.md) explains every action.

## Settings

Everything on this tab is `greenmote.toml`, except the language. **Save** writes it; leaving the
tab with unsaved changes asks whether to save them first.

**General**

- **Language**: English, French, German, Russian, Spanish or Swedish, for this session only. The
  command line, the reports and the logs are always English.
- **OpenMW config**: the `openmw.cfg` in use, or autodetection. **Select OpenMW Config** picks
  another for this session. It cannot change while a run is going.
- **Convert output directory**: where convert will write, and why: `data-local` from the selected
  `openmw.cfg`, or the current folder when there is none. The window says when OpenMW would not
  read that folder.

**Convert**: the **Grass ID patterns**, **Exclude patterns** and **Ignored plugins** lists. Add
an entry with **Add**, edit one in place (emptying it removes it), remove the selected one with
**-**, or move it with **Up** and **Down**.

**Unclip**

- **Write actions**: one box per action. With none ticked, a warning says a write would do
  nothing.
- **Policy numbers**: every tolerance, each with a tooltip naming its `greenmote.toml` key.
- **Filters**: **Include grass IDs**, **Exclude grass IDs**, **Include occluder IDs**, **Exclude
  occluder IDs** and **Road texture paths**. These are whole-ID patterns; an exclusion beats an
  inclusion, and an empty inclusion list means everything.

Keys the window does not know, such as settings left over from older versions, are dropped when
it saves. [greenmote.toml](@/docs/configuration.md) lists every key.
