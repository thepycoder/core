//! Shared transform-time provenance for normalized relation rows.
//!
//! Pattern proven in `vote_casts.rs`: artifact id from URL+cache_path, content hash
//! of the bytes read at normalize time, version stamps, and numeric confidence.

use crawl::paths::cache_dir;
use crawl::report_blocks::read_report_html;
use crawl::{
    BLOCK_PARSER_VERSION, REPORT_BLOCK_EXTRACTOR_VERSION, VOTE_EXTRACTOR_VERSION, artifact_id,
    content_hash, content_hash_bytes,
};
use std::collections::HashMap;

/// Identity-resolution confidence for an exact person match.
pub const CONFIDENCE_EXACT: f64 = 1.0;
/// Parsed / structured extraction without an identity ambiguity.
pub const CONFIDENCE_PARSED: f64 = 0.8;
/// Heuristic or partial match.
pub const CONFIDENCE_HEURISTIC: f64 = 0.5;

#[derive(Debug, Clone, Default)]
pub struct Provenance {
    pub source_url: String,
    pub cache_path: String,
    pub source_artifact_id: String,
    pub source_content_hash: String,
    pub block_parser_version: String,
    pub extractor_version: String,
    pub confidence: f64,
}

impl Provenance {
    pub fn for_cache(
        source_url: &str,
        cache_path: &str,
        content_hash_value: &str,
        block_parser_version: &str,
        extractor_version: &str,
        confidence: f64,
    ) -> Self {
        Self {
            source_url: source_url.to_string(),
            cache_path: cache_path.to_string(),
            source_artifact_id: if source_url.is_empty() && cache_path.is_empty() {
                String::new()
            } else {
                artifact_id(source_url, cache_path)
            },
            source_content_hash: content_hash_value.to_string(),
            block_parser_version: block_parser_version.to_string(),
            extractor_version: extractor_version.to_string(),
            confidence,
        }
    }
}

/// Cache of content hashes keyed by relative `cache_path`.
#[derive(Default)]
pub struct ContentHashCache {
    hashes: HashMap<String, String>,
}

impl ContentHashCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hash_for(&mut self, cache_path: &str) -> String {
        if cache_path.is_empty() {
            return String::new();
        }
        self.hashes
            .entry(cache_path.to_string())
            .or_insert_with(|| hash_cache_bytes(cache_path))
            .clone()
    }

    pub fn provenance(
        &mut self,
        source_url: &str,
        cache_path: &str,
        block_parser_version: &str,
        extractor_version: &str,
        confidence: f64,
    ) -> Provenance {
        let hash = self.hash_for(cache_path);
        Provenance::for_cache(
            source_url,
            cache_path,
            &hash,
            block_parser_version,
            extractor_version,
            confidence,
        )
    }

    /// Meeting-report rows: block parser + report extractor versions.
    pub fn meeting_report(
        &mut self,
        source_url: &str,
        cache_path: &str,
        confidence: f64,
    ) -> Provenance {
        self.provenance(
            source_url,
            cache_path,
            BLOCK_PARSER_VERSION,
            REPORT_BLOCK_EXTRACTOR_VERSION,
            confidence,
        )
    }

    /// Vote-member rows: block parser + vote extractor versions.
    pub fn vote_report(
        &mut self,
        source_url: &str,
        cache_path: &str,
        confidence: f64,
    ) -> Provenance {
        self.provenance(
            source_url,
            cache_path,
            BLOCK_PARSER_VERSION,
            VOTE_EXTRACTOR_VERSION,
            confidence,
        )
    }

    /// Non-report staging rows (members, dossiers, QRVA, …): no block parser.
    pub fn staging(
        &mut self,
        source_url: &str,
        cache_path: &str,
        extractor_version: &str,
        confidence: f64,
    ) -> Provenance {
        self.provenance(source_url, cache_path, "", extractor_version, confidence)
    }
}

fn hash_cache_bytes(cache_path: &str) -> String {
    let path = cache_dir().join(cache_path);
    if !path.exists() {
        return String::new();
    }
    if looks_like_report_html(cache_path) {
        return read_report_html(&path)
            .map(|html| content_hash(&html))
            .unwrap_or_default();
    }
    std::fs::read(path)
        .map(|bytes| content_hash_bytes(&bytes))
        .unwrap_or_default()
}

fn looks_like_report_html(cache_path: &str) -> bool {
    cache_path.contains("/meetings/") && cache_path.ends_with(".html")
}

/// Map legacy categorical confidence labels to the numeric contract.
pub fn confidence_from_label(label: &str) -> f64 {
    match label {
        "exact" => CONFIDENCE_EXACT,
        "parsed" => CONFIDENCE_PARSED,
        "heuristic" => CONFIDENCE_HEURISTIC,
        other => other.parse().unwrap_or(0.0),
    }
}

pub fn normalize_extractor_version(name: &str) -> String {
    format!("normalize_{name}_v1")
}

/// Build a [`Provenance`] from the flat fields stored on a normalized row.
pub fn provenance_of(
    source_url: &str,
    cache_path: &str,
    source_artifact_id: &str,
    source_content_hash: &str,
    block_parser_version: &str,
    extractor_version: &str,
    confidence: f64,
) -> Provenance {
    Provenance {
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
        source_artifact_id: source_artifact_id.to_string(),
        source_content_hash: source_content_hash.to_string(),
        block_parser_version: block_parser_version.to_string(),
        extractor_version: extractor_version.to_string(),
        confidence,
    }
}

/// Arrow schema fields shared by every source-derived normalized table.
pub fn provenance_fields() -> Vec<arrow::datatypes::Field> {
    use arrow::datatypes::{DataType, Field};
    use identity::parquet_io::utf8_field;
    vec![
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
        utf8_field("source_artifact_id", false),
        utf8_field("source_content_hash", false),
        utf8_field("block_parser_version", false),
        utf8_field("extractor_version", false),
        Field::new("confidence", DataType::Float64, false),
    ]
}

pub fn provenance_columns(rows: impl Iterator<Item = Provenance>) -> Vec<arrow::array::ArrayRef> {
    use arrow::array::{ArrayRef, Float64Array, StringArray};
    use std::sync::Arc;

    let rows: Vec<_> = rows.collect();
    vec![
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.source_url.as_str())
                .collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.cache_path.as_str())
                .collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.source_artifact_id.as_str())
                .collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.source_content_hash.as_str())
                .collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.block_parser_version.as_str())
                .collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.extractor_version.as_str())
                .collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(Float64Array::from(
            rows.iter().map(|r| r.confidence).collect::<Vec<_>>(),
        )) as ArrayRef,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_labels() {
        assert_eq!(confidence_from_label("exact"), 1.0);
        assert_eq!(confidence_from_label("parsed"), 0.8);
        assert_eq!(confidence_from_label("0.42"), 0.42);
    }

    #[test]
    fn artifact_id_stable() {
        let p = Provenance::for_cache(
            "https://example.com",
            "cache/x.html",
            "abc",
            "",
            "normalize_test_v1",
            1.0,
        );
        assert_eq!(
            p.source_artifact_id,
            artifact_id("https://example.com", "cache/x.html")
        );
    }
}
