# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## graphify

This project has a graphify knowledge graph at graphify-out/.

Rules:
- Before answering architecture or codebase questions, read graphify-out/GRAPH_REPORT.md for god nodes and community structure
- If graphify-out/wiki/index.md exists, navigate it instead of reading raw files
- After modifying code files in this session, run `graphify update .` to keep the graph current (AST-only, no API cost)

## Behavioral Guidelines

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

### 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

### 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

### 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

### 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

## Commands

```bash
make build      # cargo build
make run        # cargo run
make check      # cargo check (fast type-check, no binary)
make clean      # cargo clean
```

No test suite exists yet. `cargo check` is the fastest correctness check.

The `PKG_CONFIG_PATH` in the Makefile is required for GTK4/libadwaita linking on this system — always use `make` rather than bare `cargo` commands to ensure it is set.

## Architecture

**LumenNode** is a GTK4/libadwaita desktop image gallery written in Rust. The entire UI is built imperatively in `build_ui()` in [src/main.rs](src/main.rs) — there is no `.ui` file or GtkBuilder XML.

### Module responsibilities

| Module | Role |
|---|---|
| [src/main.rs](src/main.rs) | UI construction, event wiring, all GTK state. `build_ui()` is the god node (38 edges). |
| [src/scanner.rs](src/scanner.rs) | Background thread: 2-phase directory scan (enumerate → enrich). Communicates to UI via `async-channel`. |
| [src/db.rs](src/db.rs) | Per-folder SQLite (`.lumen-node.db`). Caches SHA-256 hash + metadata. Staleness check on mtime+size. |
| [src/thumbnails.rs](src/thumbnails.rs) | Freedesktop thumbnail spec (`$XDG_CACHE_HOME/thumbnails/`). Two stores: MD5-URI named (spec-compliant) and hash-named (`lumen-node/` subdir). |
| [src/metadata.rs](src/metadata.rs) | Format-dispatched metadata extraction: EXIF for JPEG/TIFF, PNG `tEXt` chunks for AI-gen images (A1111, ComfyUI, InvokeAI). |
| [src/config.rs](src/config.rs) | `~/.lumen-node/config.yml` — plain-text KV file. Stores last folder, pane sizes, sort key, thumbnail size. |

### Data flow

```
User picks folder
  → scan_directory() [background thread]
      Phase 1: emit ScanMessage::ImageEnumerated  →  UI inserts placeholder rows
      Phase 2: db::ensure_indexed_with_outcome()  →  emit ScanMessage::ImageEnriched → UI updates row with hash+meta
  → thumbnail loading: async per-image, skipped during full-preview to avoid jank
```

### Key design decisions

- **Message-driven UI updates**: scanner and thumbnail workers communicate over `async-channel`; all GTK mutations happen on the main thread via `glib::MainContext::default().spawn_local()`.
- **Generation counter**: `scan_directory()` takes a `generation: u64`. Stale messages from a previous scan are discarded by comparing generation IDs, preventing races when the user switches folders quickly.
- **Thumbnail staleness**: thumbnails are validated by comparing stored `Thumb::MTime` against current file mtime. Invalid thumbnails are regenerated.
- **Per-folder DB**: each scanned directory gets its own `.lumen-node.db` SQLite file (WAL mode). This avoids a central index and keeps the DB close to the images.
- **AI image metadata**: PNG `tEXt` chunks are parsed for Automatic1111 `"parameters"`, ComfyUI `"prompt"`/`"workflow"`, and InvokeAI `"invokeai_metadata"` keys.
- **Progress bar**: three-phase weighted progress: enumeration (10%), thumbnail (35%), enrichment (55%).
