#!/usr/bin/env bash
set -euo pipefail

# Install script for claudio on Ubuntu
# Usage: curl -fsSL https://raw.githubusercontent.com/PFigs/claudio/main/install.sh | bash

REPO="https://github.com/PFigs/claudio.git"
INSTALL_DIR="$HOME/.local/share/claudio"
BIN_DIR="$HOME/.local/bin"

info()  { printf '\033[1;34m::\033[0m %s\n' "$*"; }
error() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# ── Prerequisites ──────────────────────────────────────────────────────

info "Checking prerequisites..."

command -v git  >/dev/null || error "git is required. Install with: sudo apt install git"
command -v curl >/dev/null || error "curl is required. Install with: sudo apt install curl"

# Rust toolchain
if ! command -v cargo >/dev/null; then
    info "Installing Rust toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# uv (Python package manager for ML service)
if ! command -v uv >/dev/null; then
    info "Installing uv..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
    export PATH="$HOME/.local/bin:$PATH"
fi

# System libraries
info "Installing system dependencies (may prompt for sudo)..."
sudo apt-get update -qq
sudo apt-get install -y -qq \
    build-essential \
    pkg-config \
    libasound2-dev \
    libvulkan-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev \
    mesa-vulkan-drivers \
    libwayland-dev \
    libssl-dev

# ── Clone / update ─────────────────────────────────────────────────────

if [ -d "$INSTALL_DIR/.git" ]; then
    info "Updating existing installation..."
    git -C "$INSTALL_DIR" pull --ff-only
else
    info "Cloning claudio..."
    git clone "$REPO" "$INSTALL_DIR"
fi

# ── Build ──────────────────────────────────────────────────────────────

info "Building claudio (release)..."
cargo build --release --manifest-path "$INSTALL_DIR/Cargo.toml"

# ── Install binary ─────────────────────────────────────────────────────

mkdir -p "$BIN_DIR"
cp "$INSTALL_DIR/target/release/claudio" "$BIN_DIR/claudio"

# Ensure ~/.local/bin is on PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$BIN_DIR"; then
    info "Adding $BIN_DIR to PATH in your shell profile..."
    for rc in "$HOME/.bashrc" "$HOME/.profile"; do
        if [ -f "$rc" ] && ! grep -q 'export PATH="$HOME/.local/bin' "$rc"; then
            echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$rc"
        fi
    done
    export PATH="$BIN_DIR:$PATH"
fi

# ── Desktop entry & icons ─────────────────────────────────────────────

APPS_DIR="$HOME/.local/share/applications"
ICONS_DIR="$HOME/.local/share/icons/hicolor"
mkdir -p "$APPS_DIR"

cp "$INSTALL_DIR/assets/claudio.desktop" "$APPS_DIR/"

for size in 48 128 256; do
    icon_dir="$ICONS_DIR/${size}x${size}/apps"
    mkdir -p "$icon_dir"
    cp "$INSTALL_DIR/assets/icon-${size}.png" "$icon_dir/claudio.png"
done

if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$APPS_DIR" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache "$ICONS_DIR" 2>/dev/null || true
fi

# ── Input group ────────────────────────────────────────────────────────

if ! id -nG | grep -qw input; then
    info "Adding $USER to 'input' group (needed for push-to-talk)..."
    sudo usermod -aG input "$USER"
    info "You'll need to log out and back in for the group change to take effect."
fi

# ── Done ───────────────────────────────────────────────────────────────

info "Installed claudio to $BIN_DIR/claudio"
info "Run 'claudio' to start (first run downloads ML models)."
