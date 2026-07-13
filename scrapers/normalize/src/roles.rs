use crate::common::{SESSION_ID, UnresolvedRow, dedupe_unresolved, reason_label, split_csv};
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
    pub confidence: String,
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

    for batch in read_all_rows(&path)? {
        let session_ids = read_string_column(&batch, "session_id")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let chairs = read_string_column(&batch, "chair")?;
        let commissions = read_string_column(&batch, "commission")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            let target_id = format!("commission_{}_{}", session_ids[i], meeting_ids[i]);
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
                            source_bucket: "commission_members".to_string(),
                            role: "chair".to_string(),
                            context_id: target_id.clone(),
                            context_label: commissions[i].clone(),
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

    rows.sort_by(|a, b| {
        a.target_id
            .cmp(&b.target_id)
            .then(a.person_id.cmp(&b.person_id))
    });
    dedupe_unresolved(&mut unresolved);

    Ok(HoldsRoleOutput { rows, unresolved })
}

pub fn write_holds_role(path: &Path, rows: &[HoldsRoleRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("holds_role_id", false),
        utf8_field("person_id", false),
        utf8_field("role", false),
        utf8_field("target_type", false),
        utf8_field("target_id", false),
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
            col!(|r| r.holds_role_id.clone()),
            col!(|r| r.person_id.clone()),
            col!(|r| r.role.clone()),
            col!(|r| r.target_type.clone()),
            col!(|r| r.target_id.clone()),
            col!(|r| r.session_id.clone()),
            col!(|r| r.raw_name.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}
