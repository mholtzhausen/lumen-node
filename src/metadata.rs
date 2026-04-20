//! Metadata extraction for LumenNode.
//!
//! [`DefaultMetadataDispatcher`] dispatches to one of three strategies based
//! on file extension:
//!
//! | Format         | Strategy                                           |
//! |----------------|----------------------------------------------------|
//! | JPEG / TIFF    | `kamadak-exif` — camera EXIF tags                  |
//! | PNG            | `png` crate — `tEXt`/`zTXt`/`iTXt` chunk dispatch |
//! | everything else| Returns empty [`ImageMetadata`]                    |
//!
//! PNG dispatch keys:
//! - `"parameters"` → Automatic1111 / InvokeAI raw prompt string
//! - `"prompt"`     → ComfyUI API node graph → positive/negative prompts
//! - `"workflow"`   → ComfyUI UI workflow → human-readable node summary

use std::cmp::Reverse;
use std::io::BufReader;
use std::path::Path;

use exif::{In, Reader as ExifReader, Tag};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// All metadata collected for a single image.
/// Every field is optional; it is populated only when the corresponding
/// extraction strategy finds the relevant data.
#[derive(Debug, Default, Clone)]
pub struct ImageMetadata {
    // --- EXIF (JPEG / TIFF via kamadak-exif) ---
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub exposure: Option<String>,
    pub iso: Option<String>,

    // --- AI generation data (PNG tEXt chunks) ---
    /// `"parameters"` key — Automatic1111 / InvokeAI full prompt string.
    pub raw_parameters: Option<String>,
    /// Positive prompt, extracted from ComfyUI `"prompt"` JSON.
    pub prompt: Option<String>,
    /// Negative prompt, extracted from ComfyUI `"prompt"` JSON.
    pub negative_prompt: Option<String>,
    /// Human-readable node summary from ComfyUI `"workflow"` JSON.
    pub workflow_json: Option<String>,
}

/// Trait implemented by any type that can extract metadata from an image file.
pub trait MetadataDispatcher: Send + Sync {
    fn extract(&self, path: &Path) -> Result<ImageMetadata, Box<dyn std::error::Error>>;
}

// ---------------------------------------------------------------------------
// Messages sent from background threads to the GTK main thread
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ScanMessage {
    /// A new image path was discovered during a directory scan.
    ImageFound(String),
    /// Metadata extraction completed for one image.
    MetadataReady { path: String, data: ImageMetadata },
    /// The directory scan is finished.
    ScanComplete,
}

// ---------------------------------------------------------------------------
// Default dispatcher
// ---------------------------------------------------------------------------

pub struct DefaultMetadataDispatcher;

impl MetadataDispatcher for DefaultMetadataDispatcher {
    fn extract(&self, path: &Path) -> Result<ImageMetadata, Box<dyn std::error::Error>> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "jpg" | "jpeg" | "tiff" | "tif" => extract_exif(path),
            "png" => extract_png(path),
            _ => Ok(ImageMetadata::default()),
        }
    }
}

// ---------------------------------------------------------------------------
// EXIF extraction (JPEG / TIFF)
// ---------------------------------------------------------------------------

fn extract_exif(path: &Path) -> Result<ImageMetadata, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path)?;
    let mut buf = BufReader::new(file);
    let exif = ExifReader::new().read_from_container(&mut buf)?;

    let get_str = |tag: Tag| -> Option<String> {
        exif.get_field(tag, In::PRIMARY)
            .map(|f| f.display_value().to_string())
    };

    Ok(ImageMetadata {
        camera_make: get_str(Tag::Make),
        camera_model: get_str(Tag::Model),
        exposure: get_str(Tag::ExposureTime),
        iso: get_str(Tag::PhotographicSensitivity),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// PNG text-chunk extraction (tEXt / zTXt / iTXt)
// ---------------------------------------------------------------------------

fn extract_png(path: &Path) -> Result<ImageMetadata, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path)?;
    let decoder = png::Decoder::new(BufReader::new(file));
    let reader = decoder.read_info()?;
    let info = reader.info();

    let mut meta = ImageMetadata::default();

    // tEXt — uncompressed Latin-1 (most common for AI generators)
    for chunk in &info.uncompressed_latin1_text {
        apply_text_chunk(&mut meta, &chunk.keyword, &chunk.text);
    }

    // zTXt — compressed Latin-1
    for chunk in &info.compressed_latin1_text {
        if let Some(text) = chunk.get_text().ok() {
            apply_text_chunk(&mut meta, &chunk.keyword, &text);
        }
    }

    // iTXt — UTF-8 (compressed or uncompressed)
    for chunk in &info.utf8_text {
        if let Some(text) = chunk.get_text().ok() {
            apply_text_chunk(&mut meta, &chunk.keyword, &text);
        }
    }

    Ok(meta)
}

/// Dispatches a single decoded text chunk to the relevant `ImageMetadata` field.
fn apply_text_chunk(meta: &mut ImageMetadata, keyword: &str, text: &str) {
    match keyword {
        // Automatic1111 / InvokeAI: full prompt + settings as one flat string
        "parameters" => {
            meta.raw_parameters = Some(text.to_owned());
        }

        // ComfyUI API format: JSON map of node_id → {class_type, inputs}
        "prompt" => {
            let (pos, neg) = extract_comfyui_prompts(text);
            meta.prompt = pos;
            meta.negative_prompt = neg;
        }

        // ComfyUI UI workflow: full node graph JSON → human-readable summary
        "workflow" => {
            meta.workflow_json = extract_comfyui_summary(text)
                .or_else(|| Some(text.to_owned()));
        }

        _ => {}
    }
}

// ---------------------------------------------------------------------------
// ComfyUI helpers
// ---------------------------------------------------------------------------

/// Parses the ComfyUI API-format `"prompt"` JSON and returns
/// `(positive_prompt, negative_prompt)`.
///
/// The JSON structure is `{ "node_id": { "class_type": "...", "inputs": {...} } }`.
/// We find all `CLIPTextEncode` nodes and extract `inputs.text`.
/// The longest text is taken to be the positive prompt (conventional in most
/// ComfyUI workflows), the second longest as the negative.
fn extract_comfyui_prompts(prompt_str: &str) -> (Option<String>, Option<String>) {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(prompt_str) else {
        return (None, None);
    };
    let Some(nodes) = val.as_object() else {
        return (None, None);
    };

    let mut clips: Vec<String> = nodes
        .values()
        .filter(|n| {
            n.get("class_type")
                .and_then(|v| v.as_str())
                == Some("CLIPTextEncode")
        })
        .filter_map(|n| {
            n.get("inputs")
                .and_then(|i| i.get("text"))
                .and_then(|t| t.as_str())
                .map(String::from)
        })
        .collect();

    // Longest-first: positive prompts are virtually always more verbose.
    clips.sort_by_key(|s| Reverse(s.len()));
    let positive = clips.first().cloned();
    let negative = clips.get(1).cloned();
    (positive, negative)
}

/// Converts the ComfyUI UI-format `"workflow"` JSON into a compact
/// human-readable summary. Each output line has the form:
///
/// ```text
/// [Node Title]  "value1"  42  true
/// ```
///
/// Returns `None` if the JSON cannot be parsed or has no `"nodes"` array.
fn extract_comfyui_summary(workflow_str: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(workflow_str).ok()?;
    let nodes = val.get("nodes")?.as_array()?;

    let lines: Vec<String> = nodes
        .iter()
        .map(|node| {
            let node_type = node
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let title = node
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or(node_type);

            let values: Vec<String> = node
                .get("widgets_values")
                .and_then(|w| w.as_array())
                .map(|arr| {
                    arr.iter()
                        .take(4)
                        .filter_map(|v| match v {
                            serde_json::Value::String(s) if !s.is_empty() => {
                                if s.len() > 60 {
                                    Some(format!("\"{}…\"", &s[..57]))
                                } else {
                                    Some(format!("\"{s}\""))
                                }
                            }
                            serde_json::Value::Number(n) => Some(n.to_string()),
                            serde_json::Value::Bool(b) => Some(b.to_string()),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();

            if values.is_empty() {
                format!("[{title}]")
            } else {
                format!("[{title}]  {}", values.join("  "))
            }
        })
        .collect();

    Some(lines.join("\n"))
}
