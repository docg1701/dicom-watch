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
ICON_DEST="${XDG_DATA_HOME:-$HOME/.local/share}/icons/dicom-watch.png"
if [ -f "$ICON_SRC" ]; then
    mkdir -p "$(dirname "$ICON_DEST")"
    cp "$ICON_SRC" "$ICON_DEST"
    echo "   icon → $ICON_DEST"
    # Point .desktop directly to the file — no theme lookup
    sed -i "s|^Icon=.*|Icon=$ICON_DEST|" "$APPS_DIR/dicom-watch.desktop"
else
    echo "   (no icon.png — using system default)"
fi

# --- Update caches ---
update-desktop-database "$APPS_DIR" 2>/dev/null || true

echo "==> Done. DicomWatch is now in your application menu."
