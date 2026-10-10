#!/usr/bin/env bash
# Generate cargo-sources.json for Flathub offline Flatpak builds
# Uses the official flatpak-builder-tools cargo generator.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$SCRIPT_DIR"

GENERATOR_URL="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py"
GENERATOR_SCRIPT="$SCRIPT_DIR/.flatpak-cargo-generator.py"

if [ ! -f "$GENERATOR_SCRIPT" ]; then
    echo "==> Downloading official flatpak-cargo-generator.py..."
    curl -fsSL "$GENERATOR_URL" -o "$GENERATOR_SCRIPT"
    chmod +x "$GENERATOR_SCRIPT"
fi

echo "==> Generating cargo-sources.json from Cargo.lock..."
if command -v uv >/dev/null 2>&1; then
    uv run --with aiohttp --with tomlkit python3 "$GENERATOR_SCRIPT" "$ROOT_DIR/Cargo.lock" -o "$SCRIPT_DIR/cargo-sources.json"
elif command -v python3 >/dev/null 2>&1; then
    python3 "$GENERATOR_SCRIPT" "$ROOT_DIR/Cargo.lock" -o "$SCRIPT_DIR/cargo-sources.json"
else
    echo "Error: Neither uv nor python3 found" >&2
    exit 1
fi

echo "==> Done! Output written to: $SCRIPT_DIR/cargo-sources.json"
