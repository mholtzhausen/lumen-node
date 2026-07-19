#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ -n "$(git status --porcelain 2>/dev/null || true)" ]]; then
    echo "Preflight failed: working tree is not clean."
    exit 1
fi

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
if [[ -z "$version" ]]; then
    echo "Preflight failed: could not read version from Cargo.toml."
    exit 1
fi

APP_ID="$(grep '^APP_ID' Makefile | sed 's/.*:= *//')"
if ! rg -q "release version=\"${version}\"" "data/${APP_ID}.metainfo.xml"; then
    echo "Preflight failed: metainfo release version mismatch (${version})."
    exit 1
fi

echo "Running make check..."
make check
echo "Preflight passed for v${version}."
