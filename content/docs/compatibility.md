+++
title = "Platforms and performance"
description = "What each download can do, how to build Greenmote yourself, how releases are signed, the license, what is tested, and how long a run takes."
weight = 90

[extra]
kind = "reference"
+++

## Downloads

| Download | For | Has |
|---|---|---|
| `greenmote-Windows-X64.zip` | Windows, x86-64 | GUI and command line |
| `greenmote-macOS-ARM64.zip` | macOS, Apple silicon | GUI and command line |
| `greenmote-macOS-X64.zip` | macOS, Intel | GUI and command line |
| `greenmote-Linux-X64.zip` | Linux, x86-64, glibc 2.34 or newer, X11 or Wayland | GUI and command line |
| `greenmote-Portmaster-ARM64.zip` | ARM64 Linux handhelds, glibc 2.34 or newer | Command line |
| `greenmote-Android-ARM64.zip` | Android 6 (API 23) or newer, ARM64 | Command line. A program for a terminal such as Termux, not an app |

Each archive holds `greenmote`, its README and license, and a Sigstore bundle. On the builds
without a GUI, `greenmote` with no arguments runs `convert` with its defaults. The Linux and
PortMaster builds of 0.3.0 and older run on glibc 2.28 or newer; later ones are built on
CentOS Stream 9 and need 2.34.

## Building it yourself

Greenmote needs Rust 1.95 or newer. It is not on crates.io, because it depends on
[tes3](https://github.com/Greatness7/tes3) from git; build it from the repository:

```sh
cargo install --git https://github.com/DreamWeave-MP/greenmote
```

or, in a clone, `cargo build --release`. That builds the desktop program, GUI included. The GUI
is the one Cargo feature:

| Feature | Default | Adds |
|---|---|---|
| `gui` | Yes | The window, through eframe with OpenGL on X11 and Wayland, and native file pickers through rfd |

```sh
cargo build --release --no-default-features   # the command line only
```

The release builds are exactly these:

| Build | Features |
|---|---|
| Windows, macOS, Linux | `gui` |
| Android, PortMaster | `--no-default-features` |

The library behind the program is not a stable API; it exists for the program and its tests.

## Signed releases

Each archive holds, beside the program, a Sigstore bundle for it, such as
`greenmote-Linux-X64.bundle`, made by StroggForge's release workflow. It proves that workflow
built the program for this repository:

```sh
cosign verify-blob greenmote \
  --bundle greenmote-Linux-X64.bundle \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/DreamWeave-MP/StroggForge/\.github/workflows/rustGlobalBuild\.yml@' \
  --certificate-github-workflow-repository DreamWeave-MP/greenmote
```

Each GitHub release also links every archive's VirusTotal scan.

## License

MIT OR Apache-2.0, at your option, since 0.4.0. 0.2.0 and 0.3.0 were GPL-3.0-only, and 0.1.0 was published without a license.

## What is tested

Every push runs [StroggForge](https://github.com/DreamWeave-MP/StroggForge)'s release workflow:
the tests on Windows, Linux, and macOS on both Apple silicon and Intel, Clippy at the pedantic
level with warnings as errors, `rustfmt`, and `cargo audit`, before anything is built for release.
The same checks except the tests run every day on the default branch.

The tests build plugins, meshes and `openmw.cfg` files in code and run the real commands on them:
conversions of statics and scriptless activators, dry runs that write nothing, identical output
from identical input, and every `--auto-enable` case; unclip dry runs, in-place writes that keep
their backups and plan nothing when run again, `--output-plugin` copies, and plugins outside the
data folders. Unit tests cover split grids, masters, settings and the verdict rules. Rotation
order is checked numerically against OpenMW's, and the box-against-triangle test against the
physics library's own.

## How long a run takes

Measured on an Intel Core i7-10870H (8 cores, 16 threads) running Linux, with a release build of
the development version after 0.3.0 and a warm file cache, against Morrowind, Tribunal and
Bloodmoon:

| Run | References | Time | CPU time | Peak memory |
|---|---:|---:|---:|---:|
| `convert --dry-run` | 30,916 converted | 0.32 s | 0.35 s | 301 MB |
| `unclip`, Remiros' `Rem_AI.esp` | 91,759 | 0.58 s | 2.75 s | 407 MB |
| `unclip`, Fantasia's `FGM_BC.esp` | 119,280 | 0.44 s | 1.74 s | 438 MB |
| `unclip`, Fantasia's `FGM_AI.esp` | 173,880 | 0.60 s | 2.77 s | 494 MB |
| `unclip`, Aesthesia's `Grass Vanilla 1.esp` | 506,815 | 1.78 s | 13.46 s | 822 MB |

Unclip loads the load order's plugins and the statics' meshes in parallel, and decides every
reference in parallel, so it scales with cores. `GREENMOTE_PROFILE=1` prints where the time goes.
For the Aesthesia plugin:

```text
[profile] config, vfs, target plugin      0.123s  (total   0.123s)
[profile] context plugins                 0.180s  (total   0.303s)
[profile] statics, terrain, textures      0.013s  (total   0.317s)
[profile] grass meshes                    0.002s  (total   0.319s)
[profile] occluders                       0.114s  (total   0.433s)
[profile] decide                          0.978s  (total   1.411s)
```

Most of `decide` is static-move's search for a clear spot. Before 0.3.0 it took 27 of 36 CPU
seconds on this plugin; testing overlap before containment, testing boxes against triangles
directly, and deciding once per texture whether it is a road brought it to 9.
