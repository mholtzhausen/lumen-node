<div align="center">

```
██╗     ██╗   ██╗███╗   ███╗███████╗███╗   ██╗    ███╗   ██╗ ██████╗ ██████╗ ███████╗
██║     ██║   ██║████╗ ████║██╔════╝████╗  ██║    ████╗  ██║██╔═══██╗██╔══██╗██╔════╝
██║     ██║   ██║██╔████╔██║█████╗  ██╔██╗ ██║    ██╔██╗ ██║██║   ██║██║  ██║█████╗
██║     ██║   ██║██║╚██╔╝██║██╔══╝  ██║╚██╗██║    ██║╚██╗██║██║   ██║██║  ██║██╔══╝
███████╗╚██████╔╝██║ ╚═╝ ██║███████╗██║ ╚████║    ██║ ╚████║╚██████╔╝██████╔╝███████╗
╚══════╝ ╚═════╝ ╚═╝     ╚═╝╚══════╝╚═╝  ╚═══╝    ╚═╝  ╚═══╝ ╚═════╝ ╚═════╝ ╚══════╝
```

**A fast, beautiful image gallery for photographers and AI artists — built in Rust with GTK4.**

[![Rust](https://img.shields.io/badge/Rust-2021_Edition-CE422B?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![GTK4](https://img.shields.io/badge/GTK-4.0-4A86CF?style=flat-square&logo=gnome)](https://gtk.org/)
[![libadwaita](https://img.shields.io/badge/libadwaita-0.7-78ACD1?style=flat-square)](https://gnome.pages.gitlab.gnome.org/libadwaita/)
[![License: MIT](https://img.shields.io/badge/License-MIT-22AA55?style=flat-square)](LICENSE)

</div>

---

## TL;DR

> Browse thousands of images without waiting. LumenNode loads thumbnails progressively, caches metadata in a per-folder SQLite database, and surfaces AI-generation prompts, seeds, and workflow details from PNG images — so you can copy a prompt and re-run a generation in one click.

- **What**: GTK4 desktop image gallery for Linux
- **Who**: Photographers and AI artists (Stable Diffusion, ComfyUI, InvokeAI)
- **Why**: Most galleries don't understand AI metadata. LumenNode does.
- **How fast**: Folder opens instantly. Thumbnails load in the background. Metadata is cached.
- **Key power**: Right-click → *Copy Prompt / Copy Seed / Copy Generation Command*

```bash
make build && make run    # that's it
```

---

## Contents

- [Screenshots](#screenshots)
- [Features](#features)
- [Installation](#installation)
- [Building from Source](#building-from-source)
- [Usage](#usage)
- [AI Metadata Support](#ai-metadata-support)
- [Keyboard Shortcuts](#keyboard-shortcuts)
- [Configuration](#configuration)
- [Architecture](#architecture)
- [Internals Deep-Dive](#internals-deep-dive)
- [Roadmap](#roadmap)

---

## Features

### The essentials

| | Feature |
|---|---|
| ⚡ | **Progressive loading** — folder opens instantly, thumbnails fill in behind |
| 🖼️ | **Grid + single-view** — grid for browsing, click for focus, Escape to return |
| 📐 | **4 thumbnail sizes** — 128 → 245px, adjustable in one click |
| 🔍 | **Live search** — filters grid by filename and metadata in real time |
| ↕️ | **6 sort modes** — name, date, size (ascending and descending) |
| 💾 | **Session persistence** — last folder, sort, search, pane layout all saved |

### For AI artists

| | Feature |
|---|---|
| 🤖 | **Auto1111 / SD WebUI** — extracts prompt, seed, steps, sampler from PNG `parameters` |
| 🌊 | **ComfyUI** — parses `prompt` JSON (positive + negative) and `workflow` node summaries |
| 🎨 | **InvokeAI** — reads `invokeai_metadata` JSON for structured prompt + model info |
| 📋 | **One-click copy** — prompt, negative prompt, seed, or full generation command |
| 🔁 | **CLI reconstruction** — right-click → *Copy Generation Command* builds `--prompt "..." --negative "..." --seed X` |

### For photographers

| | Feature |
|---|---|
| 📷 | **EXIF extraction** — camera make/model, exposure time, ISO sensitivity |
| 📁 | **Folder tree sidebar** — navigate directories without a file manager |
| 🗄️ | **Per-folder cache** — `.lumen-node.db` lives next to your images, no central index |
| ♻️ | **Refresh controls** — force re-thumbnail or re-index single images or entire folders |

---

## Installation

### From AppImage (recommended)

```bash
chmod +x LumenNode-x86_64.AppImage
./LumenNode-x86_64.AppImage
```

### Install to `~/.local` (desktop integration)

```bash
git clone https://github.com/your-username/lumen-node
cd lumen-node
make install
```

This installs the binary, `.desktop` file, SVG icon, and AppData metainfo so LumenNode appears in your application launcher.

---

## Building from Source

### Prerequisites

```bash
# Debian / Ubuntu
sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config build-essential

# Fedora
sudo dnf install gtk4-devel libadwaita-devel pkg-config

# Arch
sudo pacman -S gtk4 libadwaita pkg-config
```

You also need a stable Rust toolchain:

```bash
curl https://sh.rustup.rs -sSf | sh
```

### Build

```bash
make build      # debug build
make check      # fast type-check, no binary produced
make run        # build + run
make clean      # wipe build artifacts
```

> **Always use `make`**, not bare `cargo`. The Makefile sets `PKG_CONFIG_PATH` required for GTK4/libadwaita linking.

---

## Usage

### Opening a folder

Click **Open Folder** in the header bar, or simply launch the app — it reopens the last folder you browsed.

### Navigating

```
Grid view                Single view
─────────────────────    ─────────────────────
Click image     → focus  Left / Right  → prev/next
Page Up/Down    → page   Escape        → back to grid
Home / End      → ends
```

### The context menu

Right-click any image thumbnail, the single-view image, or the sidebar preview to get:

```
Prompt Actions ──────────────
  Copy Prompt
  Copy Negative Prompt
  Copy Seed
  Copy Generation Command        ← rebuilds CLI invocation
─────────────────────────────
Clipboard ───────────────────
  Copy Image                     ← pixel data to clipboard
  Copy Path
  Copy Metadata
─────────────────────────────
Refresh ─────────────────────
  Refresh Thumbnail
  Refresh Metadata
  Refresh Folder Thumbnails
  Refresh Folder Metadata
```

### Pane layout

```
┌──────────────────────────────────────────────────────┐
│  [Open Folder]  │  [Sort ▾] [Search ___] [×]  │ ◧ ◨ │  ← header
├──────────┬──────────────────────────┬───────────────┤
│  Folder  │                          │  Metadata     │
│  Tree    │   Image Grid / Preview   │  Sidebar      │
│          │                          │               │
│          ├──────────────────────────┤               │
│          │   Sidebar Detail / Meta  │               │
├──────────┴──────────────────────────┴───────────────┤
│  ████████████████░░░░  Enum 412/412 | Thumbs 201/412 │  ← progress
└──────────────────────────────────────────────────────┘
```

The `◧` and `◨` buttons in the header toggle the folder tree and metadata panes. All three dividers are draggable; proportions are saved on exit.

---

## AI Metadata Support

LumenNode reads PNG `tEXt` chunks written by AI generation tools. No configuration needed — it detects the format automatically.

### Automatic1111 / Stable Diffusion WebUI

```
tEXt chunk key: "parameters"

a beautiful landscape, oil painting style
Negative prompt: ugly, blurry, low quality
Steps: 20, Sampler: DPM++ 2M Karras, CFG scale: 7,
Seed: 1234567890, Size: 512x512, Model: v1-5-pruned
```

Extracted: **positive prompt**, **seed** (from `Seed: NNN`), raw parameters block.

### ComfyUI

```
tEXt chunk key: "prompt"    ← node graph JSON (API format)
tEXt chunk key: "workflow"  ← UI workflow JSON
```

For `"prompt"` JSON, LumenNode finds all `CLIPTextEncode` nodes and identifies positive/negative by text length. For `"workflow"` JSON, it generates a human-readable node summary.

### InvokeAI

```
tEXt chunk key: "invokeai_metadata"

{
  "positive_prompt": "...",
  "negative_prompt": "...",
  "model": { "name": "...", "base": "sd-1" },
  ...
}
```

Extracted: **positive prompt**, **negative prompt**, model info, raw JSON.

---

## Keyboard Shortcuts

| Key | Context | Action |
|-----|---------|--------|
| `Page Up` | Grid | Scroll one page up |
| `Page Down` | Grid | Scroll one page down |
| `Home` | Grid | Jump to first image |
| `End` | Grid | Jump to last image |
| `Escape` | Grid | Quit (toast warns, second press confirms) |
| `←` / `→` | Single view | Previous / next image |
| `Escape` | Single view | Return to grid |

---

## Configuration

Config lives at `~/.lumen-node/config.yml` — a plain-text key=value file you can edit by hand.

| Key | Default | Description |
|-----|---------|-------------|
| `last_folder` | — | Folder reopened at launch |
| `window_width` / `window_height` | 1200×800 | Window size |
| `window_maximized` | false | Maximized state |
| `left_pane_width_pct` | 15 | Folder tree width (% of window) |
| `right_pane_width_pct` | 20 | Metadata sidebar width (%) |
| `left_sidebar_visible` | true | Folder tree visibility |
| `right_sidebar_visible` | true | Metadata pane visibility |
| `sort_key` | `name_asc` | One of: `name_asc/desc`, `date_asc/desc`, `size_asc/desc` |
| `search_text` | — | Last search query |
| `thumbnail_size` | 165 | Grid thumbnail size in pixels |

Per-folder SQLite databases (`.lumen-node.db`) store cached hashes, metadata, and favourite flags alongside your images. They're safe to delete — LumenNode will regenerate them.

---

## Architecture

LumenNode is ~12,000 lines of Rust across six modules.

```
src/
├── main.rs        ~3 400 lines   UI construction, event wiring, all GTK state
├── scanner.rs       ~173 lines   Background scan thread, async-channel messaging
├── db.rs            ~289 lines   Per-folder SQLite cache, staleness validation
├── metadata.rs      ~338 lines   Format-dispatched metadata extraction
├── thumbnails.rs    ~238 lines   Freedesktop spec + content-hash thumbnail stores
├── config.rs        ~149 lines   ~/.lumen-node/config.yml read/write
└── updater.rs        ~22 lines   Version check stub
```

### Data flow

```
User selects folder
       │
       ▼
scan_directory()  ──── background thread ────────────────────────────────────┐
       │                                                                      │
  Phase 1: Enumerate (fast)                        Phase 2: Enrich (slower)  │
  ┌────────────────────┐                    ┌───────────────────────────────┐ │
  │ emit               │                    │ db::ensure_indexed()          │ │
  │ ImageEnumerated    │  ─── channel ───▶  │  ├─ cache hit (mtime+size ✓) │ │
  │ (placeholder rows) │                    │  └─ cache miss:               │ │
  └────────────────────┘                    │      SHA-256 hash             │ │
                                            │      metadata extraction      │ │
                                            │      thumbnail generation     │ │
                                            │ emit ImageEnriched            │ │
                                            └───────────────────────────────┘ │
                                                                              │
       ▼  (main thread, idle-priority batches of 50)                         │
  Receiver drains async-channel  ◀───────────────────────────────────────────┘
       │
       ├─ update ListStore (GTK data model)
       ├─ update hash_cache / meta_cache / sort_fields_cache (Rc<HashMap>)
       └─ advance progress bar (enum 10% / thumbs 35% / enrich 55%)
```

### Key design decisions

**Message-driven UI** — All GTK mutations happen on the main thread via `glib::spawn_local()`. Workers communicate over bounded `async-channel` (capacity 200) for backpressure.

**Generation tokens** — Each `scan_directory()` call gets a `u64` generation ID. Stale messages from a cancelled scan are silently discarded, preventing races when the user switches folders quickly.

**Idle-priority batching** — The receiver processes messages in batches of 50 at GTK idle priority, ensuring user input is never blocked by scan activity.

**Two-tier thumbnail storage**:
- Freedesktop spec: `$XDG_CACHE_HOME/thumbnails/normal/{md5(file://URI)}.png` — interoperable with other apps
- Content-addressed: `$XDG_CACHE_HOME/thumbnails/lumen-node/{sha256}.png` — duplicate images share one thumbnail

**Per-folder SQLite (WAL mode)** — No central index. Each scanned folder gets its own `.lumen-node.db`. Staleness is checked by comparing stored `mtime + size` against current file metadata.

---

## Internals Deep-Dive

### Module details

<details>
<summary><strong>main.rs</strong> — the god node</summary>

`build_ui()` constructs the entire window imperatively — no `.ui` XML, no GtkBuilder. It wires together:
- `GtkGridView` with `SignalListItemFactory` (custom cell renderer)
- `GtkPicture` for single-view (async decode via `gio::spawn_blocking`, cancellable)
- `TreeListModel` for the folder tree (filesystem hierarchy, dotfile-filtered)
- `FilterListModel` + `SortListModel` wrapping a `GListStore`
- Three `GtkPaned` panels with percent-based proportion restore on resize
- `GtkHeaderBar` with sort dropdown, search entry, and pane-toggle buttons
- Progress bar with three-phase weighted progress text

`build_ui()` has 39 outgoing edges in the call graph — intentionally a single large function to keep all GTK state co-located and avoid lifetime tangles.

</details>

<details>
<summary><strong>scanner.rs</strong> — two-phase scan</summary>

`scan_directory()` spawns a `std::thread` and sends `ScanMessage` variants over an `async-channel`:

```rust
pub enum ScanMessage {
    ImageEnumerated { path, filename, generation },
    ImageEnriched   { path, hash, metadata, ... },
    ThumbnailReady  { path, pixbuf },
    ScanComplete    { generation, total },
    ScanError       { message },
}
```

Phase 1 emits `ImageEnumerated` for every image found (fast — just directory iteration). Phase 2 calls `db::ensure_indexed_with_outcome()` per image and emits `ImageEnriched` with the result.

A generation counter guards against stale messages: if the user switches folders before Phase 2 completes, the old thread's messages carry the old generation ID and are discarded by the receiver.

</details>

<details>
<summary><strong>db.rs</strong> — per-folder SQLite</summary>

Schema:

```sql
CREATE TABLE images (
    path             TEXT PRIMARY KEY,
    filename         TEXT,
    hash             TEXT,       -- SHA-256 hex
    mtime            INTEGER,    -- Unix seconds
    size             INTEGER,
    favourite        INTEGER,    -- 0 | 1
    camera_make      TEXT,
    camera_model     TEXT,
    exposure         TEXT,
    iso              TEXT,
    prompt           TEXT,
    negative_prompt  TEXT,
    raw_parameters   TEXT,
    workflow_json    TEXT
);
CREATE INDEX idx_images_hash ON images(hash);
```

`ensure_indexed_with_outcome()` checks `mtime + size` for staleness. A match returns the cached row in microseconds. A mismatch triggers the full slow path: SHA-256 hash → metadata extraction → thumbnail generation → DB write.

WAL mode + `PRAGMA synchronous=NORMAL` keeps writes fast without risking corruption.

</details>

<details>
<summary><strong>metadata.rs</strong> — format dispatch</summary>

Metadata extraction is dispatched by file extension:

- **JPEG / TIFF**: `kamadak-exif` reads EXIF tags (Make, Model, ExposureTime, ISOSpeedRatings)
- **PNG**: `png` crate iterates `tEXt` chunks; key-based dispatch:
  - `"parameters"` → Auto1111 parser (regex for `Seed: NNN`, splits on `Negative prompt:`)
  - `"prompt"` → ComfyUI JSON parser (finds `CLIPTextEncode` nodes, sorts by text length)
  - `"workflow"` → ComfyUI workflow summariser (human-readable node list)
  - `"invokeai_metadata"` → InvokeAI JSON parser (direct field extraction)
  - anything else → stored as `raw_parameters`

</details>

<details>
<summary><strong>thumbnails.rs</strong> — two stores</summary>

**Freedesktop store** (`normal/`): Thumbnail path is `md5("file://" + absolute_path) + ".png"`. PNG `tEXt` chunks embed `Thumb::URI` and `Thumb::MTime`. Valid only if stored mtime matches current file mtime.

**Hash store** (`lumen-node/`): Thumbnail path is `{sha256_hash}_{size}.png`. Content-addressed: two files with identical content share one thumbnail regardless of path. Generated for non-128px sizes.

Both stores use `gdk_pixbuf::Pixbuf` for scaling, preserving aspect ratio.

</details>

<details>
<summary><strong>Performance instrumentation</strong></summary>

`main.rs` contains timing infrastructure (`ClickTrace`, `FullViewTrace`) that records step-by-step millisecond timestamps for click-to-preview and full-view-activation paths. Counters track:

- `ACTIVE_THUMBNAIL_TASKS` — capped at 64 concurrent thumbnail loads
- `SUPPRESS_SIDEBAR_DURING_PREVIEW` — skips thumbnail callbacks during full-image decode
- `DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE` — holds thumbnail dispatch until Phase 1 finishes

All timing data flows to `write_timing_report()` (currently inactive, ready for a profiling UI).

</details>

### Supported formats

| Format | Extensions | Metadata |
|--------|-----------|---------|
| JPEG | `.jpg` `.jpeg` | EXIF (camera, exposure, ISO) |
| PNG | `.png` | EXIF + AI tEXt chunks (Auto1111, ComfyUI, InvokeAI) |
| TIFF | `.tiff` `.tif` | EXIF |
| WebP | `.webp` | Thumbnail only |
| GIF | `.gif` | Thumbnail only |
| BMP | `.bmp` | Thumbnail only |
| AVIF | `.avif` | Thumbnail only |

### Dependencies

| Crate | Purpose |
|-------|---------|
| `gtk4` 0.9 | GTK4 widget bindings |
| `libadwaita` 0.7 | GNOME Adwaita design system |
| `gdk-pixbuf` 0.20 | Image decoding and scaling |
| `kamadak-exif` 0.6 | EXIF tag parsing |
| `png` 0.18 | PNG chunk access |
| `serde` + `serde_json` 1.0 | JSON parsing (ComfyUI, InvokeAI) |
| `async-channel` 2 | Bounded MPMC channel (worker ↔ main thread) |
| `rusqlite` 0.31 | SQLite (bundled) |
| `sha2` + `md-5` 0.10 | SHA-256 content hash, MD5 thumbnail URI |

---

## Roadmap

- [ ] **Favourite filtering** — schema ready; filter UI not yet wired
- [ ] **Trash / delete** — move to XDG trash from context menu
- [ ] **External open** — open in GIMP, Krita, or default app
- [ ] **Side-by-side compare** — pin reference image, diff against selection
- [ ] **Free-form tags** — label images beyond favourite/unfavourite
- [ ] **Version checker** — `updater.rs` stub is in place

---

<div align="center">

Built with Rust, GTK4, and an unreasonable fondness for AI-generated images.

</div>
