pub struct UpdateInfo {
    pub version: String,
    pub url: String,
}

/// Parse a version string like "1.2.3" into its numeric components.
/// Returns [major, minor, patch], defaulting missing components to 0.
fn parse_version(v: &str) -> [u32; 3] {
    let mut parts = [0u32; 3];
    for (i, seg) in v.split('.').take(3).enumerate() {
        parts[i] = seg.parse::<u32>().unwrap_or(0);
    }
    parts
}

/// Returns true when `latest` is strictly newer than `current`.
fn is_newer_than(current: &str, latest: &str) -> bool {
    let cur = parse_version(current);
    let lat = parse_version(latest);
    cur < lat
}

pub fn check_for_update() -> Option<UpdateInfo> {
    let current = env!("CARGO_PKG_VERSION");
    let response = ureq::get("https://api.github.com/repos/mholtzhausen/lumen-node/releases/latest")
        .set("User-Agent", "lumen-node")
        .call()
        .ok()?;
    let json: serde_json::Value = response.into_json().ok()?;
    let latest = json["tag_name"].as_str()?.trim_start_matches('v');
    if is_newer_than(current, latest) {
        Some(UpdateInfo {
            version: latest.to_string(),
            url: json["html_url"].as_str()?.to_string(),
        })
    } else {
        None
    }
}
