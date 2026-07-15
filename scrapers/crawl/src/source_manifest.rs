//! Canonical per-source inventory manifests under `data/source_manifests/`.
//!
//! A live scrape establishes the contract. Status vocabulary is closed:
//! `parsed`, `no_result`, `not_found`, `unsupported_format`.

use arrow::array::{ArrayRef, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::atomic_io::{BundlePublisher, write_bytes_atomic};
use crate::paths::data_dir;

pub const MANIFEST_STATUS_PARSED: &str = "parsed";
pub const MANIFEST_STATUS_NO_RESULT: &str = "no_result";
pub const MANIFEST_STATUS_NOT_FOUND: &str = "not_found";
pub const MANIFEST_STATUS_UNSUPPORTED_FORMAT: &str = "unsupported_format";

pub const MANIFEST_STATUSES: &[&str] = &[
    MANIFEST_STATUS_PARSED,
    MANIFEST_STATUS_NO_RESULT,
    MANIFEST_STATUS_NOT_FOUND,
    MANIFEST_STATUS_UNSUPPORTED_FORMAT,
];

#[derive(Debug, Clone)]
pub struct SourceManifestRow {
    pub source: String,
    pub session_id: String,
    pub item_kind: String,
    pub native_item_id: String,
    pub source_url: String,
    pub cache_path: String,
    pub status: String,
    pub row_count: u32,
    pub content_type: String,
    pub content_hash: String,
    pub fetched_at: String,
    pub checked_at: String,
    pub run_mode: String,
    pub detail: String,
}

impl SourceManifestRow {
    pub fn run_mode_live() -> &'static str {
        "live"
    }

    pub fn run_mode_cache_only() -> &'static str {
        "cache_only"
    }
}

pub fn manifest_path(source: &str) -> PathBuf {
    data_dir()
        .join("source_manifests")
        .join(format!("{source}.parquet"))
}

pub fn is_known_manifest_status(status: &str) -> bool {
    MANIFEST_STATUSES.contains(&status)
}

pub fn write_source_manifest(
    path: &Path,
    rows: &[SourceManifestRow],
) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("source", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("item_kind", DataType::Utf8, false),
        Field::new("native_item_id", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
        Field::new("status", DataType::Utf8, false),
        Field::new("row_count", DataType::UInt32, false),
        Field::new("content_type", DataType::Utf8, false),
        Field::new("content_hash", DataType::Utf8, false),
        Field::new("fetched_at", DataType::Utf8, false),
        Field::new("checked_at", DataType::Utf8, false),
        Field::new("run_mode", DataType::Utf8, false),
        Field::new("detail", DataType::Utf8, false),
    ]));

    let columns: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(
            rows.iter().map(|r| r.source.as_str()).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.session_id.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.item_kind.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.native_item_id.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.source_url.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.cache_path.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter().map(|r| r.status.as_str()).collect::<Vec<_>>(),
        )),
        Arc::new(UInt32Array::from(
            rows.iter().map(|r| r.row_count).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.content_type.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.content_hash.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.fetched_at.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.checked_at.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter().map(|r| r.run_mode.as_str()).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter().map(|r| r.detail.as_str()).collect::<Vec<_>>(),
        )),
    ];

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write via temp then rename so a partial writer cannot leave a truncated
    // canonical manifest.
    let tmp = PathBuf::from(format!("{}.writing", path.display()));
    {
        let batch = RecordBatch::try_new(schema.clone(), columns)?;
        let mut writer = ArrowWriter::try_new(File::create(&tmp)?, schema, None)?;
        writer.write(&batch)?;
        writer.close()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Stage a manifest into a [`BundlePublisher`] and return the staging path written.
pub fn stage_source_manifest(
    bundle: &mut BundlePublisher,
    final_path: &Path,
    rows: &[SourceManifestRow],
) -> Result<PathBuf, Box<dyn Error>> {
    let staging = bundle.stage_path(final_path)?;
    write_source_manifest(&staging, rows)?;
    Ok(staging)
}

/// Validate manifest rows before publication.
pub fn validate_manifest_rows(rows: &[SourceManifestRow]) -> Result<(), Box<dyn Error>> {
    use std::collections::HashSet;
    let mut keys = HashSet::new();
    for row in rows {
        if !is_known_manifest_status(&row.status) {
            return Err(format!(
                "unknown manifest status {:?} for {}/{}",
                row.status, row.source, row.native_item_id
            )
            .into());
        }
        let key = (
            row.source.clone(),
            row.session_id.clone(),
            row.item_kind.clone(),
            row.native_item_id.clone(),
        );
        if !keys.insert(key) {
            return Err(format!(
                "duplicate manifest key {}/{}/{}/{}",
                row.source, row.session_id, row.item_kind, row.native_item_id
            )
            .into());
        }
    }
    Ok(())
}

/// Helper used by tests: write a tiny sidecar atomically.
#[allow(dead_code)]
pub fn write_json_sidecar(path: &Path, value: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    write_bytes_atomic(path, serde_json::to_string_pretty(value)?.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_status() {
        let rows = vec![SourceManifestRow {
            source: "commission_meetings".into(),
            session_id: "56".into(),
            item_kind: "meeting".into(),
            native_item_id: "1".into(),
            source_url: String::new(),
            cache_path: String::new(),
            status: "parse_failed".into(),
            row_count: 0,
            content_type: String::new(),
            content_hash: String::new(),
            fetched_at: String::new(),
            checked_at: String::new(),
            run_mode: "live".into(),
            detail: String::new(),
        }];
        assert!(validate_manifest_rows(&rows).is_err());
    }

    #[test]
    fn rejects_duplicate_keys() {
        let row = SourceManifestRow {
            source: "plenary_meetings".into(),
            session_id: "56".into(),
            item_kind: "meeting".into(),
            native_item_id: "1".into(),
            source_url: String::new(),
            cache_path: String::new(),
            status: MANIFEST_STATUS_PARSED.into(),
            row_count: 1,
            content_type: String::new(),
            content_hash: String::new(),
            fetched_at: String::new(),
            checked_at: String::new(),
            run_mode: "cache_only".into(),
            detail: String::new(),
        };
        assert!(validate_manifest_rows(&[row.clone(), row]).is_err());
    }
}
