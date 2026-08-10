#!/usr/bin/env bash
# DicomWatch — install desktop entry and icon for application menu
# Run from the extracted release directory: ./install.sh
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN="$APP_DIR/dicom-watch"
ICON_SRC="$APP_DIR/icon.png"

if [ ! -f "$BIN" ]; then
    echo "ERROR: dicom-watch binary not found at $BIN"
    echo "Run this script from the directory containing dicom-watch."
    exit 1
fi

echo "==> Installing DicomWatch to application menu..."

# --- .desktop entry ---
APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$APPS_DIR"

cat > "$APPS_DIR/dicom-watch.desktop" << EOF
[Desktop Entry]
Type=Application
Name=DicomWatch
Comment=DICOM study zip file watcher
Exec=$BIN
Path=$APP_DIR
Icon=dicom-watch
Categories=Utility;Medical;
Terminal=false
StartupNotify=false
EOF

echo "   .desktop → $APPS_DIR"

# --- Icons (freedesktop hicolor theme) ---
ICONS_BASE="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
if [ -f "$ICON_SRC" ]; then
    for size in 16 32 48 256; do
        d="$ICONS_BASE/${size}x${size}/apps"
        mkdir -p "$d"
        cp "$ICON_SRC" "$d/dicom-watch.png"
    done
    echo "   icons → $ICONS_BASE"
else
    echo "   (no icon.png — using system default)"
fi

# --- Update caches ---
update-desktop-database "$APPS_DIR" 2>/dev/null || true
if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache "$ICONS_BASE" 2>/dev/null || true
fi

echo "==> Done. DicomWatch is now in your application menu."
