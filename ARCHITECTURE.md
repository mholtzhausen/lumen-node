# Architecture Baseline (Phase 0)

This document captures the current structure before deeper refactoring.
Phase 0 is intentionally behavior-preserving: extraction and documentation only.

## Goals

- Freeze a shared understanding of current module boundaries.
- Continue extraction from `src/main.rs` without changing runtime behavior.
- Define guardrails for subsequent phases.

## Current Layout

- `src/main.rs`: composition root and primary GTK orchestration surface.
- `src/dialogs.rs`: rename/delete dialog flows extracted from `main`.
- `src/metadata_section.rs`: metadata-content visibility and split-pane state handling.
- `src/timing_report.rs`: timing-report sink abstraction (currently no-op).
- `src/scanner.rs`: background scan worker and message emission.
- `src/db.rs`: per-folder SQLite cache and indexing.
- `src/metadata.rs`: image metadata extraction and AI metadata parsing.
- `src/thumbnails.rs`: thumbnail generation/cache handling.
- `src/config.rs`: app-level persisted config.

## Dependency Direction (Current)

- UI orchestration: `main -> {dialogs, metadata_section, scanner, db, metadata, thumbnails, config, ...}`
- Data/scan path: `scanner -> db -> {metadata, thumbnails}`
- Presentation helpers: `main -> metadata_view`, `main -> view_helpers`

`main` remains the highest-coupled module by design for now; Phase 1+ will reduce it.

## Phase 0 Guardrails

- No user-visible behavior changes.
- No action name or keyboard shortcut changes.
- No schema or persistence format changes.
- No async/concurrency policy changes.
- Prefer pure moves/extractions with minimal call-site edits.

## Verification for Phase 0

- Build/type-check succeeds with `make check`.
- Manual smoke flows remain green (`docs/manual-smoke.md`):
  - scan progress and grid population
  - metadata panel updates
  - rename/delete actions
  - grid/single-view navigation
  - folder restore and persisted UI state

## Exit Criteria for Phase 0

- Extracted modules compile and are wired through `main`.
- Architectural baseline is documented (this file).
- Repository is ready for Phase 1 UI decomposition (`ui/shell`, `ui/grid`, `ui/preview`, `ui/sidebar`, `ui/actions`).
