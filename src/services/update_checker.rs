use crate::updater;
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
            update_banner.set_title(&format!("Version {} available", info.version));
            update_banner.set_revealed(true);
            update_banner.connect_button_clicked(move |_| {
                let _ = gio::AppInfo::launch_default_for_uri(
                    &info.url,
                    None::<&gio::AppLaunchContext>,
                );
            });
        }
    });
}
