//! Self-update against this project's GitHub releases.
//!
//! Flow: read `latest.json` from the latest release, compare versions, download the installer and
//! its detached minisign signature, VERIFY the signature against the baked-in public key, and only
//! then run the installer. An unverified download is never executed.

use std::path::PathBuf;

use crate::http;

const LATEST_JSON: &str =
    "https://github.com/Lumnai/sim-racing-display-fixer/releases/latest/download/latest.json";

/// The public half of the release signing key. The matching private key lives only in the
/// repository's GitHub secrets.
const PUBLIC_KEY: &str = "untrusted comment: minisign public key: 7BE49C53D0749E0A\nRWQKnnTQU5zke+cVD3FTKIjNnjL3rRFxcosOTj8F2AaXj9x8R0AS7f6h\n";

pub struct Available {
    pub version: String,
    pub url: String,
    pub signature: String,
}

fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Compare dotted versions numerically ("0.10.0" > "0.9.9").
fn is_newer(candidate: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parts(candidate), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Ask GitHub whether a newer release exists. Returns None when already current.
pub fn check() -> Result<Option<Available>, String> {
    let raw = http::get(LATEST_JSON, 256 * 1024)
        .map_err(|e| format!("could not reach the update server: {e}"))?;
    let body: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|e| format!("bad update manifest: {e}"))?;

    let version = body["version"].as_str().unwrap_or_default().to_string();
    if version.is_empty() || !is_newer(&version, current_version()) {
        return Ok(None);
    }

    // Tauri-style manifest: platforms -> windows-x86_64 -> {url, signature}
    let plat = body["platforms"]["windows-x86_64"]
        .as_object()
        .or_else(|| body["platforms"]["windows-x86_64-nsis"].as_object())
        .ok_or("this release has no Windows build")?;

    Ok(Some(Available {
        version,
        url: plat["url"].as_str().unwrap_or_default().to_string(),
        signature: plat["signature"].as_str().unwrap_or_default().to_string(),
    }))
}

/// Download, verify, and launch the installer. Returns once the installer has been started.
pub fn download_and_run(avail: &Available) -> Result<(), String> {
    if avail.url.is_empty() || avail.signature.is_empty() {
        return Err("the update is missing its download or signature".into());
    }

    let bytes = http::get(&avail.url, 200 * 1024 * 1024)
        .map_err(|e| format!("download failed: {e}"))?;

    verify(&bytes, &avail.signature)?;

    let path: PathBuf = std::env::temp_dir().join(format!(
        "SimDisplayFixer-{}-setup.exe",
        avail.version.replace(|c: char| !c.is_ascii_alphanumeric() && c != '.', "")
    ));
    std::fs::write(&path, &bytes).map_err(|e| format!("could not save the update: {e}"))?;

    // /S runs the installer silently; it relaunches the app itself when it finishes, so an update
    // is a single click with no wizard to walk through.
    std::process::Command::new(&path)
        .arg("/S")
        .spawn()
        .map_err(|e| format!("could not start the installer: {e}"))?;
    Ok(())
}

/// Reject anything not signed by our release key. The signature blob is base64 in the manifest.
fn verify(bytes: &[u8], signature_b64: &str) -> Result<(), String> {
    use minisign_verify::{PublicKey, Signature};

    let decoded = base64_decode(signature_b64.trim())
        .ok_or("the update signature is malformed")?;
    let sig_text = String::from_utf8(decoded).map_err(|_| "the update signature is malformed")?;

    let pk = PublicKey::from_base64(PUBLIC_KEY.lines().nth(1).unwrap_or_default().trim())
        .map_err(|e| format!("bad signing key: {e}"))?;
    let sig = Signature::decode(&sig_text).map_err(|e| format!("bad signature: {e}"))?;
    pk.verify(bytes, &sig, false)
        .map_err(|_| "this update is not signed by Lunis and was not installed".to_string())
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lut = [255u8; 256];
    for (i, c) in T.iter().enumerate() {
        lut[*c as usize] = i as u8;
    }
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for b in s.bytes() {
        if b == b'=' || b == b'\n' || b == b'\r' {
            continue;
        }
        let v = lut[b as usize];
        if v == 255 {
            return None;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}
