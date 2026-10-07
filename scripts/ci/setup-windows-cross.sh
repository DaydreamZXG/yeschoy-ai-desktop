#!/usr/bin/env bash
# Install the pinned Linux → Windows toolchain that build-local.mjs drives:
# Rust 1.94.0 + x86_64-pc-windows-msvc, cargo-xwin, clang/lld/llvm, Zig (RC),
# and makensis 3.12. Exports YESCHOY_CARGO_XWIN, YESCHOY_ZIG, YESCHOY_NSIS_HOME
# to $GITHUB_ENV. Usage: setup-windows-cross.sh <tools-dir>
set -euo pipefail
tools="${1:?usage: setup-windows-cross.sh <tools-dir>}"
mkdir -p "$tools"
tools="$(cd "$tools" && pwd)"
repo="$(cd "$(dirname "$0")/../.." && pwd)"
cargo_xwin_version=0.23.1
zig_version=0.14.1

sudo apt-get update -q
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends \
  clang lld llvm scons zlib1g-dev bzip2 unzip minisign
# cargo-xwin's clang mode calls these by their MSVC-style names.
for tool in clang-cl lld-link llvm-lib; do
  if ! command -v "$tool" > /dev/null; then
    versioned="$(find /usr/bin -maxdepth 1 -name "$tool-*" | sort -V | tail -1)"
    [ -n "$versioned" ] || { echo "::error::$tool not found after installing clang/lld/llvm"; exit 1; }
    sudo ln -sf "$versioned" "/usr/local/bin/$tool"
  fi
done
command -v clang-cl lld-link llvm-lib

rustup toolchain install 1.94.0 --profile minimal --target x86_64-pc-windows-msvc
if [ ! -x "$tools/cargo/bin/cargo-xwin" ]; then
  cargo +1.94.0 install cargo-xwin --version "$cargo_xwin_version" --locked --root "$tools/cargo"
fi
python3 -m venv "$tools/zig-venv"
"$tools/zig-venv/bin/pip" install -q "ziglang==$zig_version"
zig="$("$tools/zig-venv/bin/python" -c 'import ziglang, os; print(os.path.join(os.path.dirname(ziglang.__file__), "zig"))')"
"$zig" version
if [ ! -x "$tools/nsis/bin/makensis" ]; then
  "$repo/scripts/ci/build-makensis.sh" "$tools/nsis"
fi
{
  echo "YESCHOY_CARGO_XWIN=$tools/cargo/bin/cargo-xwin"
  echo "YESCHOY_ZIG=$zig"
  echo "YESCHOY_NSIS_HOME=$tools/nsis"
  echo "YESCHOY_XWIN_CROSS_COMPILER=clang-cl"
} >> "${GITHUB_ENV:-/dev/stdout}"
