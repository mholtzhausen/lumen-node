/// Messages from background workers to the GTK main thread.
/// Workers must never call GTK APIs — only send these across the channel.
#[derive(Debug, Clone)]
pub enum WorkerMessage {
    /// First line of work for a new user request (e.g. folder opened).
    Started { generation: u64, label: String },
    /// Incremental progress; UI may update a label or progress bar.
    Progress { generation: u64, current: u32, total: u32 },
    /// Terminal success for this generation.
    Finished { generation: u64, summary: String },
    /// Worker failed; show a toast or inline error (wire from `worker.rs` on I/O errors).
    #[allow(dead_code)]
    Failed { generation: u64, error: String },
}
