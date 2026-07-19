#!/usr/bin/env bash
set -euo pipefail

APPDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/AppDir"
test -d "$APPDIR" || { echo "Missing AppDir — run packaging/build-appimage.sh first"; exit 1; }

required=(
  AppRun
  usr/bin
  usr/share/applications
  usr/share/icons/hicolor/scalable/apps
)

for path in "${required[@]}"; do
  if [[ ! -e "$APPDIR/$path" ]]; then
    echo "audit failed: missing $APPDIR/$path"
    exit 1
  fi
done

echo "AppDir audit passed."
