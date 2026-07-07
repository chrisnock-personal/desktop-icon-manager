#!/usr/bin/env bash
# build.sh — Build Desktop Icon Manager from source
# Supports: Debian/Ubuntu (apt), Fedora/RHEL/CentOS (dnf/yum),
#           openSUSE (zypper), Arch Linux (pacman), Void Linux (xbps)
set -euo pipefail

BINARY_NAME="desktop-icon-manager"
INSTALL_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICONS_BASE="$HOME/.local/share/icons/hicolor"
PIXMAPS_DIR="$HOME/.local/share/pixmaps"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ASSETS_DIR="$SCRIPT_DIR/assets"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Desktop Icon Manager — Build Script"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# ── 1. Detect distro & set package manager ───────────────────────────────────
echo "➤  Detecting Linux distribution…"

PKG_MANAGER=""
DISTRO_NAME=""

detect_distro() {
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        DISTRO_NAME="${NAME:-unknown}"
        local id_like="${ID_LIKE:-}"
        local id="${ID:-}"
        for token in $id $id_like; do
            case "$token" in
                debian|ubuntu|mint|pop|elementary|kali|parrot|zorin|linuxmint)
                    PKG_MANAGER="apt";    return ;;
                fedora|rhel|centos|rocky|almalinux|ol|amzn)
                    if command -v dnf &>/dev/null; then PKG_MANAGER="dnf"
                    elif command -v yum &>/dev/null; then PKG_MANAGER="yum"; fi
                    return ;;
                opensuse*|sles|sled)
                    PKG_MANAGER="zypper"; return ;;
                arch|manjaro|endeavouros|garuda|cachyos)
                    PKG_MANAGER="pacman"; return ;;
                void)
                    PKG_MANAGER="xbps";  return ;;
            esac
        done
    fi
    if command -v apt-get      &>/dev/null; then PKG_MANAGER="apt";    return; fi
    if command -v dnf          &>/dev/null; then PKG_MANAGER="dnf";    return; fi
    if command -v yum          &>/dev/null; then PKG_MANAGER="yum";    return; fi
    if command -v zypper       &>/dev/null; then PKG_MANAGER="zypper"; return; fi
    if command -v pacman       &>/dev/null; then PKG_MANAGER="pacman"; return; fi
    if command -v xbps-install &>/dev/null; then PKG_MANAGER="xbps";  return; fi
}

detect_distro

if [ -z "$PKG_MANAGER" ]; then
    echo "⚠  Could not detect a supported package manager."
    echo "   Please install libxcb, libxkbcommon, openssl, fontconfig dev headers"
    echo "   manually, then re-run with SKIP_DEPS=1 ./build.sh"
    echo ""
else
    echo "   Distro  : ${DISTRO_NAME:-unknown}"
    echo "   Manager : $PKG_MANAGER"
fi
echo ""

# ── 2. Install system dependencies ──────────────────────────────────────────
install_deps() {
    echo "➤  Checking / installing system libraries…"
    case "$PKG_MANAGER" in
        apt)
            PKGS=(libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
                  libxkbcommon-dev libssl-dev libfontconfig1-dev)
            MISSING=()
            for pkg in "${PKGS[@]}"; do
                dpkg -s "$pkg" &>/dev/null || MISSING+=("$pkg")
            done
            if [ ${#MISSING[@]} -gt 0 ]; then
                echo "   Installing: ${MISSING[*]}"
                sudo apt-get install -y "${MISSING[@]}"
            else
                echo "   All libraries already present."
            fi ;;
        dnf|yum)
            PKGS=(libxcb-devel libxkbcommon-devel openssl-devel fontconfig-devel)
            echo "   Running: sudo $PKG_MANAGER install -y ${PKGS[*]}"
            sudo "$PKG_MANAGER" install -y "${PKGS[@]}" ;;
        zypper)
            PKGS=(libxcb-devel libxkbcommon-devel libopenssl-devel fontconfig-devel)
            echo "   Running: sudo zypper install --no-confirm ${PKGS[*]}"
            sudo zypper install --no-confirm "${PKGS[@]}" ;;
        pacman)
            PKGS=(libxcb libxkbcommon openssl fontconfig)
            echo "   Running: sudo pacman -S --needed --noconfirm ${PKGS[*]}"
            sudo pacman -S --needed --noconfirm "${PKGS[@]}" ;;
        xbps)
            PKGS=(libxcb-devel libxkbcommon-devel openssl-devel fontconfig-devel)
            echo "   Running: sudo xbps-install -y ${PKGS[*]}"
            sudo xbps-install -y "${PKGS[@]}" ;;
        *)
            echo "⚠  Unhandled package manager — skipping (build may fail)." ;;
    esac
    echo ""
}

if [ "${SKIP_DEPS:-0}" = "1" ]; then
    echo "➤  SKIP_DEPS=1 — skipping dependency installation."
    echo ""
elif [ -n "$PKG_MANAGER" ]; then
    install_deps
fi

# ── 3. Check for Rust/Cargo ──────────────────────────────────────────────────
if ! command -v cargo &>/dev/null; then
    echo "⚠  Rust toolchain not found. Install with:"
    echo "     curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "     source \"\$HOME/.cargo/env\""
    echo "   Then re-run this script."
    exit 1
fi
echo "✓  Rust $(rustc --version)"
echo ""

# ── 4. Build ──────────────────────────────────────────────────────────────────
echo "➤  Building release binary (this may take a few minutes on first run)…"
cd "$SCRIPT_DIR"
cargo build --release

BINARY="$SCRIPT_DIR/target/release/$BINARY_NAME"
echo ""
echo "✓  Built: $BINARY  ($(du -sh "$BINARY" | cut -f1))"
echo ""

# ── 5. Install binary ────────────────────────────────────────────────────────
read -r -p "➤  Install binary to $INSTALL_DIR? [Y/n]: " REPLY
REPLY="${REPLY:-Y}"
if [[ $REPLY =~ ^[Yy]$ ]]; then
    mkdir -p "$INSTALL_DIR"
    cp "$BINARY" "$INSTALL_DIR/"
    echo "✓  Copied binary to $INSTALL_DIR/$BINARY_NAME"
    if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
        echo ""
        echo "   NOTE: $INSTALL_DIR is not in your PATH."
        echo "   Add to ~/.bashrc or ~/.zshrc:"
        echo "     export PATH=\"\$HOME/.local/bin:\$PATH\""
        echo ""
    fi
fi

# ── 6. Install icon files ─────────────────────────────────────────────────────
# File managers and app launchers resolve icons from the XDG icon theme, not
# from the binary itself. We install into ~/.local/share/icons/hicolor/<size>/apps/
# at every standard size, plus ~/.local/share/pixmaps as a fallback.
#
# After this, any .desktop file with Icon=desktop-icon-manager will display
# our icon in Nautilus, Nemo, Thunar, KDE Dolphin, etc.
echo "➤  Installing icon files…"

ICON_NAME="desktop-icon-manager"
INSTALLED_ICON=""

for SIZE in 16 32 48 64 128 256 512; do
    SRC="$ASSETS_DIR/icon_${SIZE}.png"
    if [ -f "$SRC" ]; then
        DEST_DIR="$ICONS_BASE/${SIZE}x${SIZE}/apps"
        mkdir -p "$DEST_DIR"
        cp "$SRC" "$DEST_DIR/$ICON_NAME.png"
        # Track the largest installed for the .desktop fallback path
        INSTALLED_ICON="$DEST_DIR/$ICON_NAME.png"
    fi
done

# Pixmaps fallback (used by some older DEs and Nautilus when hicolor isn't found)
if [ -f "$ASSETS_DIR/icon_256.png" ]; then
    mkdir -p "$PIXMAPS_DIR"
    cp "$ASSETS_DIR/icon_256.png" "$PIXMAPS_DIR/$ICON_NAME.png"
fi

# Also copy SVG if present (scales perfectly at any size)
if [ -f "$ASSETS_DIR/icon.svg" ]; then
    SVG_DIR="$ICONS_BASE/scalable/apps"
    mkdir -p "$SVG_DIR"
    cp "$ASSETS_DIR/icon.svg" "$SVG_DIR/$ICON_NAME.svg"
    echo "✓  Installed SVG icon to $SVG_DIR/$ICON_NAME.svg"
fi

# Refresh the icon theme cache so file managers pick up the new icon immediately
if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -f -t "$ICONS_BASE" 2>/dev/null || true
    echo "✓  Icon cache refreshed (gtk-update-icon-cache)"
elif command -v xdg-icon-resource &>/dev/null; then
    xdg-icon-resource forceupdate 2>/dev/null || true
    echo "✓  Icon cache refreshed (xdg-icon-resource)"
else
    echo "   (icon cache not refreshed — gtk-update-icon-cache not found)"
    echo "   Log out and back in if the icon doesn't appear immediately."
fi
echo ""

# ── 7. Create launcher .desktop file ─────────────────────────────────────────
read -r -p "➤  Create application launcher (.desktop file)? [Y/n]: " REPLY2
REPLY2="${REPLY2:-Y}"
if [[ $REPLY2 =~ ^[Yy]$ ]]; then
    mkdir -p "$DESKTOP_DIR"
    cat > "$DESKTOP_DIR/$BINARY_NAME.desktop" <<EOF
[Desktop Entry]
Name=Desktop Icon Manager
Comment=Manage icons for Linux .desktop application entries
Exec=$INSTALL_DIR/$BINARY_NAME
Icon=$ICON_NAME
Terminal=false
Type=Application
Categories=Settings;DesktopSettings;Utility;
Keywords=icon;desktop;application;launcher;
StartupNotify=true
StartupWMClass=desktop-icon-manager
EOF
    chmod +x "$DESKTOP_DIR/$BINARY_NAME.desktop"
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
    echo "✓  Launcher created: $DESKTOP_DIR/$BINARY_NAME.desktop"
    echo "   Icon= set to theme name: $ICON_NAME"
    echo "   (resolves from ~/.local/share/icons/hicolor/)"
fi

# ── 8. Also mark the binary itself as executable with the right MIME type ─────
# Some file managers (Nautilus) show an icon for executables only when they
# have a matching .desktop file — which we've now created above.
# Force Nautilus/GIO to re-read the desktop db:
if command -v gio &>/dev/null; then
    gio mime application/x-executable 2>/dev/null || true
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  All done! Run with:"
echo "    $BINARY_NAME"
echo "  or:"
echo "    $BINARY"
echo ""
echo "  If the icon doesn't show immediately in your"
echo "  file manager, try logging out and back in."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
