use crate::common::{
    dedupe_unresolved, reason_label, split_csv, UnresolvedRow, SESSION_ID,
};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::{Bucket, Resolution, Resolver, UnresolvedReason};
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
    pub confidence: String,
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

    for (meeting_kind, rel_path) in [
        ("plenary", format!("sessions/{SESSION_ID}/plenary/questions.parquet")),
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
            let respondents = read_string_column(&batch, "respondents")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                for name in split_csv(&questioners[i]) {
                    let detail = resolver.resolve_detail(&name, Bucket::Questioner);
                    match detail.resolution {
                        Resolution::Resolved(person_id) => {
                            let key = (person_id.clone(), question_ids[i].clone());
                            if seen.insert(key) {
                                asked.push(AskedRow {
                                    asked_id: format!("{}_{person_id}", question_ids[i]),
                                    person_id,
                                    question_id: question_ids[i].clone(),
                                    session_id: session_ids[i].clone(),
                                    meeting_id: meeting_ids[i].clone(),
                                    meeting_kind: meeting_kind.to_string(),
                                    raw_name: name.clone(),
                                    source_url: source_urls[i].clone(),
                                    cache_path: cache_paths[i].clone(),
                                    confidence: "exact".to_string(),
                                });
                            }
                        }
                        Resolution::Unresolved(reason) => {
                            unresolved.push(UnresolvedRow {
                                raw_name: detail.raw_name,
                                typo_corrected: detail.typo_corrected,
                                norm_primary: detail.norm_primary,
                                norm_reordered: detail.norm_reordered,
                                reason: reason_label(&reason).to_string(),
                                source_bucket: "questioners".to_string(),
                                role: "questioner".to_string(),
                                context_id: question_ids[i].clone(),
                                context_label: format!("question {}", question_ids[i]),
                                raw_field: name,
                                source_url: source_urls[i].clone(),
                                cache_path: cache_paths[i].clone(),
                            });
                        }
                    }
                }

                for title in split_csv(&respondents[i]) {
                    let detail = resolver.resolve_detail(&title, Bucket::Respondent);
                    if matches!(detail.resolution, Resolution::Unresolved(_)) {
                        unresolved.push(UnresolvedRow {
                            raw_name: detail.raw_name,
                            typo_corrected: detail.typo_corrected,
                            norm_primary: detail.norm_primary,
                            norm_reordered: detail.norm_reordered,
                            reason: reason_label(
                                detail
                                    .reason
                                    .as_ref()
                                    .unwrap_or(&UnresolvedReason::NotInIndex),
                            )
                            .to_string(),
                            source_bucket: "respondents".to_string(),
                            role: "respondent_title".to_string(),
                            context_id: question_ids[i].clone(),
                            context_label: format!("question {}", question_ids[i]),
                            raw_field: title,
                            source_url: source_urls[i].clone(),
                            cache_path: cache_paths[i].clone(),
                        });
                    }
                }
            }
        }
    }

    asked.sort_by(|a, b| a.question_id.cmp(&b.question_id).then(a.person_id.cmp(&b.person_id)));
    dedupe_unresolved(&mut unresolved);

    Ok(AskedOutput { asked, unresolved })
}

pub fn write_asked(path: &Path, rows: &[AskedRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("asked_id", false),
        utf8_field("person_id", false),
        utf8_field("question_id", false),
        utf8_field("session_id", false),
        utf8_field("meeting_id", false),
        utf8_field("meeting_kind", false),
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
            col!(|r| r.meeting_id.clone()),
            col!(|r| r.meeting_kind.clone()),
            col!(|r| r.raw_name.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}
