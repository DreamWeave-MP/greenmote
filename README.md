# Greenmote

Turn placed grass into `OpenMW` groundcover, and fix groundcover that floats, sinks, sits on roads
or water, or clips into rocks.

Morrowind places its grass one static at a time, like any rock. `OpenMW` draws plugins loaded with
`groundcover=` as groundcover: in bulk, without collision, and thinned by the density setting.
Greenmote does two jobs against the load order in your `openmw.cfg`:

- **`greenmote convert`** finds the grass statics your content files place in exterior cells and
  writes `groundcover.omwaddon`, which places them again as groundcover, and
  `deleted_groundcover.omwaddon`, which hides the originals, with their meshes copied beside them.
- **`greenmote unclip`** measures every reference in a groundcover plugin against your terrain,
  water, road textures and static collision, and grounds, tilts, moves or deletes what does not
  sit right. It reports first, and writes only with `--write`, keeping the original.

Run it with no arguments for a desktop GUI with both, in six languages.

**Documentation and downloads: <https://dreamweave-mp.github.io/greenmote/>**

## Install

Download the build for your system from the
[releases](https://github.com/DreamWeave-MP/greenmote/releases): Windows, macOS on Apple silicon
and Intel, and Linux, with the GUI; Android and `PortMaster`, command line only. Unzip it anywhere
and run `greenmote`. Or build it:

```sh
cargo install --git https://github.com/DreamWeave-MP/greenmote
```

## Use

```sh
greenmote convert --dry-run                   # plan the conversion, write nothing
greenmote convert --auto-enable               # write it and add it to openmw.cfg
greenmote unclip --plugin Rem_AI.esp          # report what a grass mod needs fixed
greenmote unclip --plugin Rem_AI.esp --write  # fix it in place, keeping the original
```

Options such as `--openmw-cfg` and `--config` go before the subcommand. `OpenMW` draws groundcover
only with `enabled = true` under `[Groundcover]` in `settings.cfg`.

## Where to read next

- [Start here](https://dreamweave-mp.github.io/greenmote/docs/start-here/): a first conversion and
  a first unclip
- [Convert](https://dreamweave-mp.github.io/greenmote/docs/convert/) and
  [Unclip](https://dreamweave-mp.github.io/greenmote/docs/unclip/): what each reads, measures and
  writes
- [The GUI](https://dreamweave-mp.github.io/greenmote/docs/gui/)
- [greenmote.toml](https://dreamweave-mp.github.io/greenmote/docs/configuration/) and the
  [command line](https://dreamweave-mp.github.io/greenmote/docs/cli/): every setting and option
- [Troubleshooting](https://dreamweave-mp.github.io/greenmote/docs/troubleshooting/)

## Development

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -W clippy::pedantic -D warnings
cargo test --workspace --all-features
```

`cargo build --no-default-features` builds the command line without the GUI. The site in
`content/` is a [DreamWeave Mod Template](https://github.com/DreamWeave-MP/DreamWeave-Mod-Template)
site; preview it with `zola serve`.

## License

MIT OR Apache-2.0, at your option. See [LICENSE-MIT](https://github.com/DreamWeave-MP/greenmote/blob/main/LICENSE-MIT) and [LICENSE-APACHE](https://github.com/DreamWeave-MP/greenmote/blob/main/LICENSE-APACHE).
