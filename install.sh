#!/usr/bin/env bash
#
# Facile Studio installer. This is a shim by design: the installation logic
# lives in `facile`, the suite installer, so the suite has exactly one
# implementation of it instead of one copy per repo. Canonical shape in
# Wiki/CLI-STANDARD.md.
#
# Equivalent, once facile is on your PATH:
#   facile install nuage
#
# Every statement sits inside a function and main() is the last line, so a
# download truncated mid-flight executes nothing at all.

set -euo pipefail

TOOL="nuage"
BOOTSTRAP="https://get.facile.studio"
BOOTSTRAP_FALLBACK="https://raw.githubusercontent.com/FacileStudio/facile/main/install.sh"

# The pretty URL is one host; GitHub is the one that has to be up anyway. Trying
# both costs a line and stops a single VPS from being able to break every
# install command in the suite.
bootstrap_facile() {
  command -v curl >/dev/null 2>&1 ||
    { printf '\033[31m✗\033[0m curl not found — install curl first\n' >&2; exit 1; }
  curl -fsSL "$BOOTSTRAP" | bash ||
    curl -fsSL "$BOOTSTRAP_FALLBACK" | bash
  export PATH="${FACILE_BIN_DIR:-$HOME/.local/bin}:$PATH"
}

main() {
  command -v facile >/dev/null 2>&1 || bootstrap_facile
  exec facile install "$TOOL" "$@"
}

main "$@"
