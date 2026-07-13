use crate::common::SESSION_ID;
use identity::parquet_io::{read_all_rows, read_string_column};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OralLinkStatus {
    Exact,
    Ambiguous,
    Unmatched,
    None,
}

#[derive(Debug, Clone)]
pub struct OralWrittenLink {
    pub written_question_id: String,
    pub canonical_question_id: String,
    pub oral_ref: String,
    pub status: String,
    pub docname: String,
}

pub fn build_oral_index(data_dir: &Path) -> Result<HashMap<String, Vec<String>>, Box<dyn Error>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for rel in [
        format!("sessions/{SESSION_ID}/plenary/questions.parquet"),
        format!("sessions/{SESSION_ID}/commission/questions.parquet"),
    ] {
        let path = data_dir.join(rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let question_ids = read_string_column(&batch, "question_id")?;
            let internal_ids = read_string_column(&batch, "internal_ids")?;
            for i in 0..batch.num_rows() {
                for part in internal_ids[i].split(',') {
                    let id = part.trim().to_uppercase();
                    if id.is_empty() {
                        continue;
                    }
                    map.entry(id).or_default().push(question_ids[i].clone());
                }
            }
        }
    }
    for ids in map.values_mut() {
        ids.sort();
        ids.dedup();
    }
    Ok(map)
}

pub fn resolve_canonical_question_id(
    written_id: &str,
    oral_refs: &str,
    oral_index: &HashMap<String, Vec<String>>,
) -> (String, OralLinkStatus, Option<String>) {
    let refs: Vec<&str> = oral_refs
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if refs.is_empty() {
        return (written_id.to_string(), OralLinkStatus::None, None);
    }
    if refs.len() == 1 {
        let key = refs[0].to_uppercase();
        if let Some(candidates) = oral_index.get(&key) {
            if candidates.len() == 1 {
                return (
                    candidates[0].clone(),
                    OralLinkStatus::Exact,
                    Some(refs[0].to_string()),
                );
            }
            return (
                written_id.to_string(),
                OralLinkStatus::Ambiguous,
                Some(refs[0].to_string()),
            );
        }
        return (
            written_id.to_string(),
            OralLinkStatus::Unmatched,
            Some(refs[0].to_string()),
        );
    }
    (written_id.to_string(), OralLinkStatus::Ambiguous, None)
}

pub fn collect_oral_written_links(data_dir: &Path) -> Result<Vec<OralWrittenLink>, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/written/questions.parquet"));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let oral_index = build_oral_index(data_dir)?;
    let mut links = Vec::new();
    for batch in read_all_rows(&path)? {
        let question_ids = read_string_column(&batch, "question_id")?;
        let docnames = read_string_column(&batch, "docname")?;
        let oral_refs = read_string_column(&batch, "oral_refs")?;
        for i in 0..batch.num_rows() {
            let (canonical, status, oral_ref) =
                resolve_canonical_question_id(&question_ids[i], &oral_refs[i], &oral_index);
            let status_str = match status {
                OralLinkStatus::Exact => "exact",
                OralLinkStatus::Ambiguous => "ambiguous",
                OralLinkStatus::Unmatched => "unmatched",
                OralLinkStatus::None => "none",
            };
            links.push(OralWrittenLink {
                written_question_id: question_ids[i].clone(),
                canonical_question_id: canonical,
                oral_ref: oral_ref.unwrap_or_default(),
                status: status_str.to_string(),
                docname: docnames[i].clone(),
            });
        }
    }
    links.sort_by(|a, b| a.written_question_id.cmp(&b.written_question_id));
    Ok(links)
}

pub fn canonical_id_map(links: &[OralWrittenLink]) -> HashMap<String, String> {
    links
        .iter()
        .map(|l| {
            (
                l.written_question_id.clone(),
                l.canonical_question_id.clone(),
            )
        })
        .collect()
}

pub fn write_oral_written_links(
    path: &Path,
    rows: &[OralWrittenLink],
) -> Result<(), Box<dyn Error>> {
    use arrow::array::{ArrayRef, StringArray};
    use arrow::datatypes::Schema;
    use identity::parquet_io::{utf8_field, write_parquet};
    use std::sync::Arc;

    let schema = Schema::new(vec![
        utf8_field("written_question_id", false),
        utf8_field("canonical_question_id", false),
        utf8_field("oral_ref", false),
        utf8_field("status", false),
        utf8_field("docname", false),
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
            col!(|r| r.written_question_id.clone()),
            col!(|r| r.canonical_question_id.clone()),
            col!(|r| r.oral_ref.clone()),
            col!(|r| r.status.clone()),
            col!(|r| r.docname.clone()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_oral_ref_uses_oral_question() {
        let mut index = HashMap::new();
        index.insert("Q56001442P".to_string(), vec!["56_plenary_1_0".to_string()]);
        let (id, status, _) = resolve_canonical_question_id("56_written_123", "Q56001442P", &index);
        assert_eq!(id, "56_plenary_1_0");
        assert_eq!(status, OralLinkStatus::Exact);
    }

    #[test]
    fn ambiguous_refs_keep_written_id() {
        let mut index = HashMap::new();
        index.insert(
            "Q56001442P".to_string(),
            vec!["a".to_string(), "b".to_string()],
        );
        let (id, status, _) = resolve_canonical_question_id("56_written_123", "Q56001442P", &index);
        assert_eq!(id, "56_written_123");
        assert_eq!(status, OralLinkStatus::Ambiguous);
    }
}
