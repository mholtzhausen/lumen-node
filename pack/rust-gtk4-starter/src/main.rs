mod messages;
mod ui;
mod worker;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use libadwaita as adw;
use std::cell::Cell;
use std::rc::Rc;

use ui::runtime::{install_runtime, open_worker_channel, RuntimeDeps};
use ui::shell::{build_window, install_run_demo_action};

fn apply_consistent_theme_defaults() {
    let pin = std::env::var("GTK_STARTER_PIN_THEME")
        .map(|v| v != "0")
        .unwrap_or(false);
    if !pin {
        return;
    }
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_theme_name(Some("Adwaita"));
    }
}

fn runtime_environment_report() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let app_id = std::env::var("GTK_STARTER_APP_ID")
        .unwrap_or_else(|_| "com.example.GtkStarter".to_string());
    let lines = [
        "GTK Starter — runtime environment".to_string(),
        format!("HOME: {home}"),
        format!(
            "XDG_CACHE_HOME: {}",
            std::env::var("XDG_CACHE_HOME").unwrap_or_else(|_| format!("{home}/.cache"))
        ),
        format!("APPIMAGE: {}", env_or_unset("APPIMAGE")),
        format!("APPDIR: {}", env_or_unset("APPDIR")),
        format!("GTK_THEME: {}", env_or_unset("GTK_THEME")),
        format!("GDK_SCALE: {}", env_or_unset("GDK_SCALE")),
        format!("XDG_SESSION_TYPE: {}", env_or_unset("XDG_SESSION_TYPE")),
        format!("WAYLAND_DISPLAY: {}", env_or_unset("WAYLAND_DISPLAY")),
        format!("DISPLAY: {}", env_or_unset("DISPLAY")),
        format!("GTK_STARTER_APP_ID: {app_id}"),
        format!(
            "GTK_STARTER_NON_UNIQUE: {}",
            env_or_unset("GTK_STARTER_NON_UNIQUE")
        ),
    ];
    lines.join("\n")
}

fn env_or_unset(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| "<unset>".to_string())
}

fn build_ui(app: &adw::Application) {
    apply_consistent_theme_defaults();
    let runtime_report = runtime_environment_report();

    let (window, shell) = build_window(app, &runtime_report);
    let (sender, receiver) = open_worker_channel();

    let active_generation = Rc::new(Cell::new(0u64));

    install_runtime(RuntimeDeps {
        receiver,
        active_generation: active_generation.clone(),
        status_label: shell.status_label.clone(),
        progress_bar: shell.progress_bar.clone(),
        toast_overlay: shell.toast_overlay.clone(),
    });

    let start_demo = {
        let sender = sender.clone();
        let active_generation = active_generation.clone();
        let run_button = shell.run_button.clone();
        move || {
            let generation = active_generation.get().saturating_add(1);
            active_generation.set(generation);
            run_button.set_sensitive(false);
            let sender = sender.clone();
            let active_generation = active_generation.clone();
            let run_button = run_button.clone();
            worker::spawn_demo_work(
                sender,
                generation,
                format!("demo-{generation}"),
            );
            // Re-enable after a short delay on main thread (real app: enable on Finished).
            let run_button_idle = run_button.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                if active_generation.get() == generation {
                    run_button_idle.set_sensitive(true);
                }
            });
        }
    };

    install_run_demo_action(&window, start_demo.clone());
    shell.run_button.connect_clicked(move |_| start_demo());
    app.set_accels_for_action("win.run-demo", &["<Primary>r"]);

    window.present();
}

fn main() -> glib::ExitCode {
    let app_id = std::env::var("GTK_STARTER_APP_ID")
        .unwrap_or_else(|_| "com.example.GtkStarter".to_string());
    let non_unique = std::env::var("GTK_STARTER_NON_UNIQUE")
        .map(|v| v != "0")
        .unwrap_or(false);

    let mut builder = adw::Application::builder().application_id(&app_id);
    if non_unique {
        builder = builder.flags(gio::ApplicationFlags::NON_UNIQUE);
    }
    let app = builder.build();
    app.connect_activate(build_ui);
    app.run()
}
