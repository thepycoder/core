use crate::common::{SESSION_ID, UnresolvedRow, dedupe_unresolved, reason_label, split_csv};
use crate::provenance::{
    CONFIDENCE_EXACT, ContentHashCache, provenance_columns, provenance_fields, provenance_of,
};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use crawl::utils::ensure_question_id;
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::{Bucket, Resolution, Resolver};
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AskedRow {
    pub asked_id: String,
    pub person_id: String,
    pub question_id: String,
    pub session_id: String,
    pub meeting_id: String,
    pub meeting_kind: String,
    pub raw_name: String,
    pub source_url: String,
    pub cache_path: String,
    pub source_artifact_id: String,
    pub source_content_hash: String,
    pub block_parser_version: String,
    pub extractor_version: String,
    pub confidence: f64,
}

pub struct AskedOutput {
    pub asked: Vec<AskedRow>,
    pub unresolved: Vec<UnresolvedRow>,
}

pub fn normalize_asked(
    data_dir: &Path,
    resolver: &Resolver,
) -> Result<AskedOutput, Box<dyn Error>> {
    let mut asked = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut hashes = ContentHashCache::new();

    for (meeting_kind, rel_path) in [
        (
            "plenary",
            format!("sessions/{SESSION_ID}/plenary/questions.parquet"),
        ),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/questions.parquet"),
        ),
    ] {
        let path = data_dir.join(rel_path);
        for batch in read_all_rows(&path)? {
            let question_ids = read_string_column(&batch, "question_id")?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let questioners = read_string_column(&batch, "questioners")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                let question_id =
                    ensure_question_id(&session_ids[i], meeting_kind, &question_ids[i]);
                let prov =
                    hashes.meeting_report(&source_urls[i], &cache_paths[i], CONFIDENCE_EXACT);
                for name in split_csv(&questioners[i]) {
                    let detail = resolver.resolve_detail(&name, Bucket::Questioner);
                    match detail.resolution {
                        Resolution::Resolved(person_id) => {
                            let key = (person_id.clone(), question_id.clone());
                            if seen.insert(key) {
                                asked.push(AskedRow {
                                    asked_id: format!("{question_id}_{person_id}"),
                                    person_id,
                                    question_id: question_id.clone(),
                                    session_id: session_ids[i].clone(),
                                    meeting_id: meeting_ids[i].clone(),
                                    meeting_kind: meeting_kind.to_string(),
                                    raw_name: name.clone(),
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
                            unresolved.push(
                                UnresolvedRow {
                                    raw_name: detail.raw_name,
                                    typo_corrected: detail.typo_corrected,
                                    norm_primary: detail.norm_primary,
                                    norm_reordered: detail.norm_reordered,
                                    reason: reason_label(&reason).to_string(),
                                    source_bucket: "questioners".to_string(),
                                    role: "questioner".to_string(),
                                    context_id: question_id.clone(),
                                    context_label: format!("question {question_id}"),
                                    raw_field: name,
                                    ..UnresolvedRow::default()
                                }
                                .with_provenance(prov.clone()),
                            );
                        }
                    }
                }
            }
        }
    }

    asked.sort_by(|a, b| {
        a.question_id
            .cmp(&b.question_id)
            .then(a.person_id.cmp(&b.person_id))
    });
    dedupe_unresolved(&mut unresolved);
    Ok(AskedOutput { asked, unresolved })
}

pub fn write_asked(path: &Path, rows: &[AskedRow]) -> Result<(), Box<dyn Error>> {
    let mut fields = vec![
        utf8_field("asked_id", false),
        utf8_field("person_id", false),
        utf8_field("question_id", false),
        utf8_field("session_id", false),
        utf8_field("meeting_id", false),
        utf8_field("meeting_kind", false),
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
        col!(|r| r.meeting_id.clone()),
        col!(|r| r.meeting_kind.clone()),
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
