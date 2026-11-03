#!/bin/bash
# Install Symphony worker skills into CODEX_HOME/skills (default ~/.codex/skills).
set -euo pipefail
SRC="$(cd "$(dirname "$0")/../codex-skills" && pwd)"
DEST="${CODEX_HOME:-$HOME/.codex}/skills"
mkdir -p "$DEST"
for dir in "$SRC"/symphony-*/; do
    name="$(basename "$dir")"
    rm -rf "${DEST:?}/$name"
    cp -R "$dir" "$DEST/$name"
done
echo "Installed Symphony skills into $DEST"
