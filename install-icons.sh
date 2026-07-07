#!/usr/bin/env bash
# install-icons.sh — Install app icon and .desktop launcher
# Run this from inside the desktop-icon-manager folder after building.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ASSETS_DIR="$SCRIPT_DIR/assets"
BINARY="$SCRIPT_DIR/target/release/desktop-icon-manager"
INSTALL_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
HICOLOR="$HOME/.local/share/icons/hicolor"
PIXMAPS="$HOME/.local/share/pixmaps"
ICON_NAME="desktop-icon-manager"

echo "➤  Installing icons…"

if [ ! -d "$ASSETS_DIR" ]; then
    echo "✗  assets/ folder not found — make sure you extracted the full tarball."
    exit 1
fi

# Install at every standard size
for SIZE in 16 32 48 64 128 256 512; do
    SRC="$ASSETS_DIR/icon_${SIZE}.png"
    if [ -f "$SRC" ]; then
        DEST="$HICOLOR/${SIZE}x${SIZE}/apps/$ICON_NAME.png"
        mkdir -p "$(dirname "$DEST")"
        cp "$SRC" "$DEST"
    fi
done

# SVG (scales perfectly)
if [ -f "$ASSETS_DIR/icon.svg" ]; then
    mkdir -p "$HICOLOR/scalable/apps"
    cp "$ASSETS_DIR/icon.svg" "$HICOLOR/scalable/apps/$ICON_NAME.svg"
fi

# Pixmaps fallback
mkdir -p "$PIXMAPS"
cp "$ASSETS_DIR/icon_256.png" "$PIXMAPS/$ICON_NAME.png"

# Refresh icon cache
if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -f -t "$HICOLOR" 2>/dev/null || true
fi
echo "✓  Icons installed"

# Install binary
echo "➤  Installing binary…"
if [ -f "$BINARY" ]; then
    mkdir -p "$INSTALL_DIR"
    cp "$BINARY" "$INSTALL_DIR/"
    echo "✓  Binary copied to $INSTALL_DIR/desktop-icon-manager"
else
    echo "⚠  Binary not found at $BINARY — run 'cargo build --release' first"
fi

# Create .desktop launcher
echo "➤  Creating launcher…"
mkdir -p "$DESKTOP_DIR"
cat > "$DESKTOP_DIR/$ICON_NAME.desktop" << EOF
[Desktop Entry]
Name=Desktop Icon Manager
Comment=Manage icons for Linux .desktop application entries
Exec=$INSTALL_DIR/desktop-icon-manager
Icon=$ICON_NAME
Terminal=false
Type=Application
Categories=Settings;DesktopSettings;Utility;
Keywords=icon;desktop;application;launcher;
StartupNotify=true
StartupWMClass=desktop-icon-manager
EOF
chmod +x "$DESKTOP_DIR/$ICON_NAME.desktop"
update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
echo "✓  Launcher created: $DESKTOP_DIR/$ICON_NAME.desktop"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Done! The app icon should appear in your dock."
echo "  If not, log out and back in once."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
