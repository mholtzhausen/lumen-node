use crate::messages::WorkerMessage;
use async_channel::Sender;
use std::thread;
use std::time::{Duration, Instant};

/// Simulated background work. Replace the body with real I/O (scan, hash, network).
pub fn spawn_demo_work(sender: Sender<WorkerMessage>, generation: u64, label: String) {
    thread::spawn(move || {
        if sender
            .send_blocking(WorkerMessage::Started {
                generation,
                label: label.clone(),
            })
            .is_err()
        {
            return;
        }

        const TOTAL: u32 = 8;
        let started = Instant::now();
        for step in 1..=TOTAL {
            thread::sleep(Duration::from_millis(120));
            if sender
                .send_blocking(WorkerMessage::Progress {
                    generation,
                    current: step,
                    total: TOTAL,
                })
                .is_err()
            {
                return;
            }
        }

        let elapsed_ms = started.elapsed().as_millis();
        let _ = sender.send_blocking(WorkerMessage::Finished {
            generation,
            summary: format!("{label}: done in {elapsed_ms} ms (gen {generation})"),
        });
    });
}
