pub struct UpdateInfo {
    pub version: String,
    pub url: String,
}

pub fn check_for_update() -> Option<UpdateInfo> {
    let current = env!("CARGO_PKG_VERSION");
    let response = ureq::get("https://api.github.com/repos/OWNER/REPO/releases/latest")
        .set("User-Agent", "lumen-node")
        .call()
        .ok()?;
    let json: serde_json::Value = response.into_json().ok()?;
    let latest = json["tag_name"].as_str()?.trim_start_matches('v');
    if latest != current {
        Some(UpdateInfo {
            version: latest.to_string(),
            url: json["html_url"].as_str()?.to_string(),
        })
    } else {
        None
    }
}
