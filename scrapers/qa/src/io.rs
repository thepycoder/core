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
        let examples = if batch.schema().fields().iter().any(|f| f.name() == "examples") {
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
