use crate::common::{SESSION_ID, UnresolvedRow, dedupe_unresolved, reason_label, split_csv};
use crate::provenance::{
    CONFIDENCE_EXACT, ContentHashCache, provenance_columns, provenance_fields, provenance_of,
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
pub struct HoldsRoleRow {
    pub holds_role_id: String,
    pub person_id: String,
    pub role: String,
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

pub struct HoldsRoleOutput {
    pub rows: Vec<HoldsRoleRow>,
    pub unresolved: Vec<UnresolvedRow>,
}

pub fn normalize_holds_role(
    data_dir: &Path,
    resolver: &Resolver,
) -> Result<HoldsRoleOutput, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/commission/meetings.parquet"));
    let mut rows = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen: HashSet<(String, String, String)> = HashSet::new();
    let mut hashes = ContentHashCache::new();

    for batch in read_all_rows(&path)? {
        let session_ids = read_string_column(&batch, "session_id")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let chairs = read_string_column(&batch, "chair")?;
        let commissions = read_string_column(&batch, "commission")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            let target_id = format!("commission_{}_{}", session_ids[i], meeting_ids[i]);
            let prov = hashes.meeting_report(&source_urls[i], &cache_paths[i], CONFIDENCE_EXACT);
            for name in split_csv(&chairs[i]) {
                if name.trim().eq_ignore_ascii_case("N .") {
                    continue;
                }
                let detail = resolver.resolve_detail(&name, Bucket::CommissionMember);
                match detail.resolution {
                    Resolution::Resolved(person_id) => {
                        let key = (person_id.clone(), "chair".to_string(), target_id.clone());
                        if seen.insert(key) {
                            rows.push(HoldsRoleRow {
                                holds_role_id: format!("{person_id}_chair_{target_id}"),
                                person_id,
                                role: "chair".to_string(),
                                target_type: "meeting".to_string(),
                                target_id: target_id.clone(),
                                session_id: session_ids[i].clone(),
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
                                source_bucket: "commission_members".to_string(),
                                role: "chair".to_string(),
                                context_id: target_id.clone(),
                                context_label: commissions[i].clone(),
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

    rows.sort_by(|a, b| {
        a.target_id
            .cmp(&b.target_id)
            .then(a.person_id.cmp(&b.person_id))
    });
    dedupe_unresolved(&mut unresolved);

    Ok(HoldsRoleOutput { rows, unresolved })
}

pub fn write_holds_role(path: &Path, rows: &[HoldsRoleRow]) -> Result<(), Box<dyn Error>> {
    let mut fields = vec![
        utf8_field("holds_role_id", false),
        utf8_field("person_id", false),
        utf8_field("role", false),
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
        col!(|r| r.holds_role_id.clone()),
        col!(|r| r.person_id.clone()),
        col!(|r| r.role.clone()),
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
