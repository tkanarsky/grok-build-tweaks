#!/usr/bin/env bash
# Build the pager from this tree and point ~/.grok/bin/{grok,agent} at it.
#
# Usage (from anywhere):
#   ./scripts/install-local.sh
#
# The release binary is copied to:
#   $GROK_HOME/downloads/grok-tkanarsky-<sha>[-dirty]-<UTC timestamp>
# then the grok and agent symlinks in $GROK_HOME/bin are retargeted, and
# [cli] auto_update = false is written to $GROK_HOME/config.toml so the
# managed installer cannot overwrite those links on next launch.
#
# Build id: cargo/rustc do not mint a unique per-invocation id. On Linux the
# linker embeds a GNU ELF BuildID, but that is a content hash of the binary
# (identical rebuilds collide). We use git SHA + UTC timestamp instead so
# successive local installs stay distinct and traceable.
#
# Env: GROK_HOME (default: ~/.grok)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

GROK_HOME="${GROK_HOME:-${HOME}/.grok}"
DOWNLOAD_DIR="${GROK_HOME}/downloads"
BIN_DIR="${GROK_HOME}/bin"

sha="$(git -C "$ROOT" rev-parse --short=12 HEAD)"
if ! git -C "$ROOT" diff --quiet HEAD; then
  sha="${sha}-dirty"
fi
build_id="${sha}-$(date -u +%Y%m%dT%H%M%SZ)"

dest="${DOWNLOAD_DIR}/grok-tkanarsky-${build_id}"
src="${ROOT}/target/release/xai-grok-pager"

echo "Building xai-grok-pager (release)..." >&2
cargo build --release -p xai-grok-pager-bin --bin xai-grok-pager

if [[ ! -f "$src" ]]; then
  echo "error: expected binary missing: $src" >&2
  exit 1
fi

mkdir -p "$DOWNLOAD_DIR" "$BIN_DIR"

# Copy via a sibling tmp so a crash mid-copy never leaves a truncated dest.
tmp="${dest}.tmp.$$"
cp "$src" "$tmp"
chmod +x "$tmp"
mv -f "$tmp" "$dest"

# Relative symlink when bin/ and downloads/ are siblings (default layout).
# Relative links survive Docker bind-mounts with a different $HOME.
if [[ "$(dirname "$BIN_DIR")" == "$(dirname "$DOWNLOAD_DIR")" ]]; then
  link_target="../$(basename "$DOWNLOAD_DIR")/$(basename "$dest")"
else
  link_target="$dest"
fi

ln -sfn "$link_target" "$BIN_DIR/grok"
ln -sfn "$link_target" "$BIN_DIR/agent"

# Persist auto_update=false. Unset defaults to true on first launch, which
# would replace these symlinks with the channel binary.
config_file="${GROK_HOME}/config.toml"
if [[ ! -f "$config_file" ]]; then
  printf '[cli]\nauto_update = false\n' > "$config_file"
elif grep -q '^\[cli\]' "$config_file"; then
  config_tmp="${config_file}.tmp.$$"
  awk '
    /^\[cli\][[:space:]]*(#.*)?$/ {
      print
      print "auto_update = false"
      in_cli=1
      next
    }
    /^\[.*\][[:space:]]*(#.*)?$/ { in_cli=0 }
    in_cli && /^[[:space:]]*auto_update[[:space:]]*=/ { next }
    { print }
  ' "$config_file" > "$config_tmp" && mv "$config_tmp" "$config_file"
else
  printf '\n[cli]\nauto_update = false\n' >> "$config_file"
fi

echo "Installed ${dest}" >&2
echo "  ${BIN_DIR}/grok -> ${link_target}" >&2
echo "  ${BIN_DIR}/agent -> ${link_target}" >&2
echo "  auto_update = false in ${config_file}" >&2
if command -v readelf >/dev/null 2>&1; then
  elf_id="$(readelf -n "$dest" 2>/dev/null | awk '/Build ID:/ { print $3; exit }')"
  if [[ -n "${elf_id}" ]]; then
    echo "  ELF Build ID: ${elf_id}" >&2
  fi
fi
