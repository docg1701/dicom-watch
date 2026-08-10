#!/usr/bin/env bash
# Usage: ./scripts/release.sh vX.Y.Z
# Packages the release binary + config.toml.example into a zip and uploads to
# the GitHub Release (which must already exist — created by CI on tag push).
set -euo pipefail

TAG="${1:-}"
if [ -z "$TAG" ]; then
  echo "Usage: $0 vX.Y.Z"
  exit 1
fi

echo "==> Building release binary..."
cargo build --release

BIN="target/release/dicom-watch"
if [ ! -f "$BIN" ]; then
  echo "ERROR: binary not found at $BIN"
  exit 1
fi

echo "==> Binary size: $(du -h "$BIN" | cut -f1)"

ZIP="dicom-watch-${TAG}-linux-x86_64.zip"
rm -f "$ZIP"
zip -j "$ZIP" "$BIN" config.toml.example

echo "==> Uploading $ZIP to GitHub Release $TAG..."
gh release upload "$TAG" "$ZIP" --clobber

echo "==> Done: $(ls -lh "$ZIP" | awk '{print $5, $NF}')"
