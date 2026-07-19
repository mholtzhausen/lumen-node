use crate::messages::WorkerMessage;
use async_channel::Receiver;
use gtk4::prelude::*;
use gtk4::{glib, Label, ProgressBar};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

const CHANNEL_CAPACITY: usize = 64;
const DRAIN_BATCH_SIZE: usize = 32;

pub fn open_worker_channel() -> (
    async_channel::Sender<WorkerMessage>,
    Receiver<WorkerMessage>,
) {
    async_channel::bounded(CHANNEL_CAPACITY)
}

pub struct RuntimeDeps {
    pub receiver: Receiver<WorkerMessage>,
    pub active_generation: Rc<Cell<u64>>,
    pub status_label: Label,
    pub progress_bar: ProgressBar,
    pub toast_overlay: adw::ToastOverlay,
}

pub fn install_runtime(deps: RuntimeDeps) {
    let buffer: Rc<RefCell<VecDeque<WorkerMessage>>> = Rc::new(RefCell::new(VecDeque::new()));
    let drain_scheduled = Rc::new(RefCell::new(false));

    let schedule_drain = {
        let buffer = buffer.clone();
        let drain_scheduled = drain_scheduled.clone();
        let active_generation = deps.active_generation.clone();
        let status_label = deps.status_label.clone();
        let progress_bar = deps.progress_bar.clone();
        let toast_overlay = deps.toast_overlay.clone();

        Rc::new(move || {
            if *drain_scheduled.borrow() {
                return;
            }
            *drain_scheduled.borrow_mut() = true;

            let buffer = buffer.clone();
            let drain_scheduled = drain_scheduled.clone();
            let active_generation = active_generation.clone();
            let status_label = status_label.clone();
            let progress_bar = progress_bar.clone();
            let toast_overlay = toast_overlay.clone();

            glib::idle_add_local(move || {
                *drain_scheduled.borrow_mut() = false;

                let mut batch = Vec::with_capacity(DRAIN_BATCH_SIZE);
                {
                    let mut buf = buffer.borrow_mut();
                    for _ in 0..DRAIN_BATCH_SIZE {
                        if let Some(msg) = buf.pop_front() {
                            batch.push(msg);
                        } else {
                            break;
                        }
                    }
                }

                let current_gen = active_generation.get();
                for msg in batch {
                    let generation = match &msg {
                        WorkerMessage::Started { generation, .. }
                        | WorkerMessage::Progress { generation, .. }
                        | WorkerMessage::Finished { generation, .. }
                        | WorkerMessage::Failed { generation, .. } => *generation,
                    };
                    if generation != current_gen {
                        continue;
                    }

                    match msg {
                        WorkerMessage::Started { label, .. } => {
                            status_label.set_text(&format!("Working: {label}"));
                            progress_bar.set_visible(true);
                            progress_bar.set_fraction(0.0);
                            progress_bar.set_text(Some("0%"));
                        }
                        WorkerMessage::Progress { current, total, .. } => {
                            let total = total.max(1) as f64;
                            let fraction = (current as f64 / total).min(1.0);
                            progress_bar.set_fraction(fraction);
                            progress_bar.set_text(Some(&format!("{:.0}%", fraction * 100.0)));
                            status_label.set_text(&format!("Progress {current}/{total}"));
                        }
                        WorkerMessage::Finished { summary, .. } => {
                            progress_bar.set_visible(false);
                            status_label.set_text(&summary);
                            toast_overlay.add_toast(adw::Toast::new("Work finished"));
                        }
                        WorkerMessage::Failed { error, .. } => {
                            progress_bar.set_visible(false);
                            status_label.set_text(&format!("Error: {error}"));
                            toast_overlay.add_toast(
                                adw::Toast::builder()
                                    .title("Work failed")
                                    .timeout(4)
                                    .build(),
                            );
                        }
                    }
                }

                glib::ControlFlow::Continue
            });
        })
    };

    let buffer_recv = buffer.clone();
    let schedule_drain_recv = schedule_drain.clone();
    glib::MainContext::default().spawn_local(async move {
        while let Ok(msg) = deps.receiver.recv().await {
            buffer_recv.borrow_mut().push_back(msg);
            schedule_drain_recv();
        }
    });
}
