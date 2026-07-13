use crate::common::SESSION_ID;
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use identity::actor_resolver::ActorResolver;
use identity::external::department_external_id;
use identity::normalize::normalize_name;
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AddressedToRow {
    pub addressed_id: String,
    pub question_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub route_id: String,
    pub deptnum: String,
    pub questnum: String,
    pub statusq: String,
    pub dept_title_nl: String,
    pub dept_title_fr: String,
    pub properties_json: String,
    pub source_url: String,
    pub cache_path: String,
    pub confidence: String,
}

pub fn normalize_addressed_to(
    data_dir: &Path,
    _actor_resolver: &ActorResolver,
    canonical_map: &std::collections::HashMap<String, String>,
) -> Result<Vec<AddressedToRow>, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/written/routes.parquet"));
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut rows = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for batch in read_all_rows(&path)? {
        let question_ids = read_string_column(&batch, "question_id")?;
        let route_ids = read_string_column(&batch, "route_id")?;
        let deptnums = read_string_column(&batch, "deptnum")?;
        let questnums = read_string_column(&batch, "questnum")?;
        let statusq = read_string_column(&batch, "statusq")?;
        let dept_nl = read_string_column(&batch, "dept_title_nl")?;
        let dept_fr = read_string_column(&batch, "dept_title_fr")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            if deptnums[i].is_empty() {
                continue;
            }
            let entity_id = department_external_id(&deptnums[i]);
            let question_id = canonical_map
                .get(&question_ids[i])
                .cloned()
                .unwrap_or_else(|| question_ids[i].clone());
            let key = (question_id.clone(), entity_id.clone());
            if !seen.insert(key) {
                continue;
            }
            let properties_json = serde_json::json!({
                "route_id": route_ids[i],
                "deptnum": deptnums[i],
                "questnum": questnums[i],
                "statusq": statusq[i],
            })
            .to_string();
            rows.push(AddressedToRow {
                addressed_id: format!("{question_id}_{entity_id}"),
                question_id,
                entity_type: "ExternalPerson".to_string(),
                entity_id,
                route_id: route_ids[i].clone(),
                deptnum: deptnums[i].clone(),
                questnum: questnums[i].clone(),
                statusq: statusq[i].clone(),
                dept_title_nl: dept_nl[i].clone(),
                dept_title_fr: dept_fr[i].clone(),
                properties_json,
                source_url: source_urls[i].clone(),
                cache_path: cache_paths[i].clone(),
                confidence: "exact".to_string(),
            });
        }
    }

    rows.sort_by(|a, b| {
        a.question_id
            .cmp(&b.question_id)
            .then(a.entity_id.cmp(&b.entity_id))
    });
    Ok(rows)
}

pub fn write_addressed_to(path: &Path, rows: &[AddressedToRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("addressed_id", false),
        utf8_field("question_id", false),
        utf8_field("entity_type", false),
        utf8_field("entity_id", false),
        utf8_field("route_id", false),
        utf8_field("deptnum", false),
        utf8_field("questnum", false),
        utf8_field("statusq", false),
        utf8_field("dept_title_nl", false),
        utf8_field("dept_title_fr", false),
        utf8_field("properties_json", false),
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
            col!(|r| r.addressed_id.clone()),
            col!(|r| r.question_id.clone()),
            col!(|r| r.entity_type.clone()),
            col!(|r| r.entity_id.clone()),
            col!(|r| r.route_id.clone()),
            col!(|r| r.deptnum.clone()),
            col!(|r| r.questnum.clone()),
            col!(|r| r.statusq.clone()),
            col!(|r| r.dept_title_nl.clone()),
            col!(|r| r.dept_title_fr.clone()),
            col!(|r| r.properties_json.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}

pub fn dept_alias_norms(title_nl: &str, title_fr: &str) -> Vec<String> {
    let mut norms = Vec::new();
    for title in [title_nl, title_fr] {
        let norm = normalize_name(title);
        if !norm.is_empty() && !norms.contains(&norm) {
            norms.push(norm);
        }
    }
    norms
}
