#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "${SKIP_PREFLIGHT:-0}" == "1" ]]; then
    echo "Skipping release preflight (SKIP_PREFLIGHT=1)."
    exit 0
fi

if [[ -n "$(git status --porcelain)" ]]; then
    echo "Release preflight failed: working tree is not clean."
    exit 1
fi

if ! command -v gh >/dev/null 2>&1; then
    echo "Release preflight failed: GitHub CLI (gh) is not installed."
    exit 1
fi

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
if [[ -z "$version" ]]; then
    echo "Release preflight failed: could not read version from Cargo.toml."
    exit 1
fi

if ! rg -q "^## ${version//./\\.}\\b" CHANGELOG.md; then
    echo "Release preflight failed: CHANGELOG.md missing entry for ${version}."
    exit 1
fi

if ! rg -q "release version=\"${version}\"" data/com.lumennode.app.metainfo.xml; then
    echo "Release preflight failed: metainfo release version mismatch (${version})."
    exit 1
fi

echo "Running make check..."
make check

if git tag | rg -q "^v${version}\$"; then
    echo "Release preflight failed: tag v${version} already exists."
    exit 1
fi

echo "Release preflight passed for v${version}."
