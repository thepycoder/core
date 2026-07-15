use crate::common::{SESSION_ID, UnresolvedRow, dedupe_unresolved, reason_label, split_csv};
use crate::provenance::{
    CONFIDENCE_EXACT, ContentHashCache, normalize_extractor_version, provenance_columns,
    provenance_fields, provenance_of,
};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use identity::actor_resolver::{ActorResolution, ActorResolver};
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::Bucket;
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AuthoredRow {
    pub authored_id: String,
    pub person_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub target_type: String,
    pub target_id: String,
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

pub struct AuthoredOutput {
    pub rows: Vec<AuthoredRow>,
    pub unresolved: Vec<UnresolvedRow>,
}

fn is_government_author(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    lower == "government" || lower.contains("gouvernment") || lower.contains("regering")
}

pub fn normalize_authored(
    data_dir: &Path,
    actor_resolver: &ActorResolver,
) -> Result<AuthoredOutput, Box<dyn Error>> {
    let mut rows = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen: HashSet<(String, String, String)> = HashSet::new();
    let mut hashes = ContentHashCache::new();
    let extractor = normalize_extractor_version("authored");

    let dossiers_path = data_dir.join(format!("sessions/{SESSION_ID}/dossiers.parquet"));
    for batch in read_all_rows(&dossiers_path)? {
        let session_ids = read_string_column(&batch, "session_id")?;
        let dossier_ids = read_string_column(&batch, "id")?;
        let authors = read_string_column(&batch, "authors")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            let target_id = format!("{}/{}", session_ids[i], dossier_ids[i]);
            ingest_authors(
                actor_resolver,
                &authors[i],
                "dossier",
                &target_id,
                &session_ids[i],
                &source_urls[i],
                &cache_paths[i],
                &dossier_ids[i],
                &extractor,
                &mut hashes,
                &mut rows,
                &mut unresolved,
                &mut seen,
            );
        }
    }

    let subdocs_path = data_dir.join(format!("sessions/{SESSION_ID}/subdocuments.parquet"));
    for batch in read_all_rows(&subdocs_path)? {
        let dossier_ids = read_string_column(&batch, "dossier_id")?;
        let doc_ids = read_string_column(&batch, "id")?;
        let authors = read_string_column(&batch, "authors")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            ingest_authors(
                actor_resolver,
                &authors[i],
                "document",
                &doc_ids[i],
                SESSION_ID,
                &source_urls[i],
                &cache_paths[i],
                &format!("dossier {} doc {}", dossier_ids[i], doc_ids[i]),
                &extractor,
                &mut hashes,
                &mut rows,
                &mut unresolved,
                &mut seen,
            );
        }
    }

    rows.sort_by(|a, b| {
        a.target_type
            .cmp(&b.target_type)
            .then(a.target_id.cmp(&b.target_id))
            .then(a.entity_id.cmp(&b.entity_id))
    });
    dedupe_unresolved(&mut unresolved);
    Ok(AuthoredOutput { rows, unresolved })
}

#[allow(clippy::too_many_arguments)]
fn ingest_authors(
    actor_resolver: &ActorResolver,
    authors_csv: &str,
    target_type: &str,
    target_id: &str,
    session_id: &str,
    source_url: &str,
    cache_path: &str,
    context_label: &str,
    extractor: &str,
    hashes: &mut ContentHashCache,
    rows: &mut Vec<AuthoredRow>,
    unresolved: &mut Vec<UnresolvedRow>,
    seen: &mut HashSet<(String, String, String)>,
) {
    let prov = hashes.staging(source_url, cache_path, extractor, CONFIDENCE_EXACT);
    for name in split_csv(authors_csv) {
        if is_government_author(&name) {
            continue;
        }
        let detail = actor_resolver.resolve_actor_detail(&name, Bucket::Author);
        match detail.resolution {
            ActorResolution::Person(person_id) => {
                let key = (
                    person_id.clone(),
                    target_type.to_string(),
                    target_id.to_string(),
                );
                if seen.insert(key) {
                    rows.push(AuthoredRow {
                        authored_id: format!("{person_id}_{target_type}_{target_id}"),
                        person_id: person_id.clone(),
                        entity_type: "Person".to_string(),
                        entity_id: person_id,
                        target_type: target_type.to_string(),
                        target_id: target_id.to_string(),
                        session_id: session_id.to_string(),
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
            ActorResolution::ExternalPerson(ext_id) => {
                let key = (
                    ext_id.clone(),
                    target_type.to_string(),
                    target_id.to_string(),
                );
                if seen.insert(key) {
                    rows.push(AuthoredRow {
                        authored_id: format!("{ext_id}_{target_type}_{target_id}"),
                        person_id: String::new(),
                        entity_type: "ExternalPerson".to_string(),
                        entity_id: ext_id,
                        target_type: target_type.to_string(),
                        target_id: target_id.to_string(),
                        session_id: session_id.to_string(),
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
                        source_bucket: "authors".to_string(),
                        role: "author".to_string(),
                        context_id: target_id.to_string(),
                        context_label: context_label.to_string(),
                        raw_field: name,
                        ..UnresolvedRow::default()
                    }
                    .with_provenance(prov.clone()),
                );
            }
        }
    }
}

pub fn write_authored(path: &Path, rows: &[AuthoredRow]) -> Result<(), Box<dyn Error>> {
    let mut fields = vec![
        utf8_field("authored_id", false),
        utf8_field("person_id", false),
        utf8_field("entity_type", false),
        utf8_field("entity_id", false),
        utf8_field("target_type", false),
        utf8_field("target_id", false),
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
        col!(|r| r.authored_id.clone()),
        col!(|r| r.person_id.clone()),
        col!(|r| r.entity_type.clone()),
        col!(|r| r.entity_id.clone()),
        col!(|r| r.target_type.clone()),
        col!(|r| r.target_id.clone()),
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
