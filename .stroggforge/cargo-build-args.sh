#!/usr/bin/env bash
set -euo pipefail

# Copy this file to .stroggforge/cargo-build-args.sh in a consuming repository
# and make it executable. Corprus Crucible calls it for release builds as:
#
#   .stroggforge/cargo-build-args.sh "$platform_os" "$platform_arch" "$rust_target" "$binary_name"
#
# Print one extra Cargo feature argument per line on stdout. Send diagnostics to
# stderr; stdout is parsed as Cargo arguments. Do not print shell-quoted strings.
# Only --features, -F, --no-default-features, and --all-features are allowed.
# For --features or -F, print the feature list on the next line, or use
# --features=gui / -F=gui.

platform_os=${1:?platform-os}
platform_arch=${2:?platform-arch}
rust_target=${3:-}
binary_name=${4:?binary-name}

# Available for projects that need target- or binary-specific policy. This
# example only keys off the platform tuple.
: "$rust_target" "$binary_name"

case "${platform_os}-${platform_arch}" in
  macOS-ARM64|macOS-x64|Windows-x64|Linux-x64)
    # The gui feature is a default feature in the common DreamWeave desktop shape,
    # but listing it here makes the platform policy visible instead of implicit.
    printf '%s\n' --features
    printf '%s\n' gui
    ;;
  Android-ARM64|Portmaster-ARM64)
    printf '%s\n' --no-default-features
    ;;
  *)
    printf 'unsupported StroggForge platform tuple: %s-%s\n' "$platform_os" "$platform_arch" >&2
    exit 1
    ;;
esac
