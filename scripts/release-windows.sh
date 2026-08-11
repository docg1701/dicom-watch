#!/usr/bin/env bash
# Usage: ./scripts/release-windows.sh vX.Y.Z
# Cross-compiles Windows binary, packages into zip, uploads to GitHub Release.
set -euo pipefail

TAG="${1:-}"
if [ -z "$TAG" ]; then
  echo "Usage: $0 vX.Y.Z"
  exit 1
fi

TARGET="x86_64-pc-windows-gnu"
BIN="target/${TARGET}/release/dicom-watch.exe"

echo "==> Cross-compiling Windows binary (${TARGET})..."
cargo build --release --target "${TARGET}"

if [ ! -f "$BIN" ]; then
  echo "ERROR: binary not found at $BIN"
  exit 1
fi

echo "==> Binary size: $(du -h "$BIN" | cut -f1)"

ZIP="dicom-watch-${TAG}-windows-x86_64.zip"
rm -f "$ZIP"

# Copy binary temporarily so zip -j flattens the archive.
cp "$BIN" /tmp/dicom-watch.exe
zip -j "$ZIP" \
    /tmp/dicom-watch.exe \
    config.toml.example \
    assets/alarm-001.wav \
    assets/unmaximize.wav
rm /tmp/dicom-watch.exe

echo "==> Uploading $ZIP to GitHub Release $TAG..."
gh release upload "$TAG" "$ZIP" --clobber

echo "==> Done: $(ls -lh "$ZIP" | awk '{print $5, $NF}')"
