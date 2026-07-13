use crate::common::{SESSION_ID, UnresolvedRow, dedupe_unresolved, reason_label, split_csv};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use crawl::utils::ensure_question_id;
use identity::actor_resolver::{ActorResolution, ActorResolver};
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::Bucket;
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct InvitedRow {
    pub invited_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub hearing_id: String,
    pub session_id: String,
    pub meeting_id: String,
    pub meeting_kind: String,
    pub raw_name: String,
    pub source_url: String,
    pub cache_path: String,
    pub confidence: String,
}

pub struct InvitedOutput {
    pub rows: Vec<InvitedRow>,
    pub unresolved: Vec<UnresolvedRow>,
}

pub fn normalize_invited(
    data_dir: &Path,
    actor_resolver: &ActorResolver,
) -> Result<InvitedOutput, Box<dyn Error>> {
    let mut rows = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen: HashSet<(String, String, String)> = HashSet::new();

    for (meeting_kind, rel_path) in [
        (
            "plenary",
            format!("sessions/{SESSION_ID}/plenary/hearings.parquet"),
        ),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/hearings.parquet"),
        ),
    ] {
        let path = data_dir.join(rel_path);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let hearing_ids = read_string_column(&batch, "hearing_id")?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let witnesses = read_string_column(&batch, "witnesses")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                let hearing_id = ensure_question_id(&session_ids[i], meeting_kind, &hearing_ids[i]);
                for name in split_csv(&witnesses[i]) {
                    if name.trim().is_empty() {
                        continue;
                    }
                    let detail = actor_resolver.resolve_actor_detail(&name, Bucket::Speaker);
                    match detail.resolution {
                        ActorResolution::Person(entity_id) => {
                            let key = ("Person".to_string(), entity_id.clone(), hearing_id.clone());
                            if seen.insert(key) {
                                rows.push(InvitedRow {
                                    invited_id: format!("{hearing_id}_{entity_id}"),
                                    entity_type: "Person".to_string(),
                                    entity_id,
                                    hearing_id: hearing_id.clone(),
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
                        ActorResolution::ExternalPerson(entity_id) => {
                            let key = (
                                "ExternalPerson".to_string(),
                                entity_id.clone(),
                                hearing_id.clone(),
                            );
                            if seen.insert(key) {
                                rows.push(InvitedRow {
                                    invited_id: format!("{hearing_id}_{entity_id}"),
                                    entity_type: "ExternalPerson".to_string(),
                                    entity_id,
                                    hearing_id: hearing_id.clone(),
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
                        ActorResolution::Unresolved(reason) => {
                            unresolved.push(UnresolvedRow {
                                raw_name: detail.raw_name,
                                typo_corrected: detail.typo_corrected,
                                norm_primary: detail.norm_primary,
                                norm_reordered: detail.norm_reordered,
                                reason: reason_label(&reason).to_string(),
                                source_bucket: "witnesses".to_string(),
                                role: "witness".to_string(),
                                context_id: hearing_id.clone(),
                                context_label: format!("hearing {hearing_id}"),
                                raw_field: name,
                                source_url: source_urls[i].clone(),
                                cache_path: cache_paths[i].clone(),
                                ..UnresolvedRow::default()
                            });
                        }
                    }
                }
            }
        }
    }

    rows.sort_by(|a, b| {
        a.hearing_id
            .cmp(&b.hearing_id)
            .then(a.entity_id.cmp(&b.entity_id))
    });
    dedupe_unresolved(&mut unresolved);
    Ok(InvitedOutput { rows, unresolved })
}

pub fn write_invited(path: &Path, rows: &[InvitedRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("invited_id", false),
        utf8_field("entity_type", false),
        utf8_field("entity_id", false),
        utf8_field("hearing_id", false),
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
            col!(|r| r.invited_id.clone()),
            col!(|r| r.entity_type.clone()),
            col!(|r| r.entity_id.clone()),
            col!(|r| r.hearing_id.clone()),
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
