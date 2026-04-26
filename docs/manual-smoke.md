# Manual Smoke Checklist

Run this checklist after structural changes and before release.

## Setup

1. Start app: `make run`
2. Open a folder with a mix of image formats (`jpg`, `png`, `webp`).

## Scan and Grid

1. Verify scan progress appears and reaches completion.
2. Verify grid placeholders fill with thumbnails progressively.
3. Change sort modes (name/date/size, asc/desc) and confirm ordering updates.
4. Use search and confirm list filters and clears correctly.

## Metadata and Actions

1. Select images and verify metadata panel updates.
2. For AI PNGs, verify prompt/seed/generation command copy actions are available.
3. Rename one image and verify filename updates in grid/list.
4. Delete one image and verify it disappears from the current view.

## Navigation and Persistence

1. Toggle between grid and full preview and back.
2. Open another folder, then return using recent folders.
3. Restart app and verify last folder and UI state restore.

## Release Artifacts

1. Run `make check`.
2. Run `make appimage`.
3. Confirm `packaging/LumenNode-x86_64.AppImage` exists.
