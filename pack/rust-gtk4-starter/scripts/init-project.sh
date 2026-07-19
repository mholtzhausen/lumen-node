#!/usr/bin/env bash
# Copy the starter template to a new directory and rename identifiers.
set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "Usage: $0 <ProjectName> <destination-dir> [com.example.AppId]"
    echo "Example: $0 MyGallery ../my-gallery com.example.MyGallery"
    exit 1
fi

PROJECT_NAME="$1"
DEST="$2"
APP_ID="${3:-com.example.${PROJECT_NAME}}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ -e "$DEST" ]]; then
    echo "Destination already exists: $DEST"
    exit 1
fi

BINARY="$(echo "$PROJECT_NAME" | tr '[:upper:]' '[:lower:]' | tr -cd '[:alnum:]-' | tr ' ' '-')"
if [[ -z "$BINARY" ]]; then
    echo "Could not derive binary name from: $PROJECT_NAME"
    exit 1
fi

cp -a "$SRC_DIR" "$DEST"

OLD_ID="com.example.GtkStarter"
OLD_BINARY="gtk-starter"
OLD_CRATE="gtk-starter"
OLD_TITLE="GTK Starter"

find "$DEST" -type f \( -name '*.rs' -o -name '*.toml' -o -name '*.md' -o -name '*.xml' -o -name '*.desktop' -o -name 'Makefile' -o -name '*.sh' -o -name '*.svg' \) -print0 \
    | while IFS= read -r -d '' f; do
        sed -i \
            -e "s/${OLD_ID}/${APP_ID}/g" \
            -e "s/${OLD_BINARY}/${BINARY}/g" \
            -e "s/${OLD_CRATE}/${BINARY}/g" \
            -e "s/${OLD_TITLE}/${PROJECT_NAME}/g" \
            "$f"
    done

mv "$DEST/data/${OLD_ID}.desktop" "$DEST/data/${APP_ID}.desktop"
mv "$DEST/data/${OLD_ID}.metainfo.xml" "$DEST/data/${APP_ID}.metainfo.xml"
mv "$DEST/data/icons/${OLD_ID}.svg" "$DEST/data/icons/${APP_ID}.svg"

rm -rf "$DEST/target" "$DEST/Cargo.lock" 2>/dev/null || true

echo "Created project at: $DEST"
echo "  application id: $APP_ID"
echo "  binary name:    $BINARY"
echo ""
echo "Next:"
echo "  cd $DEST && make check && make run"
