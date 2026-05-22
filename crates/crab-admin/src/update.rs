use serde::Deserialize;

const GITHUB_API: &str = "https://api.github.com";
const REPO_OWNER: &str = "smile9493";
const REPO_NAME: &str = "CrabCache";

/// A single asset (binary file) from a GitHub Release.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

/// Information about the latest GitHub Release.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub published_at: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

/// Result of checking for updates on GitHub.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release: Option<ReleaseInfo>,
}

/// Download a file from a URL to a local path.
pub async fn download_asset(url: &str, dest: &std::path::Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent("CrabCache-Admin-Updater")
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Download returned HTTP {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {e}", parent.display()))?;
    }

    std::fs::write(dest, &bytes)
        .map_err(|e| format!("Failed to write file {}: {e}", dest.display()))?;

    Ok(())
}

/// Query GitHub for the latest release of the repository.
pub async fn check_latest_release() -> Result<ReleaseInfo, String> {
    let url = format!("{GITHUB_API}/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");

    let client = reqwest::Client::builder()
        .user_agent("CrabCache-Admin-Updater")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("GitHub API returned HTTP {status}: {body}"));
    }

    let release: ReleaseInfo = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub release: {e}"))?;

    Ok(release)
}

/// Check if an update is available compared to the current version.
pub async fn check_update(current_version: &str) -> Result<UpdateCheckResult, String> {
    let release = check_latest_release().await?;

    let latest_version = release.tag_name.trim_start_matches('v');
    let update_available = latest_version != current_version;

    Ok(UpdateCheckResult {
        current_version: current_version.to_string(),
        latest_version: latest_version.to_string(),
        update_available,
        release: Some(release),
    })
}

/// Verify a downloaded file against a SHA256 checksum file from the release.
/// Downloads the `.sha256` checksum file for a given asset and verifies it.
pub async fn verify_checksum(
    asset_name: &str,
    file_path: &std::path::Path,
    release: &ReleaseInfo,
) -> Result<(), String> {
    let checksum_name = format!("{asset_name}.sha256");
    let checksum_asset = release.assets.iter().find(|a| a.name == checksum_name);

    let Some(checksum_asset) = checksum_asset else {
        tracing::warn!(
            asset = asset_name,
            "No checksum file found in release, skipping verification"
        );
        return Ok(());
    };

    let client = reqwest::Client::builder()
        .user_agent("CrabCache-Admin-Updater")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let response = client
        .get(&checksum_asset.browser_download_url)
        .send()
        .await
        .map_err(|e| format!("Checksum download failed: {e}"))?;

    let checksum_text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read checksum: {e}"))?;

    let expected_hex = checksum_text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim();

    if expected_hex.is_empty() {
        return Err("Checksum file is empty or malformed".to_string());
    }

    let file_bytes = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read file for checksum: {e}"))?;

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&file_bytes);
    let actual_hex = hex::encode(hasher.finalize());

    if actual_hex != expected_hex {
        return Err(format!(
            "Checksum mismatch for {asset_name}: expected {expected_hex}, got {actual_hex}"
        ));
    }

    tracing::info!(
        asset = asset_name,
        "Checksum verified successfully"
    );

    Ok(())
}

/// Atomically replace a binary file by writing to a temp location first,
/// then renaming to the final destination.
pub fn replace_binary(src: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    if !src.exists() {
        return Err(format!("Source binary does not exist: {}", src.display()));
    }

    // Copy permissions from destination if it exists
    if dest.exists() {
        if let Ok(meta) = std::fs::metadata(dest) {
            use std::os::unix::fs::PermissionsExt;
            let perms = meta.permissions();
            let mode = perms.mode();
            if let Err(e) = std::fs::set_permissions(src, std::fs::Permissions::from_mode(mode)) {
                tracing::warn!(error = %e, "Failed to preserve binary permissions");
            }
        }
    }

    // Set executable permission
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(src) {
        let mut perms = meta.permissions();
        let current_mode = perms.mode();
        perms.set_mode(current_mode | 0o111);
        let _ = std::fs::set_permissions(src, perms);
    }

    // Atomic rename
    std::fs::rename(src, dest)
        .map_err(|e| format!("Failed to replace binary at {}: {e}", dest.display()))?;

    tracing::info!(dest = %dest.display(), "Binary replaced successfully");
    Ok(())
}

/// Send a restart request to the gateway's management API.
pub async fn restart_gateway(gateway_control_url: &str, admin_key: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let url = format!("{}/v1/system/restart", gateway_control_url.trim_end_matches('/'));
    let response = client
        .post(&url)
        .header("x-gateway-admin-key", admin_key)
        .send()
        .await
        .map_err(|e| format!("Gateway restart request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Gateway restart returned HTTP {}", response.status()));
    }

    tracing::info!("Gateway restart triggered successfully");
    Ok(())
}

/// Perform self-update: write a restart script and exit the current process.
/// The restart script waits for the old process to exit, then replaces the
/// old binary with the new one and starts it with the same command-line arguments.
pub fn self_update_and_restart(new_binary: &std::path::Path) -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to get current executable path: {e}"))?;

    let args: Vec<String> = std::env::args().collect();
    let script_path = std::path::PathBuf::from("/tmp/crabcache-update.sh");

    let mut script = String::from("#!/bin/bash\n");
    script.push_str("# Auto-generated by CrabCache self-updater\n");
    script.push_str("sleep 1\n");
    script.push_str(&format!(
        "mv {} {}\n",
        new_binary.display(),
        current_exe.display()
    ));
    script.push_str(&format!("chmod +x {}\n", current_exe.display()));
    script.push_str(&format!(
        "echo 'Starting updated CrabCache admin...'\n"
    ));
    script.push_str(&format!(
        "exec {} {}\n",
        current_exe.display(),
        args[1..].join(" ")
    ));

    std::fs::write(&script_path, &script)
        .map_err(|e| format!("Failed to write restart script: {e}"))?;

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("Failed to make restart script executable: {e}"))?;

    tracing::info!(
        script = %script_path.display(),
        current_exe = %current_exe.display(),
        new_binary = %new_binary.display(),
        "Launching self-update restart script"
    );

    // Launch the script in background, detached from the current process
    std::process::Command::new("/bin/bash")
        .arg(&script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to launch restart script: {e}"))?;

    Ok(())
}
