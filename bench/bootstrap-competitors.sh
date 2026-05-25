#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

mkdir -p .bench-tools/cargo .bench-tools/cargo-home .bench-tools/uv/tools .bench-tools/bin

echo "Installing Rust schema competitors into .bench-tools/cargo/bin"
CARGO_HOME="$repo_root/.bench-tools/cargo-home" \
  cargo install --root "$repo_root/.bench-tools/cargo" genson-cli drivel

echo "Installing Python schema competitors into .bench-tools/bin"
UV_TOOL_DIR="$repo_root/.bench-tools/uv/tools" \
UV_TOOL_BIN_DIR="$repo_root/.bench-tools/bin" \
  uv tool install json-to-schema

UV_TOOL_DIR="$repo_root/.bench-tools/uv/tools" \
UV_TOOL_BIN_DIR="$repo_root/.bench-tools/bin" \
  uv tool install schemax-cli

cat <<EOF

Installed competitor tools:
  $repo_root/.bench-tools/cargo/bin/genson-cli
  $repo_root/.bench-tools/cargo/bin/drivel
  $repo_root/.bench-tools/bin/json-to-schema
  $repo_root/.bench-tools/bin/schemax

If you are not inside nix develop, add:
  export PATH="$repo_root/.bench-tools/cargo/bin:$repo_root/.bench-tools/bin:\$PATH"
EOF
