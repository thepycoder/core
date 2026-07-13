use crate::build::{EdgeRow, register_artifact_edge};
use identity::parquet_io::{read_all_rows, read_string_column};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

const SESSION_ID: &str = "56";

pub fn load_written_question_nodes(
    data_dir: &Path,
    add_node: &mut impl FnMut(&str, &str, &str, &str, &str),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/written/questions.parquet"));
    if !path.exists() {
        return Ok(());
    }
    let merged = load_merged_written_ids(data_dir)?;
    for batch in read_all_rows(&path)? {
        let question_ids = read_string_column(&batch, "question_id")?;
        let titles_nl = read_string_column(&batch, "title_nl")?;
        let titles_fr = read_string_column(&batch, "title_fr")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            if merged.contains(&question_ids[i]) {
                continue;
            }
            let label: String = if !titles_nl[i].is_empty() {
                titles_nl[i].chars().take(120).collect()
            } else {
                titles_fr[i].chars().take(120).collect()
            };
            add_node(
                "Question",
                &question_ids[i],
                &label,
                &source_urls[i],
                &cache_paths[i],
            );
        }
    }
    Ok(())
}

pub fn load_answer_nodes(
    data_dir: &Path,
    add_node: &mut impl FnMut(&str, &str, &str, &str, &str),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/answers.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let answer_ids = read_string_column(&batch, "answer_id")?;
        let text_nl = read_string_column(&batch, "text_nl")?;
        let text_fr = read_string_column(&batch, "text_fr")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let excerpt: String = if !text_nl[i].is_empty() {
                text_nl[i].chars().take(120).collect()
            } else {
                text_fr[i].chars().take(120).collect()
            };
            let label = if excerpt.is_empty() {
                answer_ids[i].clone()
            } else {
                excerpt
            };
            add_node(
                "Answer",
                &answer_ids[i],
                &label,
                &source_urls[i],
                &cache_paths[i],
            );
        }
    }
    Ok(())
}

pub fn load_written_qa_edges(
    data_dir: &Path,
    edges: &mut Vec<EdgeRow>,
    artifact_registry: &mut HashMap<String, (String, String)>,
) -> Result<(), Box<dyn Error>> {
    load_written_asked_edges(data_dir, edges, artifact_registry)?;
    load_addressed_to_edges(data_dir, edges, artifact_registry)?;
    load_has_answer_edges(data_dir, edges, artifact_registry)?;
    load_answered_by_edges(data_dir, edges, artifact_registry)?;
    load_oral_reference_edges(data_dir, edges, artifact_registry)?;
    Ok(())
}

fn load_merged_written_ids(
    data_dir: &Path,
) -> Result<std::collections::HashSet<String>, Box<dyn Error>> {
    let mut merged = std::collections::HashSet::new();
    let path = data_dir.join("normalized/oral_written_links.parquet");
    if !path.exists() {
        return Ok(merged);
    }
    for batch in read_all_rows(&path)? {
        let written = read_string_column(&batch, "written_question_id")?;
        let canonical = read_string_column(&batch, "canonical_question_id")?;
        let statuses = read_string_column(&batch, "status")?;
        for i in 0..batch.num_rows() {
            if statuses[i] == "exact" && written[i] != canonical[i] {
                merged.insert(written[i].clone());
            }
        }
    }
    Ok(merged)
}

fn load_written_asked_edges(
    data_dir: &Path,
    edges: &mut Vec<EdgeRow>,
    artifact_registry: &mut HashMap<String, (String, String)>,
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/written_asked.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let person_ids = read_string_column(&batch, "person_id")?;
        let question_ids = read_string_column(&batch, "question_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_string_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            register_artifact_edge(
                edges,
                artifact_registry,
                "ASKED",
                "Person",
                &person_ids[i],
                "Question",
                &question_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                &confidences[i],
                "{}",
            );
        }
    }
    Ok(())
}

fn load_addressed_to_edges(
    data_dir: &Path,
    edges: &mut Vec<EdgeRow>,
    artifact_registry: &mut HashMap<String, (String, String)>,
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/addressed_to.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let question_ids = read_string_column(&batch, "question_id")?;
        let entity_types = read_string_column(&batch, "entity_type")?;
        let entity_ids = read_string_column(&batch, "entity_id")?;
        let properties = read_string_column(&batch, "properties_json")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_string_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            register_artifact_edge(
                edges,
                artifact_registry,
                "ADDRESSED_TO",
                "Question",
                &question_ids[i],
                &entity_types[i],
                &entity_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                &confidences[i],
                &properties[i],
            );
        }
    }
    Ok(())
}

fn load_has_answer_edges(
    data_dir: &Path,
    edges: &mut Vec<EdgeRow>,
    artifact_registry: &mut HashMap<String, (String, String)>,
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/answers.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let answer_ids = read_string_column(&batch, "answer_id")?;
        let question_ids = read_string_column(&batch, "question_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_string_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            register_artifact_edge(
                edges,
                artifact_registry,
                "HAS_ANSWER",
                "Question",
                &question_ids[i],
                "Answer",
                &answer_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                &confidences[i],
                "{}",
            );
        }
    }
    Ok(())
}

fn load_answered_by_edges(
    data_dir: &Path,
    edges: &mut Vec<EdgeRow>,
    artifact_registry: &mut HashMap<String, (String, String)>,
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/answered_by.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let entity_types = read_string_column(&batch, "entity_type")?;
        let entity_ids = read_string_column(&batch, "entity_id")?;
        let answer_ids = read_string_column(&batch, "answer_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_string_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            register_artifact_edge(
                edges,
                artifact_registry,
                "ANSWERED_BY",
                "Answer",
                &answer_ids[i],
                &entity_types[i],
                &entity_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                &confidences[i],
                "{}",
            );
        }
    }
    Ok(())
}

fn load_oral_reference_edges(
    data_dir: &Path,
    edges: &mut Vec<EdgeRow>,
    artifact_registry: &mut HashMap<String, (String, String)>,
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/oral_written_links.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let written = read_string_column(&batch, "written_question_id")?;
        let canonical = read_string_column(&batch, "canonical_question_id")?;
        let oral_refs = read_string_column(&batch, "oral_ref")?;
        let statuses = read_string_column(&batch, "status")?;
        for i in 0..batch.num_rows() {
            if statuses[i] != "exact" || written[i] == canonical[i] {
                continue;
            }
            let props = serde_json::json!({
                "oral_ref": oral_refs[i],
                "written_question_id": written[i],
            })
            .to_string();
            register_artifact_edge(
                edges,
                artifact_registry,
                "REFERENCES",
                "Question",
                &written[i],
                "Question",
                &canonical[i],
                "",
                "",
                "",
                "exact",
                &props,
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_ids_skip_exact_oral_matches() {
        let merged = std::collections::HashSet::from(["56_written_1".to_string()]);
        assert!(merged.contains("56_written_1"));
    }
}
