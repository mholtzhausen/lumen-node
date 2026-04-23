# Architecture

This document describes how the LumenNode codebase is laid out today: where UI lives, how scans flow into the GTK main thread, and how configuration splits between global YAML and per-folder SQLite.

## High-level layout

- **`src/main.rs`** — Composition root: `build_ui()` assembles modules under `ui::` and `core::`, owns scan progress state (`ScanProgressState`), global flags such as `SUPPRESS_SIDEBAR_DURING_PREVIEW`, and wires `glib::spawn_local` receivers. It is no longer the only place with GTK logic; most widgets and handlers live under `src/ui/`.
- **`src/ui/`** — Presentation and interaction: shell/chrome, grid and preview, context menus and actions, keyboard, layout, scan runtime (channel drain), navigation, selection (including click-to-preview timing), session helpers, and wiring glue.
- **`src/core/`** — Cross-cutting coordination without owning widgets: `app_state` (startup state from config + folder DB), `scan_coordinator` (folder switches, scan generation IDs, coordination with grid defer flags).
- **`src/services/`** — Background services (for example release/update checking) used from lifecycle wiring.
- **`src/scanner.rs`** — Background `std::thread` that walks a folder and enriches via `db`; sends **`ScanMessage`** variants defined in **`src/scan.rs`** over a bounded `async-channel`.
- **`src/db.rs`** — Per-folder `.lumen-node.db`: `images` rows, **`ui_state`** key-value rows (`sort_key`, `search_text`, `thumbnail_size`), WAL + `synchronous=NORMAL`.
- **`src/metadata.rs`**, **`src/thumbnails.rs`**, **`src/thumbnail_sizing.rs`** — Extraction, cache paths, and discrete thumbnail size steps.
- **`src/config.rs`** — `~/.lumen-node/config.yml` load/save for window, panes, recent folders, etc.; **`external_editor`** and optional startup defaults (`sort_key`, `search_text`, `thumbnail_size`) are loaded but not rewritten by `config::save` (hand-editing only for those defaults).
- **`src/dialogs.rs`**, **`src/metadata_section.rs`**, **`src/metadata_view.rs`**, **`src/view_helpers.rs`**, **`src/tree_sidebar.rs`**, etc. — Shared dialogs and helpers still at crate root where not yet folded into `ui/`.

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
4. **`ui::scan_runtime`** drains messages on the main context in **idle-priority batches** (batch size from `main.rs`, e.g. 50), updates list models and progress. Stale generations are dropped.
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
- Manual smoke flows (if present under `docs/manual-smoke.md`) — folder scan, metadata, rename/delete, grid vs single view, persisted UI state.
