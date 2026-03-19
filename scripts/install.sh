#!/usr/bin/env bash
set -euo pipefail

# claudio installer
# Usage: curl -sSL https://raw.githubusercontent.com/PFigs/claudio/main/scripts/install.sh | bash

REPO="PFigs/claudio"
INSTALL_DIR="${CLAUDIO_INSTALL_DIR:-$HOME/.local}"
BIN_DIR="$INSTALL_DIR/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/claudio"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[claudio]${NC} $*"; }
warn()  { echo -e "${YELLOW}[claudio]${NC} $*"; }
error() { echo -e "${RED}[claudio]${NC} $*" >&2; }
die()   { error "$@"; exit 1; }

check_command() {
    command -v "$1" >/dev/null 2>&1
}

detect_arch() {
    local arch
    arch="$(uname -m)"
    case "$arch" in
        x86_64)  echo "x86_64-unknown-linux-gnu" ;;
        aarch64) echo "aarch64-unknown-linux-gnu" ;;
        *)       die "Unsupported architecture: $arch" ;;
    esac
}

# --- Pre-flight checks ---

info "Checking prerequisites..."

# OS check
if [ "$(uname -s)" != "Linux" ]; then
    die "claudio only supports Linux."
fi

# Check for required system packages
MISSING_PKGS=()

if ! check_command curl; then
    MISSING_PKGS+=("curl")
fi

# Check for system libraries needed at runtime
check_lib() {
    ldconfig -p 2>/dev/null | grep -q "$1" || return 1
}

if ! check_lib libxkbcommon.so; then
    MISSING_PKGS+=("libxkbcommon-dev")
fi

if ! check_lib libvulkan.so; then
    MISSING_PKGS+=("libvulkan-dev" "mesa-vulkan-drivers")
fi

if ! check_lib libasound.so; then
    MISSING_PKGS+=("libasound2-dev")
fi

if [ ${#MISSING_PKGS[@]} -gt 0 ]; then
    warn "Missing system packages: ${MISSING_PKGS[*]}"
    warn "Install them with:"
    echo ""
    echo "  sudo apt install ${MISSING_PKGS[*]}"
    echo ""
    die "Install missing packages and re-run the installer."
fi

# Check for uv
if ! check_command uv; then
    info "Installing uv..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
    export PATH="$HOME/.local/bin:$PATH"
    if ! check_command uv; then
        die "Failed to install uv. Install it manually: https://docs.astral.sh/uv/getting-started/installation/"
    fi
fi

# Check for claude CLI
if ! check_command claude; then
    warn "claude CLI not found in PATH."
    warn "Install it: https://docs.anthropic.com/en/docs/claude-code"
    warn "claudio will work without it, but sessions won't be able to talk to Claude."
fi

# --- Determine install method ---

ARCH="$(detect_arch)"

install_from_release() {
    info "Downloading latest release..."

    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    # Get latest release tag
    local latest
    latest="$(curl -sSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | head -1 | cut -d'"' -f4)"

    if [ -z "$latest" ]; then
        return 1
    fi

    local tarball="claudio-${latest}-${ARCH}.tar.gz"
    local url="https://github.com/$REPO/releases/download/${latest}/${tarball}"

    info "Downloading $tarball..."
    if ! curl -fSL "$url" -o "$tmpdir/$tarball"; then
        return 1
    fi

    tar xzf "$tmpdir/$tarball" -C "$tmpdir"

    mkdir -p "$BIN_DIR"
    cp "$tmpdir/claudio" "$BIN_DIR/claudio"
    chmod +x "$BIN_DIR/claudio"

    # Install ML service
    mkdir -p "$DATA_DIR"
    if [ -d "$tmpdir/ml_service" ]; then
        cp -r "$tmpdir/ml_service" "$DATA_DIR/"
    fi

    return 0
}

install_from_source() {
    info "Building from source..."

    if ! check_command cargo; then
        info "Installing Rust toolchain..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi

    if ! check_command git; then
        die "git is required to build from source."
    fi

    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    info "Cloning repository..."
    git clone --depth 1 "https://github.com/$REPO.git" "$tmpdir/claudio"

    info "Building (this may take a few minutes)..."
    cargo build --release --manifest-path "$tmpdir/claudio/Cargo.toml"

    mkdir -p "$BIN_DIR"
    cp "$tmpdir/claudio/target/release/claudio" "$BIN_DIR/claudio"
    chmod +x "$BIN_DIR/claudio"

    # Install ML service
    mkdir -p "$DATA_DIR"
    cp -r "$tmpdir/claudio/ml_service" "$DATA_DIR/"
}

# Try release binary first, fall back to source build
if ! install_from_release 2>/dev/null; then
    warn "No pre-built release found, building from source..."
    install_from_source
fi

# --- Set up Python ML service ---

info "Setting up ML service with uv..."
if [ -d "$DATA_DIR/ml_service" ]; then
    (cd "$DATA_DIR/ml_service" && uv sync)
else
    warn "ML service directory not found at $DATA_DIR/ml_service"
    warn "Voice features won't work until ml_service is installed."
fi

# --- Desktop entry & icons ---

APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
mkdir -p "$APPS_DIR"

# Desktop file and icons are either in DATA_DIR (release) or from source checkout
ASSETS_DIR=""
if [ -d "$DATA_DIR/assets" ]; then
    ASSETS_DIR="$DATA_DIR/assets"
elif [ -d "$DATA_DIR/../assets" ]; then
    ASSETS_DIR="$DATA_DIR/../assets"
fi

if [ -n "$ASSETS_DIR" ] && [ -f "$ASSETS_DIR/claudio.desktop" ]; then
    cp "$ASSETS_DIR/claudio.desktop" "$APPS_DIR/"

    for size in 48 128 256; do
        icon_dir="$ICONS_DIR/${size}x${size}/apps"
        mkdir -p "$icon_dir"
        if [ -f "$ASSETS_DIR/icon-${size}.png" ]; then
            cp "$ASSETS_DIR/icon-${size}.png" "$icon_dir/claudio.png"
        fi
    done

    if command -v update-desktop-database >/dev/null; then
        update-desktop-database "$APPS_DIR" 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null; then
        gtk-update-icon-cache "$ICONS_DIR" 2>/dev/null || true
    fi

    info "Installed desktop entry and icons."
else
    warn "Assets not found — desktop entry and icons not installed."
    warn "The app will still work from the command line."
fi

# --- Post-install ---

# Check PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    warn "$BIN_DIR is not in your PATH."
    warn "Add it to your shell config:"
    echo ""
    echo "  export PATH=\"$BIN_DIR:\$PATH\""
    echo ""
fi

# Check input group
if ! groups | grep -qw input; then
    warn "You are not in the 'input' group (needed for push-to-talk)."
    warn "Run: sudo usermod -aG input \$USER"
    warn "Then log out and back in."
fi

info "Installed claudio to $BIN_DIR/claudio"
info ""
info "Get started:"
info "  claudio setup    # download ML models"
info "  claudio          # start daemon + GUI"
info ""
info "Done."
