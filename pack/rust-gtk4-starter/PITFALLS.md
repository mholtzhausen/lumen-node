# Rust + GTK4 production pitfalls

Reference for the starter in this folder. Each item maps to code or docs in LumenNode where the pattern was battle-tested.

## Build and linkage

| Pitfall | Fix |
|---------|-----|
| `pkg-config` cannot find `gtk4` / `libadwaita-1` | Export `PKG_CONFIG_PATH` before `cargo` (see `Makefile`). On Debian multi-arch: `/usr/lib/x86_64-linux-gnu/pkgconfig` plus `/usr/lib/pkgconfig`. |
| Feature/API mismatch between `gtk4` crate and system GTK | Pin crate features to your **installed** GTK minor version, e.g. `gtk4 = { features = ["v4_10", "v4_12"] }`. Upgrade system GTK before enabling newer crate features. |
| Debug vs release behave differently under AppImage | Test `target/release` with `APPIMAGE`/`APPDIR` set; use `run-release-isolated` Makefile target pattern. |
| Linking as C library from wrong arch | Install `libgtk-4-dev` for the same arch as your Rust toolchain (`x86_64` vs `aarch64`). |

## Threading and async

| Pitfall | Fix |
|---------|-----|
| Calling GTK APIs from `std::thread` or `tokio` worker | **Never.** Workers send messages; only the GTK main thread mutates widgets. |
| `tokio::spawn` + GTK | Prefer `glib::MainContext::default().spawn_local` for async that touches UI, or drain with `idle_add_local`. Tokio runtime is optional and easy to mis-wire. |
| Blocking the main loop | Long CPU or I/O on main thread freezes UI. Offload to thread + channel; batch UI updates. |
| Unbounded channels under fast producers | Use `async_channel::bounded(n)` for backpressure (starter uses 64). |
| Flooding the main loop with one message = one idle | Buffer messages; drain **N per idle tick** (starter: 32). |
| Stale updates after user switches context | Pass a monotonic **`generation: u64`** into workers; drop messages where `msg.generation != active_generation`. |
| `Rc`/`RefCell` in closures | GTK callbacks are `Send` but not `Sync`; use `Rc` on main thread only. Do not share `RefCell` with worker threads. |
| `idle_add_local` makes outer scheduler `FnOnce` | Re-clone all captures **inside** the scheduler closure before `idle_add_local`, so the scheduler stays `Fn()` and can be called from every channel receive (see `ui/runtime.rs`). |

## Application lifecycle

| Pitfall | Fix |
|---------|-----|
| Building widgets before `activate` | Construct UI in `Application::connect_activate` (or `startup` + `activate`), not in `main` before `run()`. |
| Duplicate instances blocking testing | `gio::ApplicationFlags::NON_UNIQUE` via env (starter: `GTK_STARTER_NON_UNIQUE=1`). |
| Wrong D-Bus / settings namespace | Set stable reverse-DNS `application_id` (`com.example.App`); match `.desktop` and `.metainfo.xml` `<id>`. |
| Assuming single display | Use `gtk4::gdk::Display::default()`; test Wayland and X11; read `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`, `DISPLAY`. |

## Widgets and layout

| Pitfall | Fix |
|---------|-----|
| `Paned::set_position` fights user drag | Track `position_programmatic: Cell<bool>`; ignore `notify::position` while programmatic. |
| Percent-based pane restore before realize | Defer restore until window has allocation; clamp to `min_child_size`. |
| `set_vexpand` / `set_hexpand` forgotten | Scroll areas and lists need expand flags or they collapse to zero height. |
| Memory leaks in long sessions | `disconnect` handlers when replacing models; drop `Picture`/`Texture` when clearing lists. |
| `ListView` + custom factory churn | Reuse factory state; avoid reloading full-resolution images in cell factory. |

## Data and I/O

| Pitfall | Fix |
|---------|-----|
| Wall clock for timeouts/intervals | Use `std::time::Instant` / `glib::timeout_add_local` with monotonic sources, not `SystemTime` for durations. |
| SQLite on UI thread | Open DB on worker; send results as messages (LumenNode `db.rs` + scanner). |
| WAL DB on network filesystems | Keep per-folder DBs local; expect pain on NFS for heavy write workloads. |

## Packaging and distribution

| Pitfall | Fix |
|---------|-----|
| Hard-coded `/usr` paths | Respect `XDG_*_HOME`; bundle runtime report for support (see starter `runtime_report()`). |
| Theme/scale drift in AppImage | Optional pin: `GTK_THEME`, document `GDK_SCALE` / `GDK_DPI_SCALE`; starter supports `GTK_STARTER_PIN_THEME`. |
| `.desktop` `Exec` without path | Install binary to `~/.local/bin` or PATH; AppImage uses `AppRun` wrapper. |
| Missing icon cache | Run `gtk-update-icon-cache` after `make install`. |
| Metainfo version ≠ Cargo version | Automate check in `scripts/release-preflight.sh`. |

## Rust project hygiene

| Pitfall | Fix |
|---------|-----|
| God `main.rs` | Split `ui/`, `core/`, `services/` early; keep `main` as composition root only. |
| `unwrap()` in UI paths | Show `adw::Toast` or inline error; log with context. |
| No CI on Linux GUI | At minimum `make check`; headless: `xvfb-run make check` if you add integration tests later. |

## Checklist before shipping

1. `make check` and `cargo build --release` via Makefile  
2. Run installed `.desktop` from application menu (not only `cargo run`)  
3. Second instance / `NON_UNIQUE` dev workflow documented  
4. Worker shutdown: generation bump cancels in-flight work (no writes after cancel)  
5. Changelog + metainfo version aligned  
6. AppImage audit: bundled GTK theme and pixbuf loaders present  
