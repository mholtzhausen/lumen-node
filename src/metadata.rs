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
//! - `"parameters"`        → Automatic1111 / InvokeAI raw prompt string
//! - `"prompt"`            → ComfyUI API node graph → positive/negative prompts
//! - `"workflow"`          → ComfyUI UI workflow JSON + primary prompt guess
//! - `"invokeai_metadata"` → InvokeAI JSON → positive/negative prompts + settings
//! - other keywords         → captured verbatim as fallback metadata

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
    /// Raw ComfyUI `"workflow"` JSON.
    pub workflow_json: Option<String>,
}

/// Trait implemented by any type that can extract metadata from an image file.
pub trait MetadataDispatcher: Send + Sync {
    fn extract(&self, path: &Path) -> Result<ImageMetadata, Box<dyn std::error::Error>>;
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
            "png" => extract_png_with_exif(path),
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
// PNG extraction: EXIF (eXIf chunk) + text chunks (tEXt / zTXt / iTXt)
// ---------------------------------------------------------------------------

/// Attempts EXIF extraction first (for PNG eXIf chunks), then overlays any
/// text-chunk metadata on top. Either source may fail independently.
fn extract_png_with_exif(path: &Path) -> Result<ImageMetadata, Box<dyn std::error::Error>> {
    // Best-effort EXIF from the PNG eXIf chunk (kamadak-exif supports this).
    let mut meta = extract_exif(path).unwrap_or_default();

    // Overlay text-chunk metadata; merge non-None fields.
    if let Ok(text_meta) = extract_png(path) {
        macro_rules! merge {
            ($field:ident) => {
                if text_meta.$field.is_some() {
                    meta.$field = text_meta.$field;
                }
            };
        }
        merge!(raw_parameters);
        merge!(prompt);
        merge!(negative_prompt);
        merge!(workflow_json);
    }

    Ok(meta)
}

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

        // ComfyUI UI workflow: preserve full JSON and best-effort prompt guess.
        "workflow" => {
            meta.workflow_json = Some(text.to_owned());
            if meta.prompt.is_none() {
                meta.prompt = extract_primary_prompt_from_workflow(text);
            }
        }

        // InvokeAI metadata: JSON with positive_prompt, negative_prompt, model, etc.
        "invokeai_metadata" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
                if let Some(p) = val.get("positive_prompt").and_then(|v| v.as_str()) {
                    if !p.is_empty() {
                        meta.prompt = Some(p.to_owned());
                    }
                }
                if let Some(n) = val.get("negative_prompt").and_then(|v| v.as_str()) {
                    if !n.is_empty() {
                        meta.negative_prompt = Some(n.to_owned());
                    }
                }
                // Store the full JSON as raw parameters for reference.
                meta.raw_parameters = Some(text.to_owned());
            }
        }

        _ => {
            // Capture unknown keywords so they appear as "Parameters" in the UI.
            let entry = format!("{}: {}", keyword, text);
            match &mut meta.raw_parameters {
                Some(existing) => {
                    existing.push('\n');
                    existing.push_str(&entry);
                }
                None => {
                    meta.raw_parameters = Some(entry);
                }
            }
        }
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
        .filter(|n| n.get("class_type").and_then(|v| v.as_str()) == Some("CLIPTextEncode"))
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

/// Best-effort heuristic for the "primary prompt" inside ComfyUI workflow JSON.
///
/// We recursively inspect all strings and score candidates by path/key context.
/// Strings under CLIP text nodes and keys named `text` / `prompt` rank highest.
fn extract_primary_prompt_from_workflow(workflow_str: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(workflow_str).ok()?;
    let mut best: Option<(i32, String)> = None;

    fn walk(
        value: &serde_json::Value,
        path: &mut Vec<String>,
        in_clip_text_node: bool,
        best: &mut Option<(i32, String)>,
    ) {
        match value {
            serde_json::Value::Object(map) => {
                let class_type = map
                    .get("class_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let node_type = map
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let node_title = map
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let is_clip_text_node = class_type.contains("cliptextencode")
                    || node_type.contains("cliptextencode")
                    || node_title.contains("cliptextencode");
                let child_in_clip = in_clip_text_node || is_clip_text_node;

                for (k, v) in map {
                    path.push(k.to_ascii_lowercase());
                    walk(v, path, child_in_clip, best);
                    path.pop();
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    walk(item, path, in_clip_text_node, best);
                }
            }
            serde_json::Value::String(s) => {
                let text = s.trim();
                if text.is_empty() || text.len() < 8 {
                    return;
                }

                let path_text = path.join(".");
                let mut score = text.len().min(800) as i32;
                if in_clip_text_node {
                    score += 600;
                }
                if path_text.contains("inputs.text") {
                    score += 500;
                }
                if path_text.contains("widgets_values") {
                    score += 300;
                }
                if path_text.contains("prompt") || path_text.ends_with("text") {
                    score += 250;
                }
                if path_text.contains("negative") || path_text.contains("neg") {
                    score -= 500;
                }

                let replace = best
                    .as_ref()
                    .map_or(true, |(best_score, _)| score > *best_score);
                if replace {
                    *best = Some((score, text.to_owned()));
                }
            }
            _ => {}
        }
    }

    let mut path = Vec::new();
    walk(&val, &mut path, false, &mut best);
    best.map(|(_, prompt)| prompt)
}
