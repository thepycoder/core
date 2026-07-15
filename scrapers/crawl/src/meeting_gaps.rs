//! Shared meeting gap schema and discovery helpers for plenary and commission.

use arrow::array::{Array, ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use crate::source_manifest::{
    MANIFEST_STATUS_NOT_FOUND, MANIFEST_STATUS_PARSED, MANIFEST_STATUS_UNSUPPORTED_FORMAT,
};

pub const GAP_REASON_NOT_FOUND: &str = "not_found";
pub const GAP_REASON_UNSUPPORTED_FORMAT: &str = "unsupported_format";
pub const GAP_REASON_NO_RESULT: &str = "no_result";

pub const ACCEPTED_GAP_REASONS: &[&str] = &[
    GAP_REASON_NOT_FOUND,
    GAP_REASON_UNSUPPORTED_FORMAT,
    GAP_REASON_NO_RESULT,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingGapRow {
    pub session_id: u32,
    pub meeting_kind: String,
    pub meeting_id: u32,
    pub reason: String,
    pub detail: String,
    pub source_url: String,
    pub cache_path: String,
    pub content_hash: String,
    pub fetched_at: String,
    pub checked_at: String,
}

impl MeetingGapRow {
    pub fn new(
        session_id: u32,
        meeting_kind: &str,
        meeting_id: u32,
        reason: &str,
        detail: impl Into<String>,
        source_url: impl Into<String>,
        cache_path: impl Into<String>,
    ) -> Self {
        Self {
            session_id,
            meeting_kind: meeting_kind.to_string(),
            meeting_id,
            reason: reason.to_string(),
            detail: detail.into(),
            source_url: source_url.into(),
            cache_path: cache_path.into(),
            content_hash: String::new(),
            fetched_at: String::new(),
            checked_at: String::new(),
        }
    }

    pub fn with_hashes(mut self, content_hash: impl Into<String>) -> Self {
        self.content_hash = content_hash.into();
        self
    }

    pub fn with_timestamps(
        mut self,
        fetched_at: impl Into<String>,
        checked_at: impl Into<String>,
    ) -> Self {
        self.fetched_at = fetched_at.into();
        self.checked_at = checked_at.into();
        self
    }
}

pub fn record_gap(gaps: &mut BTreeMap<u32, MeetingGapRow>, gap: MeetingGapRow) {
    gaps.entry(gap.meeting_id).or_insert(gap);
}

/// Insert or replace a gap (used when reclassifying e.g. PDF unsupported_format).
pub fn upsert_gap(gaps: &mut BTreeMap<u32, MeetingGapRow>, gap: MeetingGapRow) {
    gaps.insert(gap.meeting_id, gap);
}

/// Result of sequential ID discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryResult {
    /// Last confirmed existing meeting ID (may equal `start_id` if nothing new).
    pub last_id: u32,
    /// Interior missing IDs strictly between `start_id` and `last_id` (inclusive of holes).
    /// Trailing consecutive misses used only as the stop signal are excluded.
    pub interior_missing: Vec<u32>,
}

/// Scan forward from `start_id`, treating `exists(probe_id)` as whether the report is online.
/// Stops after `max_consecutive_misses` consecutive missing ids.
///
/// Trailing boundary misses are not gaps — only interior holes are returned.
pub fn discover_last_from_probes(
    start_id: u32,
    max_consecutive_misses: u32,
    mut exists: impl FnMut(u32) -> bool,
) -> DiscoveryResult {
    let mut last = start_id;
    let mut consecutive_misses = 0u32;
    let mut probe = start_id + 1;
    let mut missing_streak = Vec::new();
    let mut interior_missing = Vec::new();

    while consecutive_misses < max_consecutive_misses {
        if exists(probe) {
            // Any misses accumulated since the previous hit are interior (or between
            // start and this newly found id).
            interior_missing.append(&mut missing_streak);
            last = probe;
            consecutive_misses = 0;
        } else {
            missing_streak.push(probe);
            consecutive_misses += 1;
        }
        probe += 1;
    }

    // `missing_streak` now holds only the trailing stop-signal misses — drop them.
    DiscoveryResult {
        last_id: last,
        interior_missing,
    }
}

pub fn write_meeting_gaps_parquet(
    path: &Path,
    gaps: &[MeetingGapRow],
) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_kind", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("reason", DataType::Utf8, false),
        Field::new("detail", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
        Field::new("content_hash", DataType::Utf8, false),
        Field::new("fetched_at", DataType::Utf8, false),
        Field::new("checked_at", DataType::Utf8, false),
    ]));

    let columns: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(
            gaps.iter()
                .map(|g| g.session_id.to_string())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            gaps.iter()
                .map(|g| g.meeting_kind.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            gaps.iter()
                .map(|g| g.meeting_id.to_string())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            gaps.iter().map(|g| g.reason.as_str()).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            gaps.iter().map(|g| g.detail.as_str()).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            gaps.iter()
                .map(|g| g.source_url.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            gaps.iter()
                .map(|g| g.cache_path.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            gaps.iter()
                .map(|g| g.content_hash.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            gaps.iter()
                .map(|g| g.fetched_at.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            gaps.iter()
                .map(|g| g.checked_at.as_str())
                .collect::<Vec<_>>(),
        )),
    ];

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let batch = RecordBatch::try_new(schema.clone(), columns)?;
    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

pub fn read_meeting_gaps_parquet(path: &Path) -> Result<Vec<MeetingGapRow>, Box<dyn Error>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch?;
        let session_ids = string_col(&batch, "session_id")?;
        let kinds = string_col(&batch, "meeting_kind")
            .unwrap_or_else(|_| vec!["commission".to_string(); batch.num_rows()]);
        let meeting_ids = string_col(&batch, "meeting_id")?;
        let reasons = string_col(&batch, "reason")?;
        let details = string_col(&batch, "detail")?;
        let urls = string_col(&batch, "source_url")
            .unwrap_or_else(|_| vec![String::new(); batch.num_rows()]);
        let caches = string_col(&batch, "cache_path")
            .unwrap_or_else(|_| vec![String::new(); batch.num_rows()]);
        let hashes = string_col(&batch, "content_hash")
            .unwrap_or_else(|_| vec![String::new(); batch.num_rows()]);
        let fetched = string_col(&batch, "fetched_at")
            .unwrap_or_else(|_| vec![String::new(); batch.num_rows()]);
        let checked = string_col(&batch, "checked_at")
            .unwrap_or_else(|_| vec![String::new(); batch.num_rows()]);
        for i in 0..batch.num_rows() {
            let meeting_id: u32 = meeting_ids[i].parse().unwrap_or(0);
            let session_id: u32 = session_ids[i].parse().unwrap_or(0);
            rows.push(MeetingGapRow {
                session_id,
                meeting_kind: kinds[i].clone(),
                meeting_id,
                reason: reasons[i].clone(),
                detail: details[i].clone(),
                source_url: urls[i].clone(),
                cache_path: caches[i].clone(),
                content_hash: hashes[i].clone(),
                fetched_at: fetched[i].clone(),
                checked_at: checked[i].clone(),
            });
        }
    }
    Ok(rows)
}

fn string_col(batch: &RecordBatch, name: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let idx = batch.schema().index_of(name)?;
    let col = batch.column(idx);
    let arr = col
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| format!("column {name} is not Utf8"))?;
    Ok((0..arr.len())
        .map(|i| {
            if arr.is_null(i) {
                String::new()
            } else {
                arr.value(i).to_string()
            }
        })
        .collect())
}

/// Every ID in `1..=boundary` must appear in exactly one of `parsed_ids` or `gap_ids`.
pub fn reconcile_meeting_coverage(
    boundary: u32,
    parsed_ids: &BTreeSet<u32>,
    gaps: &BTreeMap<u32, MeetingGapRow>,
) -> Result<(), Box<dyn Error>> {
    let gap_ids: BTreeSet<u32> = gaps.keys().copied().collect();
    let overlap: Vec<_> = parsed_ids.intersection(&gap_ids).copied().collect();
    if !overlap.is_empty() {
        return Err(format!(
            "meeting coverage overlap: ids {:?} appear in both parsed rows and gaps",
            &overlap[..overlap.len().min(8)]
        )
        .into());
    }

    for id in 1..=boundary {
        let in_parsed = parsed_ids.contains(&id);
        let in_gaps = gap_ids.contains(&id);
        if in_parsed == in_gaps {
            // both true already handled; both false is incomplete
            if !in_parsed && !in_gaps {
                return Err(format!(
                    "meeting coverage incomplete: id {id} is neither parsed nor an accepted gap (boundary={boundary})"
                )
                .into());
            }
        }
        if in_gaps {
            let reason = &gaps[&id].reason;
            if !ACCEPTED_GAP_REASONS.contains(&reason.as_str()) {
                return Err(format!(
                    "meeting gap {id} has disallowed reason {reason:?}; parser failures must abort, not publish"
                )
                .into());
            }
        }
    }

    // Gaps beyond the boundary are unexpected.
    for id in &gap_ids {
        if *id > boundary {
            return Err(format!("meeting gap {id} is beyond discovery boundary {boundary}").into());
        }
    }
    Ok(())
}

/// Map a meeting gap reason onto a source-manifest status.
pub fn gap_reason_to_manifest_status(reason: &str) -> &'static str {
    match reason {
        GAP_REASON_NOT_FOUND => MANIFEST_STATUS_NOT_FOUND,
        GAP_REASON_UNSUPPORTED_FORMAT => MANIFEST_STATUS_UNSUPPORTED_FORMAT,
        GAP_REASON_NO_RESULT => crate::source_manifest::MANIFEST_STATUS_NO_RESULT,
        _ => MANIFEST_STATUS_NOT_FOUND,
    }
}

pub fn manifest_status_for_parsed() -> &'static str {
    MANIFEST_STATUS_PARSED
}

/// Load prior accepted gaps (for cache-only reconciliation of known remote holes).
/// Migrates legacy three-column commission gap files (`meeting_id`, `reason`, `detail`).
pub fn load_prior_gaps(
    path: &Path,
    session_id: u32,
    meeting_kind: &str,
) -> Result<BTreeMap<u32, MeetingGapRow>, Box<dyn Error>> {
    let mut map = BTreeMap::new();
    if !path.exists() {
        return Ok(map);
    }

    // Prefer the shared reader; if the legacy schema lacks meeting_kind etc., fall back.
    match read_meeting_gaps_parquet(path) {
        Ok(rows) => {
            for mut row in rows {
                if row.meeting_kind.is_empty() {
                    row.meeting_kind = meeting_kind.to_string();
                }
                if row.session_id == 0 {
                    row.session_id = session_id;
                }
                // Drop legacy parse_failed — caller must reclassify or abort.
                if ACCEPTED_GAP_REASONS.contains(&row.reason.as_str()) {
                    map.insert(row.meeting_id, row);
                } else if row.reason == "parse_failed" {
                    // Heuristic migration from the July 2026 snapshot:
                    // cache-missing → not_found; PDF/"No table" → unsupported_format.
                    let detail = row.detail.to_lowercase();
                    let reason = if detail.contains("cache missing") {
                        GAP_REASON_NOT_FOUND
                    } else if detail.contains("no table") || detail.contains("pdf") {
                        GAP_REASON_UNSUPPORTED_FORMAT
                    } else {
                        continue;
                    };
                    row.reason = reason.to_string();
                    row.session_id = session_id;
                    row.meeting_kind = meeting_kind.to_string();
                    map.insert(row.meeting_id, row);
                }
            }
        }
        Err(_) => {
            // Very old files: try reading just the three columns via a looser path.
            let file = File::open(path)?;
            let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
            for batch in reader {
                let batch = batch?;
                let meeting_ids = string_col(&batch, "meeting_id")?;
                let reasons = string_col(&batch, "reason")?;
                let details = string_col(&batch, "detail")?;
                for i in 0..batch.num_rows() {
                    let meeting_id: u32 = meeting_ids[i].parse().unwrap_or(0);
                    let mut reason = reasons[i].as_str();
                    let detail = &details[i];
                    if reason == "parse_failed" {
                        let d = detail.to_lowercase();
                        if d.contains("cache missing") {
                            reason = GAP_REASON_NOT_FOUND;
                        } else if d.contains("no table") || d.contains("pdf") {
                            reason = GAP_REASON_UNSUPPORTED_FORMAT;
                        } else {
                            continue;
                        }
                    }
                    if !ACCEPTED_GAP_REASONS.contains(&reason) {
                        continue;
                    }
                    map.insert(
                        meeting_id,
                        MeetingGapRow::new(
                            session_id,
                            meeting_kind,
                            meeting_id,
                            reason,
                            detail.clone(),
                            "",
                            "",
                        ),
                    );
                }
            }
        }
    }
    Ok(map)
}

/// Helper for tests and callers that only need the ID set.
pub fn gap_id_set(gaps: &BTreeMap<u32, MeetingGapRow>) -> HashSet<u32> {
    gaps.keys().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_interior_gap_excludes_trailing_misses() {
        let exists = |id: u32| id != 67 && id <= 70;
        let result = discover_last_from_probes(66, 2, exists);
        assert_eq!(result.last_id, 70);
        assert_eq!(result.interior_missing, vec![67]);
    }

    #[test]
    fn discover_stops_after_two_consecutive_misses() {
        let exists = |id: u32| id == 68;
        let result = discover_last_from_probes(66, 2, exists);
        assert_eq!(result.last_id, 68);
        assert_eq!(result.interior_missing, vec![67]);
    }

    #[test]
    fn trailing_404_only_is_not_a_gap() {
        let exists = |_id: u32| false;
        let result = discover_last_from_probes(10, 2, exists);
        assert_eq!(result.last_id, 10);
        assert!(result.interior_missing.is_empty());
    }

    #[test]
    fn reconcile_requires_full_coverage() {
        let mut parsed = BTreeSet::new();
        parsed.insert(1);
        parsed.insert(2);
        let mut gaps = BTreeMap::new();
        gaps.insert(
            3,
            MeetingGapRow::new(
                56,
                "commission",
                3,
                GAP_REASON_NOT_FOUND,
                "HTTP 404",
                "",
                "",
            ),
        );
        assert!(reconcile_meeting_coverage(3, &parsed, &gaps).is_ok());

        // Missing id 2
        parsed.remove(&2);
        assert!(reconcile_meeting_coverage(3, &parsed, &gaps).is_err());
    }

    #[test]
    fn reconcile_rejects_parse_failed() {
        let parsed = BTreeSet::from([1u32]);
        let mut gaps = BTreeMap::new();
        gaps.insert(
            2,
            MeetingGapRow::new(56, "commission", 2, "parse_failed", "boom", "", ""),
        );
        assert!(reconcile_meeting_coverage(2, &parsed, &gaps).is_err());
    }

    #[test]
    fn reconcile_rejects_overlap() {
        let parsed = BTreeSet::from([1u32]);
        let mut gaps = BTreeMap::new();
        gaps.insert(
            1,
            MeetingGapRow::new(56, "plenary", 1, GAP_REASON_NOT_FOUND, "x", "", ""),
        );
        assert!(reconcile_meeting_coverage(1, &parsed, &gaps).is_err());
    }
}
