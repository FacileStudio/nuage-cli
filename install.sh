#!/usr/bin/env bash
#
# Facile Studio installer. This is a shim by design: the installation logic
# lives in `kit`, the Facile package manager, so the suite has exactly one
# implementation of it instead of one copy per repo.
#
# Equivalent, once kit is on your PATH:
#   kit add nuage
set -euo pipefail
BOOTSTRAP="https://get.facile.studio"
BOOTSTRAP_FALLBACK="https://raw.githubusercontent.com/FacileStudio/kit/main/install.sh"
command -v kit >/dev/null 2>&1 ||
  { curl -fsSL "$BOOTSTRAP" | bash || curl -fsSL "$BOOTSTRAP_FALLBACK" | bash
    export PATH="${KIT_BIN_DIR:-$HOME/.local/bin}:$PATH"; }
exec kit add nuage "$@"
