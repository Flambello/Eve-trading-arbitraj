#!/usr/bin/env bash
# EVE Trade Scout - Startup Script (Launches Desktop GUI)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/start-gui.sh" "$@"
