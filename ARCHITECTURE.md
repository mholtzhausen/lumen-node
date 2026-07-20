# Architecture

This document describes how the LumenNode codebase is laid out today: where UI lives, how scans flow into the GTK main thread, and how configuration splits between global YAML and per-folder SQLite.

## High-level layout

- **`src/main.rs`** — Composition root: `build_ui()` assembles modules under `ui::` and `core::`, owns scan progress state (`ScanProgressState`), global flags such as `SUPPRESS_SIDEBAR_DURING_PREVIEW`, and wires `glib::spawn_local` receivers. It is no longer the only place with GTK logic; most widgets and handlers live under `src/ui/`.
- **`src/ui/`** — Presentation and interaction: shell/chrome, grid and preview, context menus and actions, keyboard, layout, scan runtime (channel drain), navigation, selection (including click-to-preview timing), session helpers, and wiring glue. Preview / single-view zoom lives in **`ui::zoom`** (display-only CSS scale on `GtkPicture`, Ctrl+scroll and `+/-`/`0`, fade-out level HUD). Grid cards overlay a right-hand chrome pane (favourite + quick-tag `MenuButton`; **`ui::quick_tag`**) that appears on hover/selection/favourite; pane button size is live-scaled via `thumbnail_chrome_scale` (Preferences → Appearance). The image grid uses **`gtk4::MultiSelection`** (`ui::models`); when `N≥2`, the right sidebar `Stack` shows **`ui::batch_editor`** (summary, copy, tri-state favourite/tags, sortable list, batch rename) instead of preview+metadata; thumbnail chrome is suppressed. The center `ViewStack` has **`grid`**, **`single`**, and **`compare`** pages; compare is a horizontal `gtk4::Paned` with two zoomed pictures (left = `AppState.pinned_compare_path`, right = selection; Left/Right and scroll advance the right pane only). Tabbed preferences live in **`ui::preferences`** (`adw::PreferencesDialog`: General / Appearance / Startup / Tags; modal overlay with left sidebar).
- **`src/icons.rs`** — Registers bundled hicolor symbolics (notably **`lumen-tag-symbolic`**) on the GTK icon theme search path for dev, install, and AppImage.
- **`src/core/`** — Cross-cutting coordination without owning widgets: `app_state` (startup state from config + folder DB, including `pinned_compare_path` for compare mode, `selected_path` / `selected_paths` mirrors, `batch_list_sort_key`, plus in-memory `prompt_similarity_index` / `similar_paths` for temporary similar-browse filtering), `scan_coordinator` (folder switches, scan generation IDs, coordination with grid defer flags; clears the compare pin and similarity index/filter on folder change).
- **`src/view_helpers.rs`** — Selection helpers (`selected_image_paths`, `select_only_path`, batch order, Esc collapse) shared by keyboard, actions, and the batch editor.
- **`src/file_name_ops.rs`** — Single and batch rename stem expansion (`{index}` / `{index:N}`) and collision checks.
- **`src/similarity.rs`** — Prompt-token normalization, Jaccard scoring (+ same-seed boost), and helpers to build/query the in-memory index used by `ctx.show-similar`.
- **`src/services/`** — Background services (for example release/update checking) used from lifecycle wiring.
- **`src/scanner.rs`** — Background `std::thread` that walks a folder and enriches via `db`; sends **`ScanMessage`** variants defined in **`src/scan.rs`** over a bounded `async-channel`.
- **`src/db.rs`** — Per-folder `.lumen-node.db`: `images` rows, free-form **`image_tags(path, tag)`** junction (+ index on `tag`), **`ui_state`** key-value rows (`sort_key`, `search_text`, `favorites_only`, `active_tags` JSON array, `thumbnail_size`), WAL + `synchronous=NORMAL`.
- **`src/metadata.rs`**, **`src/thumbnails.rs`**, **`src/thumbnail_sizing.rs`** — Extraction, cache paths, and discrete thumbnail size steps.
- **`src/config.rs`** — `~/.lumen-node/config.yml` load/save for window, panes, recent folders, sidebar visibility, **`meta_section_expanded`** (Metadata expander open/closed), **`color_scheme`** (`system` / `light` / `dark`), **`thumbnail_chrome_scale`**, etc. Full session `save()` still omits preference-only keys; those are updated via partial writers (`save_color_scheme`, `save_recent_state`, `save_external_editor`, `save_full_view_favourite_prefs`, `save_thumbnail_chrome_scale`, `save_startup_defaults`) that read/replace key families without clobbering unknown lines. Theme preference is applied through `adw::StyleManager::set_color_scheme` from the header toggle / Preferences Appearance tab in `src/ui/shell.rs` + `src/ui/preferences.rs` (and on startup). Single/full view wraps the picture in an overlay for the timed favourite star HUD (`ui::grid::FullViewFavouriteHud`).
- **`src/dialogs.rs`**, **`src/metadata_section.rs`**, **`src/metadata_view.rs`**, **`src/view_helpers.rs`**, **`src/tree_sidebar.rs`**, etc. — Shared dialogs and helpers still at crate root where not yet folded into `ui/`. Tag assignment uses `dialogs::open_add_tag_dialog`, grid quick-tag popovers in **`ui::quick_tag`**, and filtering/search inclusion in `ui::models` / `ui::controls` with `AppState.tags_cache` + `active_tags`.

## Folder tree vs gallery folder

- **Tree root** — first entry of the sidebar `ListStore` (`tree_sidebar::reset_tree_root` / `tree_root_path`). Set by Open Folder, recent history, session restore, or **double-click / Enter** on a tree row (`ListView::activate` in `ui::tree`). Persisted as `last_folder` in config.
- **Current folder** — `AppState.current_folder`, what the thumbnail grid scans. **Single-click** on a tree row browses here without changing the tree root; `last_folder` stays the root.

## Dependency direction (typical)

```
main
  → ui::{shell, layout, chrome, center, wiring, lifecycle, scan_runtime, …}
  → core::{app_state, scan_coordinator}
scanner → db → {metadata, thumbnails}
scan_runtime (UI) → drains async-channel → models / caches / progress
```

Workers must not call GTK APIs; they send messages; the main thread applies updates inside `glib::spawn_local` and idle handlers.

## Scan and UI data flow

1. User picks a folder; **`scan_coordinator`** starts **`scanner::scan_directory`** with a new generation id.
2. Worker enumerates files, then for each image calls **`db::ensure_indexed_with_outcome`** (hash, metadata, thumbnails on miss).
3. **`ScanMessage`** crosses an **`async_channel::bounded`** queue (capacity 200) for backpressure.
4. **`ui::scan_runtime`** drains messages on the main context in **idle-priority batches** (batch size from `main.rs`, e.g. 50), updates list models and progress, and upserts the prompt similarity index on `ImageEnriched`. Stale generations are dropped. The grid `CustomFilter` ANDs favourites / tags / search with an optional `similar_paths` set from *Similar in folder*.
5. Progress text is three phases (enum / thumbs / index) with weighted bar fractions defined beside `ScanProgressState` in `main.rs`.

## Thumbnails

- **Freedesktop** cache under `$XDG_CACHE_HOME/thumbnails/normal/` (and related spec paths) keyed by MD5 of `file://` URI.
- **Content-hash** store under `$XDG_CACHE_HOME/thumbnails/lumen-node/` for deduplication and non-default sizes.

Discrete sizes come from **`thumbnail_sizing::thumbnail_size_options()`** (128 px base and larger stepped sizes).

## Updates

**`src/updater.rs`** uses **`ureq`** to call the GitHub releases API. The repository URL in that module must match the real GitHub project before in-app update checks return useful results.

## Historical note

An earlier “Phase 0” baseline described extraction from a monolithic `main.rs`. Much of the UI has since moved under **`src/ui/`**; this file reflects the current tree. For user-facing behaviour and keyboard shortcuts, prefer **`README.md`**.

## Verification

- `make check` — type-check without producing a full binary.
- Manual smoke flows (if present under `docs/manual-smoke.md`) — folder scan, metadata, rename/delete, grid vs single vs compare view, persisted UI state.
