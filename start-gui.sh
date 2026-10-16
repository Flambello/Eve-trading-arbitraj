#!/usr/bin/env bash
# EVE Traveling Merchant - GUI Launcher Script
set -e

# Resolve script directory (handles symlinks and spaces in paths)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Ensure Rust environment is loaded if compilation is required
if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

if [ -f "$SCRIPT_DIR/eve-merchant" ]; then
    BINARY="$SCRIPT_DIR/eve-merchant"
else
    BINARY="$SCRIPT_DIR/target/release/eve-merchant"

    # Check if release binary is missing or any source file is newer than the binary
    NEEDS_BUILD=false
    if [ ! -f "$BINARY" ]; then
        NEEDS_BUILD=true
    elif [ -n "$(find "$SCRIPT_DIR/src" "$SCRIPT_DIR/Cargo.toml" "$SCRIPT_DIR/Cargo.lock" -type f -newer "$BINARY" 2>/dev/null)" ]; then
        NEEDS_BUILD=true
    fi

    if [ "$NEEDS_BUILD" = true ]; then
        if command -v notify-send >/dev/null 2>&1; then
            notify-send "EVE Traveling Merchant" "Compiling updated release binary..." --icon=applications-games || true
        fi
        cargo build --release --bin eve-merchant
    fi
fi

# Launch the native desktop graphical interface (GUI window)
exec "$BINARY" "$@"

