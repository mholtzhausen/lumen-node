use crate::metadata::ImageMetadata;

#[derive(Debug)]
pub enum ScanMessage {
    /// Scan has started and the final candidate image count is known.
    ScanStarted { total_count: u32, generation: u64 },
    /// A path was found during the fast enumeration phase (no metadata yet).
    ImageEnumerated { path: String, generation: u64 },
    /// Enumeration is done; enrichment phase is starting.
    EnumerationComplete { generation: u64 },
    /// An image has been fully indexed (hash, metadata, thumbnail).
    ImageEnriched {
        path: String,
        hash: String,
        meta: ImageMetadata,
        indexed_from_cache: bool,
        generation: u64,
    },
    /// The directory scan (enumerate + enrich) is finished.
    ScanComplete { generation: u64 },
}
