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

BINARY="$SCRIPT_DIR/target/release/eve-merchant"

# Ensure release binary is always compiled and up to date
cargo build --release --bin eve-merchant

# Launch the native desktop graphical interface (GUI window)
exec "$BINARY" --gui "$@"
