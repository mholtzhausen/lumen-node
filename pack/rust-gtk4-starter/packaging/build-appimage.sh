#!/usr/bin/env bash
# AppImage skeleton — install linuxdeploy + linuxdeploy-plugin-gtk, then bundle.
# See LumenNode packaging/ for a full working script (not vendored in git).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

APP_ID="$(grep '^APP_ID' Makefile | sed 's/.*:= *//')"
BINARY="$(grep '^name' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')"

echo "Building release binary..."
make -C "$ROOT_DIR" check
PKG_CONFIG_PATH="${PKG_CONFIG_PATH:-/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig}"
export PKG_CONFIG_PATH
cargo build --release

APPDIR="$ROOT_DIR/packaging/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/metainfo"
mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/apps"

install -Dm755 "target/release/$BINARY" "$APPDIR/usr/bin/$BINARY"
install -Dm644 "data/${APP_ID}.desktop" "$APPDIR/usr/share/applications/${APP_ID}.desktop"
install -Dm644 "data/${APP_ID}.metainfo.xml" "$APPDIR/usr/share/metainfo/${APP_ID}.metainfo.xml"
install -Dm644 "data/icons/${APP_ID}.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/${APP_ID}.svg"

cat > "$APPDIR/AppRun" <<'EOF'
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "$0")")"
export PATH="${HERE}/usr/bin:${PATH}"
export LD_LIBRARY_PATH="${HERE}/usr/lib:${HERE}/usr/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}"
exec "${HERE}/usr/bin/"* 2>/dev/null || exec "${HERE}/usr/bin/gtk-starter" "$@"
EOF
chmod +x "$APPDIR/AppRun"

echo "AppDir staged at $APPDIR"
echo "Next: run linuxdeploy with --appdir and gtk plugin to produce .AppImage"
