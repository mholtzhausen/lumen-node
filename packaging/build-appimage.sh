#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$SCRIPT_DIR"

# Download linuxdeploy and GTK plugin if not already present
if [ ! -f linuxdeploy-x86_64.AppImage ]; then
    echo "Downloading linuxdeploy..."
    wget -q --show-progress \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
    chmod +x linuxdeploy-x86_64.AppImage
fi

if [ ! -f linuxdeploy-plugin-gtk.sh ]; then
    echo "Downloading linuxdeploy-plugin-gtk..."
    wget -q --show-progress \
        "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh"
    chmod +x linuxdeploy-plugin-gtk.sh
fi

# Build release binary
echo "Building release binary..."
PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig \
    cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml"

# Assemble AppDir
rm -rf AppDir
mkdir -p AppDir/usr/share/metainfo

cp "$REPO_ROOT/data/com.lumennode.app.metainfo.xml" AppDir/usr/share/metainfo/

# Run linuxdeploy with GTK plugin
echo "Assembling AppImage..."
DEPLOY_GTK_VERSION=4 \
    ./linuxdeploy-x86_64.AppImage \
    --appdir AppDir \
    --plugin gtk \
    --executable "$REPO_ROOT/target/release/lumen-node" \
    --desktop-file "$REPO_ROOT/data/com.lumennode.app.desktop" \
    --icon-file "$REPO_ROOT/data/icons/com.lumennode.app.svg" \
    --output appimage

echo "Done. AppImage written to packaging/"
