#!/bin/bash
set -euo pipefail

REPO="https://github.com/FacileStudio/nuage-cli.git"
BIN_NAME="nuage"

info()  { printf '\033[1;36m%s\033[0m\n' "$*"; }
error() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 || error "cargo not found. Install Rust first: https://rustup.rs"
command -v git   >/dev/null 2>&1 || error "git not found. Install git first."

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "Cloning $REPO..."
git clone --depth 1 --quiet "$REPO" "$TMPDIR/nuage-cli"

info "Building and installing $BIN_NAME..."
cargo install --path "$TMPDIR/nuage-cli" --force --quiet

INSTALL_PATH="$(command -v "$BIN_NAME" 2>/dev/null || echo "$HOME/.cargo/bin/$BIN_NAME")"
info "Installed $BIN_NAME to $INSTALL_PATH"
