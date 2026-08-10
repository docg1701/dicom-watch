#!/usr/bin/env bash
# DicomWatch — remove desktop entry and icon
set -euo pipefail

APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons"

echo "==> Removing DicomWatch from application menu..."

rm -f "$APPS_DIR/dicom-watch.desktop"
echo "   removed $APPS_DIR/dicom-watch.desktop"

rm -f "$ICONS_DIR/dicom-watch.png"
echo "   removed $ICONS_DIR/dicom-watch.png"

update-desktop-database "$APPS_DIR" 2>/dev/null || true

echo "==> Done."
