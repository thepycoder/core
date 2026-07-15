use crate::common::{SESSION_ID, UnresolvedRow, dedupe_unresolved, reason_label};
use crate::provenance::{
    CONFIDENCE_EXACT, CONFIDENCE_PARSED, ContentHashCache, normalize_extractor_version,
    provenance_columns, provenance_fields, provenance_of,
};
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
    pub source_artifact_id: String,
    pub source_content_hash: String,
    pub block_parser_version: String,
    pub extractor_version: String,
    pub confidence: f64,
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
    let mut hashes = ContentHashCache::new();
    let extractor = normalize_extractor_version("written_asked");

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
            let confidence = if author_actr[i].is_empty() {
                CONFIDENCE_PARSED
            } else {
                CONFIDENCE_EXACT
            };
            let prov = hashes.staging(&source_urls[i], &cache_paths[i], &extractor, confidence);
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
                            source_url: prov.source_url.clone(),
                            cache_path: prov.cache_path.clone(),
                            source_artifact_id: prov.source_artifact_id.clone(),
                            source_content_hash: prov.source_content_hash.clone(),
                            block_parser_version: prov.block_parser_version.clone(),
                            extractor_version: prov.extractor_version.clone(),
                            confidence: prov.confidence,
                        });
                    }
                }
                Resolution::Unresolved(reason) => {
                    let detail = resolver.resolve_detail(&author_raw[i], Bucket::Questioner);
                    unresolved.push(
                        UnresolvedRow {
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
                            ..UnresolvedRow::default()
                        }
                        .with_provenance(prov),
                    );
                }
            }
        }
    }

    rows.sort_by(|a, b| {
        a.question_id
            .cmp(&b.question_id)
            .then(a.person_id.cmp(&b.person_id))
    });
    dedupe_unresolved(&mut unresolved);
    Ok(WrittenAskedOutput { rows, unresolved })
}

pub fn write_written_asked(path: &Path, rows: &[WrittenAskedRow]) -> Result<(), Box<dyn Error>> {
    let mut fields = vec![
        utf8_field("asked_id", false),
        utf8_field("person_id", false),
        utf8_field("question_id", false),
        utf8_field("session_id", false),
        utf8_field("raw_name", false),
    ];
    fields.extend(provenance_fields());
    let schema = Schema::new(fields);
    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }
    let mut columns = vec![
        col!(|r| r.asked_id.clone()),
        col!(|r| r.person_id.clone()),
        col!(|r| r.question_id.clone()),
        col!(|r| r.session_id.clone()),
        col!(|r| r.raw_name.clone()),
    ];
    columns.extend(provenance_columns(rows.iter().map(|r| {
        provenance_of(
            &r.source_url,
            &r.cache_path,
            &r.source_artifact_id,
            &r.source_content_hash,
            &r.block_parser_version,
            &r.extractor_version,
            r.confidence,
        )
    })));
    write_parquet(path, schema, columns)
}
