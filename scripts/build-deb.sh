#!/usr/bin/env bash
set -euo pipefail

# Build a .deb package for claudio
# Usage: ./scripts/build-deb.sh <version>
# Expects target/release/claudio to already be built and stripped.

VERSION="${1:?Usage: build-deb.sh <version>}"
ARCH="$(dpkg --print-architecture)"
PKG_NAME="claudio"
PKG_DIR="dist/${PKG_NAME}_${VERSION}_${ARCH}"

rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR/DEBIAN"
mkdir -p "$PKG_DIR/usr/bin"
mkdir -p "$PKG_DIR/usr/share/claudio/ml_service"
mkdir -p "$PKG_DIR/usr/share/doc/claudio"
mkdir -p "$PKG_DIR/usr/lib/systemd/user"

# Binary
cp target/release/claudio "$PKG_DIR/usr/bin/claudio"

# ML service
cp -r ml_service/src ml_service/pyproject.toml "$PKG_DIR/usr/share/claudio/ml_service/"

# Docs
cp LICENSE "$PKG_DIR/usr/share/doc/claudio/copyright"
cp README.md "$PKG_DIR/usr/share/doc/claudio/"

# Systemd user service
cat > "$PKG_DIR/usr/lib/systemd/user/claudio.service" << 'EOF'
[Unit]
Description=Claudio - Voice-activated terminal session manager
After=graphical-session.target

[Service]
Type=simple
ExecStart=/usr/bin/claudio start --foreground
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF

# Control file
INSTALLED_SIZE=$(du -sk "$PKG_DIR" | cut -f1)
cat > "$PKG_DIR/DEBIAN/control" << EOF
Package: ${PKG_NAME}
Version: ${VERSION}
Architecture: ${ARCH}
Maintainer: PFigs
Description: Voice-activated terminal session manager for Claude Code
 Speak to your focused session, hear responses via TTS, manage multiple
 sessions with independent audio modes.
Depends: libasound2, libxkbcommon0, libvulkan1, mesa-vulkan-drivers, curl
Recommends: mesa-vulkan-drivers
Section: utils
Priority: optional
Homepage: https://github.com/PFigs/claudio
Installed-Size: ${INSTALLED_SIZE}
EOF

# Post-install script
cat > "$PKG_DIR/DEBIAN/postinst" << 'POSTINST'
#!/bin/bash
set -e

# Install uv if not present
if ! command -v uv >/dev/null 2>&1; then
    echo "claudio: installing uv for Python ML service..."
    curl -LsSf https://astral.sh/uv/install.sh | sh || true
fi

# Sync ML service dependencies
ML_DIR="/usr/share/claudio/ml_service"
if [ -d "$ML_DIR" ] && command -v uv >/dev/null 2>&1; then
    echo "claudio: syncing ML service dependencies..."
    cd "$ML_DIR" && uv sync || true
fi

echo ""
echo "claudio installed successfully."
echo ""
echo "Post-install steps:"
echo "  1. Add yourself to the input group: sudo usermod -aG input \$USER"
echo "  2. Log out and back in"
echo "  3. Run: claudio setup"
echo ""
POSTINST
chmod 755 "$PKG_DIR/DEBIAN/postinst"

# Build
mkdir -p dist
dpkg-deb --build "$PKG_DIR"

echo "Built: dist/${PKG_NAME}_${VERSION}_${ARCH}.deb"
