use crate::metadata::ImageMetadata;

pub fn format_metadata_text(meta: &ImageMetadata) -> String {
    let mut out = Vec::new();
    if let Some(v) = &meta.camera_make {
        out.push(format!("Make: {}", v.as_str()));
    }
    if let Some(v) = &meta.camera_model {
        out.push(format!("Model: {}", v.as_str()));
    }
    if let Some(v) = &meta.exposure {
        out.push(format!("Exposure: {}", v.as_str()));
    }
    if let Some(v) = &meta.iso {
        out.push(format!("ISO: {}", v.as_str()));
    }
    if let Some(v) = &meta.prompt {
        out.push(format!("Prompt: {}", v.as_str()));
    }
    if let Some(v) = &meta.negative_prompt {
        out.push(format!("Neg. Prompt: {}", v.as_str()));
    }
    if let Some(v) = &meta.raw_parameters {
        out.push(format!("Parameters: {}", v.as_str()));
    }
    if let Some(v) = &meta.workflow_json {
        out.push(format!("Workflow: {}", v.as_str()));
    }
    if out.is_empty() {
        "No metadata found".to_string()
    } else {
        out.join("\n\n")
    }
}

/// Extracts seed value from raw parameters string (Automatic1111 format: "Seed: 123456, ...")
pub fn extract_seed_from_parameters(meta: &ImageMetadata) -> Option<String> {
    if let Some(params) = &meta.raw_parameters {
        // Try to find "Seed: <number>" pattern
        for part in params.split(',') {
            if let Some(seed_part) = part.trim().strip_prefix("Seed:") {
                if let Ok(seed_val) = seed_part.trim().parse::<u64>() {
                    return Some(seed_val.to_string());
                }
            }
        }
    }
    None
}

/// True when `format_generation_command` would include more than the placeholder stub.
pub fn has_generation_command_content(meta: &ImageMetadata) -> bool {
    meta.prompt.as_ref().is_some_and(|s| !s.trim().is_empty())
        || meta.negative_prompt.as_ref().is_some_and(|s| !s.trim().is_empty())
        || extract_seed_from_parameters(meta).is_some()
        || meta
            .raw_parameters
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
        || meta
            .workflow_json
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
}

/// Formats a CLI-style generation command from available metadata.
pub fn format_generation_command(meta: &ImageMetadata) -> String {
    let mut parts = Vec::new();

    if let Some(prompt) = &meta.prompt {
        parts.push(format!("--prompt \"{}\" ", prompt.replace('"', "\\\"")));
    }

    if let Some(neg_prompt) = &meta.negative_prompt {
        parts.push(format!(
            "--negative \"{}\" ",
            neg_prompt.replace('"', "\\\"")
        ));
    }

    if let Some(seed) = extract_seed_from_parameters(meta) {
        parts.push(format!("--seed {} ", seed));
    }

    if parts.is_empty() {
        "comfy-ui-cli".to_string()
    } else {
        format!("comfy-ui-cli {}", parts.join("").trim())
    }
}
