use crate::provenance::{Provenance, provenance_columns, provenance_fields};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use identity::parquet_io::{read_all_rows, utf8_field, write_parquet};
use identity::resolver::{Resolution, UnresolvedReason};
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

pub const SESSION_ID: &str = "56";

#[derive(Debug, Clone, Default)]
pub struct UnresolvedRow {
    pub raw_name: String,
    pub typo_corrected: String,
    pub norm_primary: String,
    pub norm_reordered: String,
    pub reason: String,
    pub source_bucket: String,
    pub role: String,
    pub context_id: String,
    pub context_label: String,
    pub raw_field: String,
    pub source_url: String,
    pub cache_path: String,
    pub source_artifact_id: String,
    pub source_content_hash: String,
    pub block_parser_version: String,
    pub extractor_version: String,
    pub confidence: f64,
}

impl UnresolvedRow {
    /// Attach transform-time provenance. Prefer this over `..Default` when url/cache are known.
    pub fn with_provenance(mut self, p: Provenance) -> Self {
        self.source_url = p.source_url;
        self.cache_path = p.cache_path;
        self.source_artifact_id = p.source_artifact_id;
        self.source_content_hash = p.source_content_hash;
        self.block_parser_version = p.block_parser_version;
        self.extractor_version = p.extractor_version;
        self.confidence = p.confidence;
        self
    }

    fn to_provenance(&self) -> Provenance {
        Provenance {
            source_url: self.source_url.clone(),
            cache_path: self.cache_path.clone(),
            source_artifact_id: self.source_artifact_id.clone(),
            source_content_hash: self.source_content_hash.clone(),
            block_parser_version: self.block_parser_version.clone(),
            extractor_version: self.extractor_version.clone(),
            confidence: self.confidence,
        }
    }
}

pub fn split_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn reason_label(reason: &UnresolvedReason) -> &'static str {
    match reason {
        UnresolvedReason::Empty => "empty",
        UnresolvedReason::NotInIndex => "not_in_index",
        UnresolvedReason::Ambiguous => "ambiguous",
    }
}

pub fn is_resolved(resolution: &Resolution) -> bool {
    matches!(resolution, Resolution::Resolved(_))
}

pub fn person_id_from(resolution: &Resolution) -> Option<String> {
    match resolution {
        Resolution::Resolved(id) => Some(id.clone()),
        Resolution::Unresolved(_) => None,
    }
}

pub fn write_unresolved_persons(path: &Path, rows: &[UnresolvedRow]) -> Result<(), Box<dyn Error>> {
    let mut fields = vec![
        utf8_field("raw_name", false),
        utf8_field("typo_corrected", false),
        utf8_field("norm_primary", false),
        utf8_field("norm_reordered", false),
        utf8_field("reason", false),
        utf8_field("source_bucket", false),
        utf8_field("role", false),
        utf8_field("context_id", false),
        utf8_field("context_label", false),
        utf8_field("raw_field", false),
    ];
    fields.extend(provenance_fields());
    let schema = Schema::new(fields);

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    let mut columns = vec![
        col!(|r| r.raw_name.clone()),
        col!(|r| r.typo_corrected.clone()),
        col!(|r| r.norm_primary.clone()),
        col!(|r| r.norm_reordered.clone()),
        col!(|r| r.reason.clone()),
        col!(|r| r.source_bucket.clone()),
        col!(|r| r.role.clone()),
        col!(|r| r.context_id.clone()),
        col!(|r| r.context_label.clone()),
        col!(|r| r.raw_field.clone()),
    ];
    columns.extend(provenance_columns(
        rows.iter().map(UnresolvedRow::to_provenance),
    ));

    write_parquet(path, schema, columns)
}

pub fn dedupe_unresolved(rows: &mut Vec<UnresolvedRow>) {
    let mut seen: HashSet<(String, String, String, String)> = HashSet::new();
    rows.retain(|row| {
        seen.insert((
            row.raw_name.clone(),
            row.source_bucket.clone(),
            row.context_id.clone(),
            row.role.clone(),
        ))
    });
    rows.sort_by(|a, b| {
        a.source_bucket
            .cmp(&b.source_bucket)
            .then(a.raw_name.cmp(&b.raw_name))
            .then(a.context_id.cmp(&b.context_id))
    });
}

/// Verify staging schemas match STAGING.md (regeneration check for Step 0).
pub fn verify_staging(data_dir: &Path) -> Result<(), Box<dyn Error>> {
    let commission_questions = data_dir.join(format!(
        "sessions/{SESSION_ID}/commission/questions.parquet"
    ));
    let batch = read_all_rows(&commission_questions)?
        .into_iter()
        .next()
        .ok_or("commission questions parquet is empty")?;
    let cols: HashSet<String> = batch
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();

    if cols.contains("dossier_ids") {
        return Err("commission questions still has dossier_ids column; regenerate staging".into());
    }
    if cols.contains("discussion") {
        return Err("questions still has discussion column; regenerate staging".into());
    }
    if !cols.contains("internal_ids") {
        return Err("commission questions missing internal_ids column".into());
    }

    for (_kind, rel) in [
        (
            "plenary",
            format!("sessions/{SESSION_ID}/plenary/utterances.parquet"),
        ),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/utterances.parquet"),
        ),
    ] {
        let path = data_dir.join(rel);
        if !path.exists() {
            return Err(format!("missing staging utterances parquet: {}", path.display()).into());
        }
    }

    println!("[normalize] staging check: questions without discussion; utterances.parquet present");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_csv_trims_and_skips_empty() {
        assert_eq!(
            split_csv("Jan Jambon,  ,Peter De Roover"),
            vec!["Jan Jambon".to_string(), "Peter De Roover".to_string()]
        );
    }
}
