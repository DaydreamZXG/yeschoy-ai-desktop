#!/usr/bin/env bash
# Build a Linux makensis 3.12 with the layout build-local.mjs expects:
#   $PREFIX/bin/makensis and $PREFIX/share/nsis/{Stubs,Plugins,Include,Contrib}
# The compiler is built from the official source; the Windows-side data
# (stubs, plugins, headers) comes from the official 3.12 binary zip.
# Usage: build-makensis.sh <prefix>
set -euo pipefail
prefix="${1:?usage: build-makensis.sh <prefix>}"
version=3.12
src_sha=f3ed7a8e4aa2cf4e8cf47d3b563a02559e0cb4934db2662b2f9661b824e2b186
zip_sha=56581f90db321581c5381193d796fffcf2d24b2f8fed2160a6c6a3baa67f2c4f
base="https://downloads.sourceforge.net/project/nsis/NSIS%203/${version}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl -fsSL --retry 3 -o "$work/src.tar.bz2" "$base/nsis-${version}-src.tar.bz2"
curl -fsSL --retry 3 -o "$work/bin.zip" "$base/nsis-${version}.zip"
printf '%s  %s\n%s  %s\n' "$src_sha" "$work/src.tar.bz2" "$zip_sha" "$work/bin.zip" | sha256sum --check --strict
tar -xjf "$work/src.tar.bz2" -C "$work"
unzip -q "$work/bin.zip" -d "$work"
(
  cd "$work/nsis-${version}-src"
  python3 -m SCons -Q SKIPSTUBS=all SKIPPLUGINS=all SKIPUTILS=all SKIPMISC=all \
    NSIS_CONFIG_CONST_DATA_PATH=no PREFIX="$work/out" install-compiler
)
mkdir -p "$prefix/bin" "$prefix/share/nsis"
install -m 755 "$work/out/makensis" "$prefix/bin/makensis"
for dir in Stubs Plugins Include Contrib; do
  cp -R "$work/nsis-${version}/$dir" "$prefix/share/nsis/"
done
NSISDIR="$prefix/share/nsis" "$prefix/bin/makensis" -VERSION
echo
