# Project Guidelines

## Code Style
- Keep changes minimal and localized; avoid broad refactors unless requested.
- Follow existing module boundaries:
  - `src/main.rs`: GTK/libadwaita UI composition and event wiring
  - `src/scanner.rs`: directory scan and indexing dispatch
  - `src/db.rs`: per-folder SQLite cache (`.lumen-node.db`)
  - `src/metadata.rs`: format-specific metadata extraction
  - `src/thumbnails.rs`: thumbnail cache paths/validation/generation
  - `src/config.rs`: persisted UI state in `~/.lumen-node/config.yml`
- Prefer non-blocking UI behavior: expensive work should run off the GTK main thread and report back through channels.

## Architecture
- Data flow is message-driven:
  1. Folder selection triggers `scan_directory(...)`
  2. Scanner indexes images and emits `ScanMessage`
  3. UI receiver updates `gio::ListStore` + in-memory caches
  4. `FilterListModel` + `SortListModel` drive the visible grid
- Per-folder cache database lives at `<folder>/.lumen-node.db`.
- Thumbnail cache uses Freedesktop locations under `$XDG_CACHE_HOME` (or `~/.cache`).

## Build and Test
- Preferred commands:
  - `make check`
  - `make build`
  - `make run`
- If running cargo commands directly, ensure GTK pkg-config paths are available:
  - `PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig`
- There is currently no automated test suite in this workspace.

## Conventions
- Keep search/sort behavior consistent with current keys:
  - `name_asc`, `name_desc`, `date_asc`, `date_desc`, `size_asc`, `size_desc`
- Metadata shown in UI should be safely escaped before rendering as subtitle text.
- For cache correctness, treat file `mtime` + `size` as staleness checks before re-indexing metadata/hash.
- Preserve persisted user state updates through `config::save(...)` when changing related UI features.

## Existing Project Notes
- Context-menu feature planning notes are in `.plan/context-menu.md`.
