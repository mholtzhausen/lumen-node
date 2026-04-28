use crate::updater;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use libadwaita as adw;

pub(crate) fn install_update_checker(update_banner: adw::Banner) {
    let (update_tx, update_rx) = async_channel::bounded::<updater::UpdateInfo>(1);
    std::thread::spawn(move || {
        if let Some(info) = updater::check_for_update() {
            let _ = update_tx.send_blocking(info);
        }
    });
    glib::MainContext::default().spawn_local(async move {
        if let Ok(info) = update_rx.recv().await {
            update_banner.set_title(&format!("Version {} available — click to view release", info.version));
            update_banner.set_revealed(true);
            // Dismiss button hides the banner.
            update_banner.connect_button_clicked(glib::clone!(
                #[weak]
                update_banner,
                move |_| {
                    update_banner.set_revealed(false);
                }
            ));
            // Clicking the banner title area opens the release URL.
            let click = gtk4::GestureClick::new();
            let url = info.url.clone();
            click.connect_released(move |_, _, _, _| {
                let _ = gio::AppInfo::launch_default_for_uri(&url, None::<&gio::AppLaunchContext>);
            });
            update_banner.add_controller(click);
        }
    });
}
