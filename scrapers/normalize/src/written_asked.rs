use crate::common::{dedupe_unresolved, reason_label, UnresolvedRow, SESSION_ID};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::{Bucket, Resolution, Resolver};
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct WrittenAskedRow {
    pub asked_id: String,
    pub person_id: String,
    pub question_id: String,
    pub session_id: String,
    pub raw_name: String,
    pub source_url: String,
    pub cache_path: String,
    pub confidence: String,
}

pub struct WrittenAskedOutput {
    pub rows: Vec<WrittenAskedRow>,
    pub unresolved: Vec<UnresolvedRow>,
}

pub fn normalize_written_asked(
    data_dir: &Path,
    resolver: &Resolver,
    canonical_map: &std::collections::HashMap<String, String>,
) -> Result<WrittenAskedOutput, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/written/questions.parquet"));
    if !path.exists() {
        return Ok(WrittenAskedOutput {
            rows: Vec::new(),
            unresolved: Vec::new(),
        });
    }

    let mut rows = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for batch in read_all_rows(&path)? {
        let question_ids = read_string_column(&batch, "question_id")?;
        let author_actr = read_string_column(&batch, "author_actr_id")?;
        let author_raw = read_string_column(&batch, "author_raw")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            let question_id = canonical_map
                .get(&question_ids[i])
                .cloned()
                .unwrap_or_else(|| question_ids[i].clone());
            let resolution = if !author_actr[i].is_empty() {
                resolver.resolve_by_actr_id(&author_actr[i])
            } else {
                resolver.resolve_person(&author_raw[i], Bucket::Questioner)
            };

            match resolution {
                Resolution::Resolved(person_id) => {
                    let key = (person_id.clone(), question_id.clone());
                    if seen.insert(key) {
                        rows.push(WrittenAskedRow {
                            asked_id: format!("{question_id}_{person_id}"),
                            person_id,
                            question_id,
                            session_id: SESSION_ID.to_string(),
                            raw_name: author_raw[i].clone(),
                            source_url: source_urls[i].clone(),
                            cache_path: cache_paths[i].clone(),
                            confidence: if author_actr[i].is_empty() {
                                "parsed".to_string()
                            } else {
                                "exact".to_string()
                            },
                        });
                    }
                }
                Resolution::Unresolved(reason) => {
                    let detail = resolver.resolve_detail(&author_raw[i], Bucket::Questioner);
                    unresolved.push(UnresolvedRow {
                        raw_name: detail.raw_name,
                        typo_corrected: detail.typo_corrected,
                        norm_primary: detail.norm_primary,
                        norm_reordered: detail.norm_reordered,
                        reason: reason_label(&reason).to_string(),
                        source_bucket: "written_author".to_string(),
                        role: "questioner".to_string(),
                        context_id: question_id.clone(),
                        context_label: format!("written question {question_id}"),
                        raw_field: author_raw[i].clone(),
                        source_url: source_urls[i].clone(),
                        cache_path: cache_paths[i].clone(),
                    });
                }
            }
        }
    }

    rows.sort_by(|a, b| a.question_id.cmp(&b.question_id).then(a.person_id.cmp(&b.person_id)));
    dedupe_unresolved(&mut unresolved);
    Ok(WrittenAskedOutput { rows, unresolved })
}

pub fn write_written_asked(path: &Path, rows: &[WrittenAskedRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("asked_id", false),
        utf8_field("person_id", false),
        utf8_field("question_id", false),
        utf8_field("session_id", false),
        utf8_field("raw_name", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
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
            col!(|r| r.asked_id.clone()),
            col!(|r| r.person_id.clone()),
            col!(|r| r.question_id.clone()),
            col!(|r| r.session_id.clone()),
            col!(|r| r.raw_name.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}
