use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, Picture};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub static ACTIVE_PREVIEW_TASKS: AtomicU64 = AtomicU64::new(0);

struct AtomicTaskGuard {
    counter: &'static AtomicU64,
}

impl AtomicTaskGuard {
    fn new(counter: &'static AtomicU64) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self { counter }
    }
}

impl Drop for AtomicTaskGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

pub enum PreviewLoadOutcome {
    Displayed,
    Failed,
    StaleOrCancelled,
}

pub struct PreviewLoadMetrics {
    pub outcome: PreviewLoadOutcome,
    pub queue_wait_ms: f64,
    pub file_open_ms: f64,
    pub decode_ms: f64,
    pub texture_create_ms: f64,
    pub worker_total_ms: f64,
    pub worker_done_since_enqueue_ms: f64,
    pub main_thread_dispatch_ms: f64,
    pub texture_apply_ms: f64,
}

pub fn load_picture_async(
    picture: &Picture,
    path: &str,
    max_dimension: Option<i32>,
    on_complete: Option<Box<dyn Fn(PreviewLoadMetrics) + 'static>>,
) {
    picture.set_paintable(gdk::Paintable::NONE);

    let prev_cancel: Option<gio::Cancellable> = unsafe { picture.steal_data("loading-cancel") };
    if let Some(c) = prev_cancel {
        c.cancel();
    }

    let cancel = gio::Cancellable::new();
    unsafe { picture.set_data("loading-cancel", cancel.clone()) };
    unsafe { picture.set_data("loading-path", path.to_owned()) };

    let path_owned = path.to_owned();
    let path_check = path_owned.clone();
    let cancel_bg = cancel.clone();
    let weak = picture.downgrade();
    let enqueued_at = Instant::now();
    let enqueued_at_main = enqueued_at;
    let task = gio::spawn_blocking(move || {
        let worker_started_at = Instant::now();
        let started_at = Instant::now();
        let queue_wait_ms = started_at.duration_since(enqueued_at).as_secs_f64() * 1000.0;
        let _guard = AtomicTaskGuard::new(&ACTIVE_PREVIEW_TASKS);

        if cancel_bg.is_cancelled() {
            let worker_total_ms = worker_started_at.elapsed().as_secs_f64() * 1000.0;
            let worker_done_since_enqueue_ms =
                worker_started_at.duration_since(enqueued_at).as_secs_f64() * 1000.0
                    + worker_total_ms;
            return (
                None,
                PreviewLoadMetrics {
                    outcome: PreviewLoadOutcome::StaleOrCancelled,
                    queue_wait_ms,
                    file_open_ms: 0.0,
                    decode_ms: 0.0,
                    texture_create_ms: 0.0,
                    worker_total_ms,
                    worker_done_since_enqueue_ms,
                    main_thread_dispatch_ms: 0.0,
                    texture_apply_ms: 0.0,
                },
            );
        }

        let file = gio::File::for_path(&path_owned);
        let file_open_started = Instant::now();
        let Some(stream) = file.read(Some(&cancel_bg)).ok() else {
            let worker_total_ms = worker_started_at.elapsed().as_secs_f64() * 1000.0;
            let worker_done_since_enqueue_ms =
                worker_started_at.duration_since(enqueued_at).as_secs_f64() * 1000.0
                    + worker_total_ms;
            return (
                None,
                PreviewLoadMetrics {
                    outcome: PreviewLoadOutcome::Failed,
                    queue_wait_ms,
                    file_open_ms: file_open_started.elapsed().as_secs_f64() * 1000.0,
                    decode_ms: 0.0,
                    texture_create_ms: 0.0,
                    worker_total_ms,
                    worker_done_since_enqueue_ms,
                    main_thread_dispatch_ms: 0.0,
                    texture_apply_ms: 0.0,
                },
            );
        };
        let file_open_ms = file_open_started.elapsed().as_secs_f64() * 1000.0;

        let decode_started = Instant::now();
        let pixbuf = match max_dimension {
            Some(dim) => gdk_pixbuf::Pixbuf::from_stream_at_scale(
                &stream,
                dim,
                dim,
                true,
                Some(&cancel_bg),
            ),
            None => gdk_pixbuf::Pixbuf::from_stream(&stream, Some(&cancel_bg)),
        };
        let decode_ms = decode_started.elapsed().as_secs_f64() * 1000.0;

        match pixbuf {
            Ok(pb) => {
                let texture_create_started = Instant::now();
                let tex = gdk::Texture::for_pixbuf(&pb);
                let texture_create_ms = texture_create_started.elapsed().as_secs_f64() * 1000.0;
                let worker_total_ms = worker_started_at.elapsed().as_secs_f64() * 1000.0;
                let worker_done_since_enqueue_ms =
                    worker_started_at.duration_since(enqueued_at).as_secs_f64() * 1000.0
                        + worker_total_ms;
                (
                    Some(tex),
                    PreviewLoadMetrics {
                        outcome: PreviewLoadOutcome::Displayed,
                        queue_wait_ms,
                        file_open_ms,
                        decode_ms,
                        texture_create_ms,
                        worker_total_ms,
                        worker_done_since_enqueue_ms,
                        main_thread_dispatch_ms: 0.0,
                        texture_apply_ms: 0.0,
                    },
                )
            }
            Err(_) => {
                let worker_total_ms = worker_started_at.elapsed().as_secs_f64() * 1000.0;
                let worker_done_since_enqueue_ms =
                    worker_started_at.duration_since(enqueued_at).as_secs_f64() * 1000.0
                        + worker_total_ms;
                (
                    None,
                    PreviewLoadMetrics {
                        outcome: PreviewLoadOutcome::Failed,
                        queue_wait_ms,
                        file_open_ms,
                        decode_ms,
                        texture_create_ms: 0.0,
                        worker_total_ms,
                        worker_done_since_enqueue_ms,
                        main_thread_dispatch_ms: 0.0,
                        texture_apply_ms: 0.0,
                    },
                )
            }
        }
    });

    glib::MainContext::default().spawn_local(async move {
        let mut on_complete = on_complete;

        let Ok((maybe_tex, mut metrics)) = task.await else {
            if let Some(cb) = on_complete.take() {
                cb(PreviewLoadMetrics {
                    outcome: PreviewLoadOutcome::Failed,
                    queue_wait_ms: 0.0,
                    file_open_ms: 0.0,
                    decode_ms: 0.0,
                    texture_create_ms: 0.0,
                    worker_total_ms: 0.0,
                    worker_done_since_enqueue_ms: 0.0,
                    main_thread_dispatch_ms: 0.0,
                    texture_apply_ms: 0.0,
                });
            }
            return;
        };

        let callback_started_since_enqueue_ms =
            Instant::now().duration_since(enqueued_at_main).as_secs_f64() * 1000.0;
        metrics.main_thread_dispatch_ms =
            (callback_started_since_enqueue_ms - metrics.worker_done_since_enqueue_ms).max(0.0);

        let Some(pic) = weak.upgrade() else {
            if let Some(cb) = on_complete.take() {
                metrics.outcome = PreviewLoadOutcome::StaleOrCancelled;
                cb(metrics);
            }
            return;
        };

        let is_current = unsafe {
            pic.data::<String>("loading-path")
                .map(|p| p.as_ref() == &path_check)
                .unwrap_or(false)
        };
        if !is_current {
            if let Some(cb) = on_complete.take() {
                metrics.outcome = PreviewLoadOutcome::StaleOrCancelled;
                cb(metrics);
            }
            return;
        }

        if let Some(tex) = maybe_tex {
            let apply_started = Instant::now();
            pic.set_paintable(Some(&tex));
            metrics.texture_apply_ms = apply_started.elapsed().as_secs_f64() * 1000.0;
        }

        if let Some(cb) = on_complete.take() {
            cb(metrics);
        }
    });
}
