use adw::prelude::*;
use gtk4::prelude::*;
use gtk4::{gio, Align, Box, Button, Label, Orientation, ProgressBar};
use libadwaita as adw;
use std::rc::Rc;

pub struct ShellWidgets {
    pub status_label: Label,
    pub progress_bar: ProgressBar,
    pub run_button: Button,
    pub toast_overlay: adw::ToastOverlay,
}

pub fn build_window(app: &adw::Application, runtime_report: &str) -> (adw::ApplicationWindow, ShellWidgets) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("GTK Starter")
        .default_width(640)
        .default_height(400)
        .build();

    let header = adw::HeaderBar::new();
    let run_button = Button::with_label("Run demo work");
    header.pack_end(&run_button);

    let status_label = Label::new(Some("Idle — press Run or use Ctrl+R"));
    status_label.set_halign(Align::Start);
    status_label.set_wrap(true);
    status_label.set_justify(gtk4::Justification::Left);

    let progress_bar = ProgressBar::new();
    progress_bar.set_visible(false);

    let content = Box::new(Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_vexpand(true);
    content.append(&status_label);
    content.append(&progress_bar);

    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&content));

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);
    toolbar_view.set_content(Some(&toast_overlay));

    window.set_content(Some(&toolbar_view));

    install_window_actions(&window, runtime_report);

    (
        window,
        ShellWidgets {
            status_label,
            progress_bar,
            run_button,
            toast_overlay,
        },
    )
}

fn install_window_actions(window: &adw::ApplicationWindow, runtime_report: &str) {
    let about_action = gio::SimpleAction::new("about", None);
    let win = window.clone();
    about_action.connect_activate(move |_, _| {
        let dialog = adw::AboutWindow::builder()
            .transient_for(&win)
            .application_name("GTK Starter")
            .version(env!("CARGO_PKG_VERSION"))
            .developer_name("Your Name")
            .comments("Production-oriented GTK4 + libadwaita boilerplate")
            .build();
        dialog.present();
    });
    window.add_action(&about_action);

    let report_action = gio::SimpleAction::new("runtime-report", None);
    let win = window.clone();
    let report = runtime_report.to_string();
    report_action.connect_activate(move |_, _| {
        let dialog = gtk4::Window::builder()
            .transient_for(&win)
            .modal(true)
            .title("Runtime environment")
            .default_width(720)
            .default_height(420)
            .build();
        let text_view = gtk4::TextView::new();
        text_view.set_editable(false);
        text_view.set_monospace(true);
        text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
        text_view.buffer().set_text(&report);
        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_child(Some(&text_view));
        scroll.set_vexpand(true);
        scroll.set_hexpand(true);
        dialog.set_child(Some(&scroll));
        dialog.present();
    });
    window.add_action(&report_action);
}

/// Returns the action so `main` can wire `connect_activate` to start workers.
pub fn install_run_demo_action<F>(window: &adw::ApplicationWindow, on_run: F)
where
    F: Fn() + 'static,
{
    let run_action = gio::SimpleAction::new("run-demo", None);
    let handler = Rc::new(on_run);
    run_action.connect_activate(move |_, _| handler());
    window.add_action(&run_action);
}
