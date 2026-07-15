use crate::types::{AliasCandidate, CheckDetail, CheckSummary};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

pub fn write_check_details(path: &Path, rows: &[CheckDetail]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("check_id", false),
        utf8_field("severity", false),
        utf8_field("status", false),
        utf8_field("session_id", false),
        utf8_field("meeting_kind", false),
        utf8_field("meeting_id", false),
        utf8_field("entity_type", false),
        utf8_field("entity_id", false),
        utf8_field("expected", false),
        utf8_field("actual", false),
        utf8_field("message", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
        utf8_field("source_block", false),
        utf8_field("created_at", false),
        utf8_field("warning_id", false),
        utf8_field("warning_kind", false),
        utf8_field("graph_node_type", false),
        utf8_field("graph_node_id", false),
        utf8_field("source_artifact_id", false),
    ]);

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    write_parquet(
        path,
        schema,
        vec![
            col!(|r| r.check_id.clone()),
            col!(|r| r.severity.clone()),
            col!(|r| r.status.clone()),
            col!(|r| r.session_id.clone()),
            col!(|r| r.meeting_kind.clone()),
            col!(|r| r.meeting_id.clone()),
            col!(|r| r.entity_type.clone()),
            col!(|r| r.entity_id.clone()),
            col!(|r| r.expected.clone()),
            col!(|r| r.actual.clone()),
            col!(|r| r.message.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.source_block.clone()),
            col!(|r| r.created_at.clone()),
            col!(|r| r.warning_id.clone()),
            col!(|r| r.warning_kind.clone()),
            col!(|r| r.graph_node_type.clone()),
            col!(|r| r.graph_node_id.clone()),
            col!(|r| r.source_artifact_id.clone()),
        ],
    )
}

pub fn write_check_summaries(path: &Path, rows: &[CheckSummary]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("table", false),
        utf8_field("check", false),
        utf8_field("status", false),
        utf8_field("count", false),
        utf8_field("detail", false),
        utf8_field("examples", false),
    ]);

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    write_parquet(
        path,
        schema,
        vec![
            col!(|r| r.table.clone()),
            col!(|r| r.check.clone()),
            col!(|r| r.status.clone()),
            col!(|r| r.count.to_string()),
            col!(|r| r.detail.clone()),
            col!(|r| r.examples.clone()),
        ],
    )
}

pub fn write_alias_candidates(path: &Path, rows: &[AliasCandidate]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("raw_name", false),
        utf8_field("cleaned_name", false),
        utf8_field("matched_person_id", false),
        utf8_field("source_bucket", false),
        utf8_field("context_id", false),
        utf8_field("check_id", false),
        utf8_field("confidence", false),
    ]);

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    write_parquet(
        path,
        schema,
        vec![
            col!(|r| r.raw_name.clone()),
            col!(|r| r.cleaned_name.clone()),
            col!(|r| r.matched_person_id.clone()),
            col!(|r| r.source_bucket.clone()),
            col!(|r| r.context_id.clone()),
            col!(|r| r.check_id.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}

pub fn read_check_summaries(path: &Path) -> Result<Vec<CheckSummary>, Box<dyn Error>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for batch in read_all_rows(path)? {
        let tables = read_string_column(&batch, "table")?;
        let checks = read_string_column(&batch, "check")?;
        let statuses = read_string_column(&batch, "status")?;
        let counts = read_string_column(&batch, "count")?;
        let details = read_string_column(&batch, "detail")?;
        let examples = if batch.schema().index_of("examples").is_ok() {
            read_string_column(&batch, "examples")?
        } else {
            vec![String::new(); batch.num_rows()]
        };
        for i in 0..batch.num_rows() {
            rows.push(CheckSummary {
                table: tables[i].clone(),
                check: checks[i].clone(),
                status: statuses[i].clone(),
                count: counts[i].parse().unwrap_or(0),
                detail: details[i].clone(),
                examples: examples[i].clone(),
            });
        }
    }
    Ok(rows)
}

pub fn parquet_row_count(path: &Path) -> Result<usize, Box<dyn Error>> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0usize;
    for batch in read_all_rows(path)? {
        total += batch.num_rows();
    }
    Ok(total)
}

pub fn write_coverage_baselines(
    path: &Path,
    rows: &[crate::types::CoverageBaselineRow],
) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("meeting_kind", false),
        utf8_field("meeting_id", false),
        utf8_field("source_words", false),
        utf8_field("saved_words", false),
        utf8_field("ratio", false),
        utf8_field("updated_at", false),
    ]);
    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }
    write_parquet(
        path,
        schema,
        vec![
            col!(|r| r.meeting_kind.clone()),
            col!(|r| r.meeting_id.clone()),
            col!(|r| r.source_words.to_string()),
            col!(|r| r.saved_words.to_string()),
            col!(|r| format!("{:.6}", r.ratio)),
            col!(|r| r.updated_at.clone()),
        ],
    )
}

pub fn read_coverage_baselines(
    path: &Path,
) -> Result<Vec<crate::types::CoverageBaselineRow>, Box<dyn Error>> {
    use crate::types::CoverageBaselineRow;

    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for batch in read_all_rows(path)? {
        let kinds = read_string_column(&batch, "meeting_kind")?;
        let ids = read_string_column(&batch, "meeting_id")?;
        let has_words = batch.schema().index_of("source_words").is_ok();
        let source = read_string_column(
            &batch,
            if has_words {
                "source_words"
            } else {
                "source_chars"
            },
        )?;
        let saved = read_string_column(
            &batch,
            if has_words {
                "saved_words"
            } else {
                "saved_chars"
            },
        )?;
        let ratios = read_string_column(&batch, "ratio")?;
        let updated = read_string_column(&batch, "updated_at")?;
        for i in 0..batch.num_rows() {
            rows.push(CoverageBaselineRow {
                meeting_kind: kinds[i].clone(),
                meeting_id: ids[i].clone(),
                source_words: source[i].parse().unwrap_or(0),
                saved_words: saved[i].parse().unwrap_or(0),
                ratio: ratios[i].parse().unwrap_or(0.0),
                updated_at: updated[i].clone(),
            });
        }
    }
    Ok(rows)
}
