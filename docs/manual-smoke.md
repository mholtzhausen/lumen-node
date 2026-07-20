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

## Multiselect and Batch Editor

1. Ctrl+click two thumbnails → right pane shows batch editor (not metadata).
2. Ctrl+A selects all filtered; Esc collapses to last item in batch list sort order.
3. Plain click one thumb exits batch; chrome (fav/tags) is hidden while multi-selected.
4. Toggle a mixed tag (confirm dialog) and favourite-all; verify grid icons update.
5. Batch rename with `test_{index:2}`; confirm preview and that Apply disables on collision.
6. Right-click a selected item in multi → selection-management menu; right-click unselected → single menu and exits batch.
7. Confirm Similar / double-click full view do nothing while multi-selected.

## Navigation and Persistence

1. Toggle between grid and full preview and back.
2. Pin an image for compare (context menu), confirm left stays fixed while Left/Right changes the right pane; Escape exits to single, then to grid.
3. Open another folder, then return using recent folders.
4. Restart app and verify last folder and UI state restore.

## Release Artifacts

1. Run `make check`.
2. Run `make appimage`.
3. Confirm `packaging/LumenNode-x86_64.AppImage` exists.
