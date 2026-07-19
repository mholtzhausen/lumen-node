# Production Rust + GTK4 starter pack

This directory is a **copy-out boilerplate** distilled from [LumenNode](../README.md): the patterns that keep GTK4/libadwaita apps reliable under load, packaging, and fast folder switches.

## Contents

| Path | Purpose |
|------|---------|
| [`rust-gtk4-starter/`](rust-gtk4-starter/) | Minimal compilable app + Makefile + desktop/metainfo templates |
| [`rust-gtk4-starter/PITFALLS.md`](rust-gtk4-starter/PITFALLS.md) | Pitfall catalog (threading, channels, panes, packaging, CI) |
| [`rust-gtk4-starter/scripts/init-project.sh`](rust-gtk4-starter/scripts/init-project.sh) | Clone template into a new directory with renamed app id |

## Quick start

```bash
# From repo root
./pack/rust-gtk4-starter/scripts/init-project.sh my-gallery /tmp/my-gallery
cd /tmp/my-gallery
make check && make run
```

Or copy the folder manually and replace `com.example.GtkStarter` / `gtk-starter` in `Cargo.toml`, `data/*`, and `src/main.rs`.

## System dependencies (Debian/Ubuntu)

```bash
sudo apt install \
  build-essential pkg-config \
  libgtk-4-dev libadwaita-1-dev libgdk-pixbuf-2.0-dev
```

Fedora: `gtk4-devel`, `libadwaita-devel`, `gdk-pixbuf2-devel`.

Always build through **`make`** in the starter (exports `PKG_CONFIG_PATH`). Bare `cargo build` often fails to find GTK `.pc` files on multi-arch layouts.

## What the starter demonstrates

- `libadwaita::Application` with overridable app id and `NON_UNIQUE` for side-by-side dev runs
- UI built in `activate` (no GtkBuilder XML required)
- Background work on `std::thread` → bounded `async-channel` → **`glib::idle_add_local` batched drain** on the main loop
- **Generation counter** so stale worker messages are ignored after navigation/cancel
- `Rc` / `RefCell` / `Cell` for GTK signal closures (no `Send` across threads)
- Gio `SimpleAction` + accelerators on the window
- Optional theme pinning for reproducible AppImage screenshots
- `Makefile` install targets + desktop/metainfo/icon stubs
- Packaging script stubs under `packaging/` (AppImage workflow)

For a full gallery architecture (SQLite, thumbnails, scan coordinator), see [`ARCHITECTURE.md`](../ARCHITECTURE.md) in the parent project.
