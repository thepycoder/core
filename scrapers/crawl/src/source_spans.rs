//! Generic block provenance spans.

use crate::artifact_id::{BLOCK_PARSER_VERSION, VOTE_EXTRACTOR_VERSION, artifact_id};
use arrow::array::{ArrayRef, Float64Array, RecordBatch, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SourceSpanDraft {
    pub span_id: String,
    pub artifact_id: String,
    pub source_content_hash: String,
    pub session_id: u32,
    pub meeting_id: u32,
    pub entity_type: String,
    pub entity_id: String,
    pub span_role: String,
    pub block_start: u32,
    pub block_end: u32,
    pub coverage_kind: String,
    pub field_names: String,
    pub confidence: f64,
    pub extractor: String,
    pub block_parser_version: String,
    pub extractor_version: String,
    pub source_url: String,
    pub cache_path: String,
    pub validation_status: String,
    pub unresolved_reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct SpanValidationOutput {
    pub rows: Vec<SourceSpanDraft>,
    pub unresolved: Vec<SourceSpanDraft>,
}

pub fn validate_source_spans(
    rows: impl IntoIterator<Item = SourceSpanDraft>,
    artifact: &str,
    source_content_hash: &str,
    block_parser_version: &str,
    block_count: u32,
) -> SpanValidationOutput {
    let mut output = SpanValidationOutput::default();
    for mut row in rows {
        let reason = if row.artifact_id != artifact {
            Some("wrong_artifact")
        } else if row.source_content_hash != source_content_hash {
            Some("stale_source_content")
        } else if row.block_parser_version != block_parser_version {
            Some("stale_block_parser")
        } else if row.entity_id.trim().is_empty() {
            Some("missing_entity_id")
        } else if row.block_start >= row.block_end {
            Some("invalid_half_open_range")
        } else if row.block_end > block_count {
            Some("out_of_bounds")
        } else {
            None
        };
        if let Some(reason) = reason {
            row.validation_status = "unresolved".to_string();
            row.unresolved_reason = reason.to_string();
            output.unresolved.push(row.clone());
        } else {
            row.validation_status = "valid".to_string();
            row.unresolved_reason.clear();
        }
        output.rows.push(row);
    }
    output
}

pub fn span_id(
    artifact: &str,
    entity_type: &str,
    entity_id: &str,
    role: &str,
    start: u32,
    end: u32,
) -> String {
    format!("{artifact}:{entity_type}:{entity_id}:{role}:{start}:{end}")
}

pub fn write_source_spans_parquet(
    path: &Path,
    rows: &[SourceSpanDraft],
) -> Result<(), Box<dyn Error>> {
    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }
    let schema = Arc::new(Schema::new(vec![
        Field::new("span_id", DataType::Utf8, false),
        Field::new("artifact_id", DataType::Utf8, false),
        Field::new("source_content_hash", DataType::Utf8, false),
        Field::new("session_id", DataType::UInt32, false),
        Field::new("meeting_id", DataType::UInt32, false),
        Field::new("entity_type", DataType::Utf8, false),
        Field::new("entity_id", DataType::Utf8, false),
        Field::new("span_role", DataType::Utf8, false),
        Field::new("block_start", DataType::UInt32, false),
        Field::new("block_end", DataType::UInt32, false),
        Field::new("coverage_kind", DataType::Utf8, false),
        Field::new("field_names", DataType::Utf8, false),
        Field::new("confidence", DataType::Float64, false),
        Field::new("extractor", DataType::Utf8, false),
        Field::new("block_parser_version", DataType::Utf8, false),
        Field::new("extractor_version", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
        Field::new("validation_status", DataType::Utf8, false),
        Field::new("unresolved_reason", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            col!(|r| r.span_id.clone()),
            col!(|r| r.artifact_id.clone()),
            col!(|r| r.source_content_hash.clone()),
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.session_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.meeting_id).collect::<Vec<_>>(),
            )) as ArrayRef,
            col!(|r| r.entity_type.clone()),
            col!(|r| r.entity_id.clone()),
            col!(|r| r.span_role.clone()),
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.block_start).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.block_end).collect::<Vec<_>>(),
            )) as ArrayRef,
            col!(|r| r.coverage_kind.clone()),
            col!(|r| r.field_names.clone()),
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.confidence).collect::<Vec<_>>(),
            )) as ArrayRef,
            col!(|r| r.extractor.clone()),
            col!(|r| r.block_parser_version.clone()),
            col!(|r| r.extractor_version.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.validation_status.clone()),
            col!(|r| r.unresolved_reason.clone()),
        ],
    )?;
    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

pub fn vote_extractor_version() -> &'static str {
    VOTE_EXTRACTOR_VERSION
}

pub fn block_parser_version() -> &'static str {
    BLOCK_PARSER_VERSION
}

pub fn make_artifact_id(source_url: &str, cache_path: &str) -> String {
    artifact_id(source_url, cache_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(id: &str, start: u32, end: u32) -> SourceSpanDraft {
        SourceSpanDraft {
            span_id: id.to_string(),
            artifact_id: "artifact".to_string(),
            source_content_hash: "content".to_string(),
            session_id: 56,
            meeting_id: 1,
            entity_type: "Question".to_string(),
            entity_id: id.to_string(),
            span_role: "entity_title".to_string(),
            block_start: start,
            block_end: end,
            coverage_kind: "extraction".to_string(),
            field_names: "title_nl".to_string(),
            confidence: 1.0,
            extractor: "test".to_string(),
            block_parser_version: BLOCK_PARSER_VERSION.to_string(),
            extractor_version: "test_v1".to_string(),
            source_url: "url".to_string(),
            cache_path: "cache".to_string(),
            validation_status: String::new(),
            unresolved_reason: String::new(),
        }
    }

    #[test]
    fn validates_half_open_bounds_and_preserves_overlaps() {
        let rows = vec![
            draft("left", 1, 3),
            draft("overlap", 2, 4),
            draft("empty", 3, 3),
            draft("past-end", 4, 6),
        ];
        let output = validate_source_spans(rows, "artifact", "content", BLOCK_PARSER_VERSION, 5);
        assert_eq!(output.rows.len(), 4);
        assert_eq!(output.unresolved.len(), 2);
        assert_eq!(output.rows[0].validation_status, "valid");
        assert_eq!(output.rows[1].validation_status, "valid");
        assert_eq!(output.rows[2].unresolved_reason, "invalid_half_open_range");
        assert_eq!(output.rows[3].unresolved_reason, "out_of_bounds");
    }

    #[test]
    fn reports_wrong_artifact_and_stale_metadata() {
        let mut wrong_artifact = draft("wrong-artifact", 0, 1);
        wrong_artifact.artifact_id = "other".to_string();
        let mut stale_content = draft("stale-content", 0, 1);
        stale_content.source_content_hash = "old".to_string();
        let mut stale_parser = draft("stale-parser", 0, 1);
        stale_parser.block_parser_version = "report_blocks_old".to_string();
        let output = validate_source_spans(
            vec![wrong_artifact, stale_content, stale_parser],
            "artifact",
            "content",
            BLOCK_PARSER_VERSION,
            1,
        );
        let reasons: Vec<_> = output
            .unresolved
            .iter()
            .map(|row| row.unresolved_reason.as_str())
            .collect();
        assert_eq!(
            reasons,
            vec![
                "wrong_artifact",
                "stale_source_content",
                "stale_block_parser"
            ]
        );
    }
}
