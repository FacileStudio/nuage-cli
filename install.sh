#!/usr/bin/env bash
#
# Facile Studio installer. Canonical shape lives in Wiki/CLI-STANDARD.md.
# Everything below the config block is byte-identical in every Facile CLI repo.
# Every statement sits inside a function and main() is the last line, so a
# download truncated mid-flight executes nothing at all.

set -euo pipefail

NAME="Nuage"
BIN="nuage"
REPO="FacileStudio/nuage-cli"
BRANCH="main"
BUILD="rust"
SRC_SUBDIR="."
ASSET=""
SKILL="nuage"
GO_VERSION_VAR=""

# --- output -----------------------------------------------------------------

setup_colors() {
  if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-dumb}" != "dumb" ]; then
    C_INFO=$'\033[36m' C_OK=$'\033[32m' C_WARN=$'\033[33m' C_ERR=$'\033[31m'
    C_DIM=$'\033[2m' C_OFF=$'\033[0m'
  else
    C_INFO="" C_OK="" C_WARN="" C_ERR="" C_DIM="" C_OFF=""
  fi
}

info() { printf '%s▸%s %s\n' "$C_INFO" "$C_OFF" "$*"; }
ok()   { printf '%s✓%s %s\n' "$C_OK" "$C_OFF" "$*"; }
warn() { printf '%s!%s %s\n' "$C_WARN" "$C_OFF" "$*" >&2; }
hint() { printf '  %s%s%s\n' "$C_DIM" "$*" "$C_OFF"; }
die()  { printf '%s✗%s %s\n' "$C_ERR" "$C_OFF" "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "$1 not found — $2"; }

usage() {
  cat <<EOF
Install $NAME.

Usage:
  install.sh [options]

Options:
  --bin-dir <dir>   Directory to install into (default: ~/.local/bin)
  --version <tag>   Release tag to install (default: latest)
  --source          Build from source, ignore published releases
  --no-skill        Skip AI agent skill registration
  -h, --help        Show this help

Environment:
  FACILE_BIN_DIR    Same as --bin-dir
  NO_COLOR          Disable colored output
EOF
}

# --- steps ------------------------------------------------------------------

parse_args() {
  BIN_DIR="${FACILE_BIN_DIR:-$HOME/.local/bin}"
  VERSION=""
  FROM_SOURCE=0
  WITH_SKILL=1
  while [ $# -gt 0 ]; do
    case "$1" in
      --bin-dir) BIN_DIR="${2:?--bin-dir needs a value}"; shift 2 ;;
      --bin-dir=*) BIN_DIR="${1#*=}"; shift ;;
      --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
      --version=*) VERSION="${1#*=}"; shift ;;
      --source) FROM_SOURCE=1; shift ;;
      --no-skill) WITH_SKILL=0; shift ;;
      -h|--help) usage; exit 0 ;;
      *) die "unknown option: $1 — run install.sh --help" ;;
    esac
  done
  BIN_DIR="${BIN_DIR%/}"
}

detect_platform() {
  case "$(uname -s)" in
    Linux) OS=linux ;;
    Darwin) OS=darwin ;;
    *) die "unsupported operating system: $(uname -s)" ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64) ARCH=amd64 ;;
    arm64|aarch64) ARCH=arm64 ;;
    *) die "unsupported architecture: $(uname -m)" ;;
  esac
}

make_workdir() {
  WORK="$(mktemp -d)"
  trap 'rm -rf "$WORK"' EXIT
  mkdir -p "$WORK/out"
}

prepare_bin_dir() {
  mkdir -p "$BIN_DIR" 2>/dev/null || die "cannot create $BIN_DIR"
  [ -w "$BIN_DIR" ] || die "$BIN_DIR is not writable"
}

latest_tag() {
  curl -fsSLI -o /dev/null -w '%{url_effective}' \
    "https://github.com/$REPO/releases/latest" 2>/dev/null |
    sed -n 's#.*/releases/tag/##p'
}

install_from_release() {
  [ -n "$ASSET" ] || return 1
  [ "$FROM_SOURCE" -eq 0 ] || return 1
  command -v curl >/dev/null 2>&1 || return 1
  command -v tar >/dev/null 2>&1 || return 1

  local tag ver archive base
  tag="$VERSION"
  [ -n "$tag" ] || tag="$(latest_tag)"
  [ -n "$tag" ] || return 1

  ver="${tag#v}"
  archive="${ASSET}_${ver}_${OS}_${ARCH}.tar.gz"
  base="https://github.com/$REPO/releases/download/$tag"

  info "Downloading $BIN $ver for $OS/$ARCH"
  curl -fsSL -o "$WORK/$archive" "$base/$archive" 2>/dev/null || return 1
  curl -fsSL -o "$WORK/checksums.txt" "$base/checksums.txt" 2>/dev/null || return 1
  verify_checksum "$WORK" "$archive" || die "checksum mismatch for $archive"

  tar -xzf "$WORK/$archive" -C "$WORK/out" || return 1
  [ -f "$WORK/out/$BIN" ] || return 1
  install -m 755 "$WORK/out/$BIN" "$BIN_DIR/$BIN" || die "cannot write $BIN_DIR/$BIN"
}

verify_checksum() {
  local dir="$1" file="$2" sum
  if command -v sha256sum >/dev/null 2>&1; then
    sum="$(cd "$dir" && sha256sum "$file")"
  elif command -v shasum >/dev/null 2>&1; then
    sum="$(cd "$dir" && shasum -a 256 "$file")"
  else
    warn "no sha256 tool available, skipping checksum verification"
    return 0
  fi
  grep -qF "${sum%% *}  $file" "$dir/checksums.txt"
}

install_from_source() {
  need git "install git first"
  info "Fetching source"
  git clone --depth 1 --quiet --branch "$BRANCH" \
    "https://github.com/$REPO.git" "$WORK/src" || die "cannot clone $REPO"
  SRC="$WORK/src/$SRC_SUBDIR"

  info "Building from source, this takes a minute"
  case "$BUILD" in
    rust) build_rust ;;
    go)   build_go ;;
    bun)  build_bun ;;
    *)    die "unknown build backend: $BUILD" ;;
  esac
  install -m 755 "$WORK/out/$BIN" "$BIN_DIR/$BIN" || die "cannot write $BIN_DIR/$BIN"
}

build_rust() {
  need cargo "install Rust from https://rustup.rs"
  cargo install --path "$SRC" --root "$WORK/cargo" --force --quiet
  mv "$WORK/cargo/bin/$BIN" "$WORK/out/$BIN"
}

build_go() {
  need go "install Go from https://go.dev/dl"
  local ldflags="-s -w" ver
  if [ -n "$GO_VERSION_VAR" ]; then
    ver="$(git -C "$WORK/src" describe --tags --always 2>/dev/null || echo dev)"
    ldflags="$ldflags -X $GO_VERSION_VAR=${ver#v}"
  fi
  (cd "$SRC" && go build -trimpath -ldflags "$ldflags" -o "$WORK/out/$BIN" .)
}

build_bun() {
  need bun "install Bun from https://bun.sh"
  (cd "$SRC" && bun install --frozen-lockfile --silent && bun run --silent build >/dev/null)
  mv "$SRC/$BIN" "$WORK/out/$BIN"
}

# --- AI agent skill ---------------------------------------------------------

register_skill() {
  [ -n "$SKILL" ] && [ "$WITH_SKILL" -eq 1 ] || return 0
  command -v claude >/dev/null 2>&1 || command -v codex >/dev/null 2>&1 || return 0

  local md="$WORK/SKILL.md"
  if [ -f "$WORK/src/integrations/SKILL.md" ]; then
    cp "$WORK/src/integrations/SKILL.md" "$md"
  else
    curl -fsSL -o "$md" \
      "https://raw.githubusercontent.com/$REPO/$BRANCH/integrations/SKILL.md" 2>/dev/null || return 0
  fi
  [ -s "$md" ] || return 0

  if command -v claude >/dev/null 2>&1; then
    mkdir -p "$HOME/.claude/skills/$SKILL"
    cp "$md" "$HOME/.claude/skills/$SKILL/SKILL.md"
    ok "Claude Code skill installed"
  fi
  if command -v codex >/dev/null 2>&1; then
    mkdir -p "$HOME/.codex"
    inject_block "$HOME/.codex/AGENTS.md" "$md"
    ok "Codex skill installed"
  fi
}

inject_block() {
  local file="$1" content="$2" start="<!-- $SKILL:start -->" end="<!-- $SKILL:end -->"
  local tmp
  tmp="$(mktemp)"
  if [ -f "$file" ]; then
    awk -v s="$start" -v e="$end" '
      $0 == s { skip = 1; next }
      $0 == e { skip = 0; next }
      !skip   { print }
    ' "$file" >"$tmp"
    [ -s "$tmp" ] && printf '\n' >>"$tmp"
  fi
  {
    printf '%s\n' "$start"
    cat "$content"
    printf '%s\n' "$end"
  } >>"$tmp"
  mv "$tmp" "$file"
}

# --- report -----------------------------------------------------------------

report() {
  local version shadow
  version="$("$BIN_DIR/$BIN" --version 2>/dev/null | head -n1)" ||
    die "$BIN installed to $BIN_DIR/$BIN but does not run"
  ok "${version:-$BIN} installed to ${BIN_DIR/#$HOME/\~}/$BIN"

  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
      warn "${BIN_DIR/#$HOME/\~} is not on your PATH"
      hint "export PATH=\"$BIN_DIR:\$PATH\""
      ;;
  esac

  shadow="$(command -v "$BIN" 2>/dev/null || true)"
  if [ -n "$shadow" ] && [ "$shadow" != "$BIN_DIR/$BIN" ]; then
    warn "another $BIN comes first on your PATH: $shadow"
  fi

  info "Run \`$BIN --help\` to get started"
}

# --- main -------------------------------------------------------------------

main() {
  parse_args "$@"
  setup_colors
  info "Installing $NAME"
  detect_platform
  make_workdir
  prepare_bin_dir
  install_from_release || install_from_source
  register_skill
  report
}

main "$@"
