# gtk-starter — production Rust + GTK4 boilerplate

Minimal **libadwaita** app showing the patterns you want on day one: correct threading, bounded channels, idle-batched UI updates, generation-guarded workers, and packaging hooks.

## Run

```bash
make check
make run

# Side-by-side with an installed build (unique app id + non-unique flag)
make run-dev-isolated
```

## Rename for your app

```bash
./scripts/init-project.sh MyApp ../my-app
```

Or search-replace:

- `com.example.GtkStarter` → your reverse-DNS id  
- `gtk-starter` → your binary name  
- `data/com.example.GtkStarter.*` filenames  

## Layout

```
src/
  main.rs      # Application entry, build_ui composition root
  messages.rs  # Worker → UI message types
  worker.rs    # Background thread (no GTK calls)
  ui/
    mod.rs
    shell.rs   # Window, header, actions
    runtime.rs # Channel receiver + idle batch drain
```

Read **[PITFALLS.md](PITFALLS.md)** before adding features.

## Parent project

Born from [LumenNode](https://github.com/mholtzhausen/lumen-node). See `pack/README.md` in the repo root for the full pack index.
