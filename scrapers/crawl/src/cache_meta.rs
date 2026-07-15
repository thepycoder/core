//! Cache artifact metadata sidecars and safe cache writes.
//!
//! Each cached file may have a sibling `{filename}.meta.json` with provenance
//! timestamps and the raw content hash. Cache-only mode must not mutate metadata.

use crate::artifact_id::content_hash_bytes;
use crate::atomic_io::write_bytes_atomic;
use crate::paths::cache_only;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Sidecar written beside a cached artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheMetadata {
    pub source_url: String,
    pub content_type: String,
    /// SHA-256 of the raw bytes on disk.
    pub content_hash: String,
    /// RFC3339 timestamp when bytes were last fetched from the network.
    pub fetched_at: String,
    /// RFC3339 timestamp when the artifact was last verified (fetch or live check).
    pub checked_at: String,
}

impl CacheMetadata {
    pub fn new(
        source_url: impl Into<String>,
        content_type: impl Into<String>,
        content_hash: impl Into<String>,
        fetched_at: impl Into<String>,
        checked_at: impl Into<String>,
    ) -> Self {
        Self {
            source_url: source_url.into(),
            content_type: content_type.into(),
            content_hash: content_hash.into(),
            fetched_at: fetched_at.into(),
            checked_at: checked_at.into(),
        }
    }
}

pub fn meta_path_for(cache_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.meta.json", cache_path.display()))
}

pub fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Sufficient for provenance ordering; avoid pulling chrono into crawl for this.
    format_unix_secs(secs)
}

fn format_unix_secs(secs: u64) -> String {
    // Approximate UTC formatting without chrono: use a fixed epoch converter.
    // Days since Unix epoch → Y-M-D, then h:m:s.
    const SECS_PER_DAY: u64 = 86_400;
    let days = secs / SECS_PER_DAY;
    let tod = secs % SECS_PER_DAY;
    let (y, m, d) = civil_from_days(days as i64);
    let hh = tod / 3600;
    let mm = (tod % 3600) / 60;
    let ss = tod % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Howard Hinnant's civil-from-days (days since 1970-01-01 → Y-M-D).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

pub fn read_cache_metadata(cache_path: &Path) -> Result<Option<CacheMetadata>, Box<dyn Error>> {
    let meta = meta_path_for(cache_path);
    if !meta.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&meta)?;
    Ok(Some(serde_json::from_str(&text)?))
}

pub fn write_cache_metadata(cache_path: &Path, meta: &CacheMetadata) -> Result<(), Box<dyn Error>> {
    let path = meta_path_for(cache_path);
    let json = serde_json::to_string_pretty(meta)?;
    write_bytes_atomic(&path, json.as_bytes())
}

/// True when bytes look like a PDF (`%PDF` magic).
pub fn looks_like_pdf(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[..4] == b"%PDF"
}

/// Write cache bytes and metadata. In cache-only mode this returns an error
/// (callers must not invent network content).
pub fn write_cache_artifact(
    cache_path: &Path,
    bytes: &[u8],
    source_url: &str,
    content_type: &str,
) -> Result<CacheMetadata, Box<dyn Error>> {
    if cache_only() {
        return Err("refusing to write cache artifact while SCRAPER_CACHE_ONLY is set".into());
    }
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_bytes_atomic(cache_path, bytes)?;
    let now = now_rfc3339();
    let meta = CacheMetadata::new(
        source_url,
        content_type,
        content_hash_bytes(bytes),
        &now,
        &now,
    );
    write_cache_metadata(cache_path, &meta)?;
    Ok(meta)
}

/// Touch `checked_at` on an existing artifact without changing bytes.
/// No-op (and no error) when cache-only.
pub fn touch_checked_at(cache_path: &Path) -> Result<(), Box<dyn Error>> {
    if cache_only() {
        return Ok(());
    }
    let mut meta = read_cache_metadata(cache_path)?.unwrap_or_else(|| {
        let hash = fs::read(cache_path)
            .map(|b| content_hash_bytes(&b))
            .unwrap_or_default();
        let now = now_rfc3339();
        CacheMetadata::new("", "application/octet-stream", hash, &now, &now)
    });
    meta.checked_at = now_rfc3339();
    write_cache_metadata(cache_path, &meta)
}

/// Require that a previously known cache file still exists (cache-only safety).
pub fn require_cache_present(cache_path: &Path, label: &str) -> Result<(), Box<dyn Error>> {
    if cache_path.exists() {
        return Ok(());
    }
    Err(format!(
        "cache-only incomplete snapshot: expected {label} at {} is missing — aborting to preserve prior outputs",
        cache_path.display()
    )
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn pdf_magic_detection() {
        assert!(looks_like_pdf(b"%PDF-1.4\n"));
        assert!(!looks_like_pdf(b"<html>"));
    }

    #[test]
    fn write_and_read_metadata_roundtrip() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Ensure not cache-only for this write.
        unsafe { std::env::remove_var("SCRAPER_CACHE_ONLY") };

        let dir = std::env::temp_dir().join(format!(
            "crawl-meta-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.html");
        let meta =
            write_cache_artifact(&path, b"<html/>", "https://example.com/x", "text/html").unwrap();
        assert_eq!(meta.source_url, "https://example.com/x");
        assert!(path.exists());
        let loaded = read_cache_metadata(&path).unwrap().unwrap();
        assert_eq!(loaded.content_hash, meta.content_hash);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cache_only_refuses_write() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("SCRAPER_CACHE_ONLY", "1") };
        let dir = std::env::temp_dir().join(format!(
            "crawl-meta-co-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.html");
        let err =
            write_cache_artifact(&path, b"x", "https://example.com", "text/html").unwrap_err();
        assert!(err.to_string().contains("SCRAPER_CACHE_ONLY"));
        unsafe { std::env::remove_var("SCRAPER_CACHE_ONLY") };
        fs::remove_dir_all(&dir).unwrap();
    }
}
