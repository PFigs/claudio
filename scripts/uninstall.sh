#!/usr/bin/env bash
set -euo pipefail

# claudio uninstaller

BIN_DIR="${CLAUDIO_INSTALL_DIR:-$HOME/.local}/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/claudio"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/ok_claude"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/ok_claude"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[claudio]${NC} $*"; }
warn()  { echo -e "${YELLOW}[claudio]${NC} $*"; }

# Stop daemon if running
if command -v claudio >/dev/null 2>&1; then
    info "Stopping daemon..."
    claudio stop 2>/dev/null || true
fi

# Remove binary
if [ -f "$BIN_DIR/claudio" ]; then
    rm "$BIN_DIR/claudio"
    info "Removed $BIN_DIR/claudio"
fi

# Remove data (ML service, models)
if [ -d "$DATA_DIR" ]; then
    rm -rf "$DATA_DIR"
    info "Removed $DATA_DIR"
fi

# Ask about config
if [ -d "$CONFIG_DIR" ]; then
    echo ""
    read -rp "Remove configuration at $CONFIG_DIR? [y/N] " answer
    if [[ "$answer" =~ ^[Yy]$ ]]; then
        rm -rf "$CONFIG_DIR"
        info "Removed $CONFIG_DIR"
    else
        info "Kept $CONFIG_DIR"
    fi
fi

# Remove state
if [ -d "$STATE_DIR" ]; then
    rm -rf "$STATE_DIR"
    info "Removed $STATE_DIR"
fi

# Remove piper voices
PIPER_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/piper"
if [ -d "$PIPER_DIR" ]; then
    echo ""
    read -rp "Remove downloaded Piper voices at $PIPER_DIR? [y/N] " answer
    if [[ "$answer" =~ ^[Yy]$ ]]; then
        rm -rf "$PIPER_DIR"
        info "Removed $PIPER_DIR"
    else
        info "Kept $PIPER_DIR"
    fi
fi

info "Uninstall complete."
