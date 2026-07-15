use crate::common::{SESSION_ID, UnresolvedRow, dedupe_unresolved, reason_label, split_csv};
use crate::provenance::{
    CONFIDENCE_EXACT, ContentHashCache, provenance_columns, provenance_fields, provenance_of,
};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use crawl::utils::ensure_question_id;
use identity::actor_resolver::{ActorResolution, ActorResolver};
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::{Bucket, Resolution, Resolver};
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct InterpellatedRow {
    pub interpellated_id: String,
    pub person_id: String,
    pub interpellation_id: String,
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

#[derive(Debug, Clone)]
pub struct InterpellationRespondedRow {
    pub responded_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub interpellation_id: String,
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

pub struct InterpellationOutput {
    pub interpellated: Vec<InterpellatedRow>,
    pub responded: Vec<InterpellationRespondedRow>,
    pub unresolved: Vec<UnresolvedRow>,
}

pub fn normalize_interpellations(
    data_dir: &Path,
    resolver: &Resolver,
    actor_resolver: &ActorResolver,
) -> Result<InterpellationOutput, Box<dyn Error>> {
    let mut interpellated = Vec::new();
    let mut responded = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen_interpellated: HashSet<(String, String)> = HashSet::new();
    let mut seen_responded: HashSet<(String, String, String)> = HashSet::new();
    let mut hashes = ContentHashCache::new();

    for (meeting_kind, rel_path) in [
        (
            "plenary",
            format!("sessions/{SESSION_ID}/plenary/interpellations.parquet"),
        ),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/interpellations.parquet"),
        ),
    ] {
        let path = data_dir.join(rel_path);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let interpellation_ids = read_string_column(&batch, "interpellation_id")?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let interpellators = read_string_column(&batch, "interpellators")?;
            let respondents = read_string_column(&batch, "respondents")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                let interpellation_id =
                    ensure_question_id(&session_ids[i], meeting_kind, &interpellation_ids[i]);
                let prov =
                    hashes.meeting_report(&source_urls[i], &cache_paths[i], CONFIDENCE_EXACT);

                for name in split_csv(&interpellators[i]) {
                    let detail = resolver.resolve_detail(&name, Bucket::Questioner);
                    match detail.resolution {
                        Resolution::Resolved(person_id) => {
                            let key = (person_id.clone(), interpellation_id.clone());
                            if seen_interpellated.insert(key) {
                                interpellated.push(InterpellatedRow {
                                    interpellated_id: format!("{interpellation_id}_{person_id}"),
                                    person_id,
                                    interpellation_id: interpellation_id.clone(),
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
                                    source_bucket: "interpellators".to_string(),
                                    role: "interpellator".to_string(),
                                    context_id: interpellation_id.clone(),
                                    context_label: format!("interpellation {interpellation_id}"),
                                    raw_field: name,
                                    ..UnresolvedRow::default()
                                }
                                .with_provenance(prov.clone()),
                            );
                        }
                    }
                }

                for name in split_csv(&respondents[i]) {
                    let detail = actor_resolver.resolve_actor_detail(&name, Bucket::Respondent);
                    match detail.resolution {
                        ActorResolution::Person(entity_id) => {
                            let key = (
                                "Person".to_string(),
                                entity_id.clone(),
                                interpellation_id.clone(),
                            );
                            if seen_responded.insert(key) {
                                responded.push(InterpellationRespondedRow {
                                    responded_id: format!("{interpellation_id}_{entity_id}"),
                                    entity_type: "Person".to_string(),
                                    entity_id,
                                    interpellation_id: interpellation_id.clone(),
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
                        ActorResolution::ExternalPerson(entity_id) => {
                            let key = (
                                "ExternalPerson".to_string(),
                                entity_id.clone(),
                                interpellation_id.clone(),
                            );
                            if seen_responded.insert(key) {
                                responded.push(InterpellationRespondedRow {
                                    responded_id: format!("{interpellation_id}_{entity_id}"),
                                    entity_type: "ExternalPerson".to_string(),
                                    entity_id,
                                    interpellation_id: interpellation_id.clone(),
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
                        ActorResolution::Unresolved(reason) => {
                            unresolved.push(
                                UnresolvedRow {
                                    raw_name: detail.raw_name,
                                    typo_corrected: detail.typo_corrected,
                                    norm_primary: detail.norm_primary,
                                    norm_reordered: detail.norm_reordered,
                                    reason: reason_label(&reason).to_string(),
                                    source_bucket: "respondents".to_string(),
                                    role: "respondent".to_string(),
                                    context_id: interpellation_id.clone(),
                                    context_label: format!("interpellation {interpellation_id}"),
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

    interpellated.sort_by(|a, b| {
        a.interpellation_id
            .cmp(&b.interpellation_id)
            .then(a.person_id.cmp(&b.person_id))
    });
    responded.sort_by(|a, b| {
        a.interpellation_id
            .cmp(&b.interpellation_id)
            .then(a.entity_id.cmp(&b.entity_id))
    });
    dedupe_unresolved(&mut unresolved);
    Ok(InterpellationOutput {
        interpellated,
        responded,
        unresolved,
    })
}

pub fn write_interpellated(path: &Path, rows: &[InterpellatedRow]) -> Result<(), Box<dyn Error>> {
    let mut fields = vec![
        utf8_field("interpellated_id", false),
        utf8_field("person_id", false),
        utf8_field("interpellation_id", false),
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
        col!(|r| r.interpellated_id.clone()),
        col!(|r| r.person_id.clone()),
        col!(|r| r.interpellation_id.clone()),
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

pub fn write_interpellation_responded(
    path: &Path,
    rows: &[InterpellationRespondedRow],
) -> Result<(), Box<dyn Error>> {
    let mut fields = vec![
        utf8_field("responded_id", false),
        utf8_field("entity_type", false),
        utf8_field("entity_id", false),
        utf8_field("interpellation_id", false),
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
        col!(|r| r.responded_id.clone()),
        col!(|r| r.entity_type.clone()),
        col!(|r| r.entity_id.clone()),
        col!(|r| r.interpellation_id.clone()),
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
