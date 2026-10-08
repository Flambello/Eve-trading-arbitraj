#!/usr/bin/env bash
# EVE Trade Scout - GUI Launcher Script
set -e

# Resolve script directory (handles symlinks and spaces in paths)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Ensure Rust environment is loaded if compilation is required
if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

BINARY="$SCRIPT_DIR/target/release/eve-trade"

# Automatically compile release binary if missing
if [ ! -f "$BINARY" ]; then
    echo "Release binary not found. Building EVE Trade Scout..."
    cargo build --release
fi

# Launch the native desktop graphical interface (GUI window)
exec "$BINARY" --gui "$@"
