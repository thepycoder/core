//! Transform-time provenance checks for normalized tables (issue 10).

use crate::types::CheckDetail;
use arrow::datatypes::DataType;
use crawl::artifact_id;
use identity::parquet_io::{read_all_rows, read_f64_column, read_string_column};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs::File;
use std::path::Path;

const PROVENANCE_COLUMNS: &[&str] = &[
    "source_url",
    "cache_path",
    "source_artifact_id",
    "source_content_hash",
    "block_parser_version",
    "extractor_version",
    "confidence",
];

/// Source-derived normalized tables and their primary row-id column.
fn provenance_tables() -> Vec<(&'static str, &'static str)> {
    vec![
        ("vote_casts", "vote_cast_id"),
        ("asked", "asked_id"),
        ("answered", "answered_id"),
        ("authored", "authored_id"),
        ("holds_role", "holds_role_id"),
        ("invited", "invited_id"),
        ("interpellated", "interpellated_id"),
        ("interpellation_responded", "responded_id"),
        ("written_asked", "asked_id"),
        ("addressed_to", "addressed_id"),
        ("answered_by", "answered_by_id"),
        ("answers", "answer_id"),
        ("oral_written_links", "written_question_id"),
        ("utterances", "utterance_id"),
        ("unresolved_persons", "raw_name"),
    ]
}

pub fn run_normalize_provenance_checks(
    data_dir: &Path,
) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let graph_artifacts = load_graph_artifact_ids(data_dir)?;

    for (table, row_id_col) in provenance_tables() {
        let path = data_dir.join(format!("normalized/{table}.parquet"));
        if !path.exists() {
            continue;
        }
        let field_names = parquet_field_names(&path)?;
        let field_types = parquet_field_types(&path)?;

        let mut missing_cols = Vec::new();
        for col in PROVENANCE_COLUMNS {
            if !field_names.contains(*col) {
                missing_cols.push(*col);
            }
        }
        if !missing_cols.is_empty() {
            details.push(
                CheckDetail::new(
                    "normalize.provenance_columns",
                    "error",
                    "fail",
                    format!(
                        "normalized/{table}.parquet missing provenance columns: {}",
                        missing_cols.join(", ")
                    ),
                )
                .with_entity("table", table),
            );
            continue;
        }

        if let Some(dt) = field_types.get("confidence") {
            if *dt != DataType::Float64 {
                details.push(
                    CheckDetail::new(
                        "normalize.confidence_typed",
                        "error",
                        "fail",
                        format!(
                            "normalized/{table}.parquet confidence must be FLOAT64, got {dt:?}"
                        ),
                    )
                    .with_entity("table", table),
                );
                continue;
            }
        }

        let batches = read_all_rows(&path)?;
        if batches.is_empty() {
            // Empty table still validated schema above.
            continue;
        }

        for batch in batches {
            let row_ids = if batch.schema().index_of(row_id_col).is_ok() {
                read_string_column(&batch, row_id_col)?
            } else {
                (0..batch.num_rows()).map(|i| i.to_string()).collect()
            };
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            let artifact_ids = read_string_column(&batch, "source_artifact_id")?;
            let content_hashes = read_string_column(&batch, "source_content_hash")?;
            let extractor_versions = read_string_column(&batch, "extractor_version")?;
            let confidences = read_f64_column(&batch, "confidence")?;

            for i in 0..batch.num_rows() {
                let entity_id = &row_ids[i];
                let has_source = !source_urls[i].is_empty() || !cache_paths[i].is_empty();
                if !has_source {
                    continue;
                }

                if artifact_ids[i].is_empty()
                    || content_hashes[i].is_empty()
                    || extractor_versions[i].is_empty()
                {
                    details.push(
                        CheckDetail::new(
                            "normalize.provenance_complete",
                            "error",
                            "fail",
                            format!("normalized/{table} row missing required provenance values"),
                        )
                        .with_entity(table, entity_id)
                        .with_source(&source_urls[i], &cache_paths[i]),
                    );
                }

                let conf = confidences[i];
                if !conf.is_finite() || !(0.0..=1.0).contains(&conf) {
                    details.push(
                        CheckDetail::new(
                            "normalize.confidence_typed",
                            "error",
                            "fail",
                            format!("normalized/{table} confidence out of range [0,1]: {conf}"),
                        )
                        .with_entity(table, entity_id)
                        .with_source(&source_urls[i], &cache_paths[i]),
                    );
                }

                let canonical = artifact_id(&source_urls[i], &cache_paths[i]);
                if artifact_ids[i] != canonical {
                    details.push(
                        CheckDetail::new(
                            "normalize.provenance_artifact_id",
                            "error",
                            "fail",
                            format!("normalized/{table} source_artifact_id mismatch"),
                        )
                        .with_entity(table, entity_id)
                        .with_values(&canonical, &artifact_ids[i])
                        .with_source(&source_urls[i], &cache_paths[i]),
                    );
                } else if !graph_artifacts.is_empty() && !graph_artifacts.contains(&artifact_ids[i])
                {
                    details.push(
                        CheckDetail::new(
                            "normalize.provenance_artifact_id",
                            "error",
                            "fail",
                            format!(
                                "normalized/{table} artifact missing from graph/source_artifacts.parquet"
                            ),
                        )
                        .with_entity(table, entity_id)
                        .with_values("present in graph", &artifact_ids[i])
                        .with_source(&source_urls[i], &cache_paths[i]),
                    );
                }
            }
        }
    }

    Ok(details)
}

fn parquet_field_names(path: &Path) -> Result<HashSet<String>, Box<dyn Error>> {
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    Ok(builder
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect())
}

fn parquet_field_types(path: &Path) -> Result<HashMap<String, DataType>, Box<dyn Error>> {
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    Ok(builder
        .schema()
        .fields()
        .iter()
        .map(|f| (f.name().clone(), f.data_type().clone()))
        .collect())
}

fn load_graph_artifact_ids(data_dir: &Path) -> Result<HashSet<String>, Box<dyn Error>> {
    let path = data_dir.join("graph/source_artifacts.parquet");
    let mut ids = HashSet::new();
    if !path.exists() {
        return Ok(ids);
    }
    for batch in read_all_rows(&path)? {
        for id in read_string_column(&batch, "source_artifact_id")? {
            if !id.is_empty() {
                ids.insert(id);
            }
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{ArrayRef, Float64Array, StringArray};
    use arrow::datatypes::{Field, Schema};
    use identity::parquet_io::{utf8_field, write_parquet};
    use std::sync::Arc;
    fn test_root(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("qa_provenance_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_asked(
        path: &Path,
        artifact_id: &str,
        content_hash: &str,
        confidence: f64,
        confidence_as_utf8: bool,
    ) {
        let mut fields = vec![
            utf8_field("asked_id", false),
            utf8_field("person_id", false),
            utf8_field("question_id", false),
            utf8_field("source_url", false),
            utf8_field("cache_path", false),
            utf8_field("source_artifact_id", false),
            utf8_field("source_content_hash", false),
            utf8_field("block_parser_version", false),
            utf8_field("extractor_version", false),
        ];
        if confidence_as_utf8 {
            fields.push(utf8_field("confidence", false));
        } else {
            fields.push(Field::new("confidence", DataType::Float64, false));
        }
        let schema = Schema::new(fields);
        let mut columns: Vec<ArrayRef> = vec![
            Arc::new(StringArray::from(vec!["a1"])) as ArrayRef,
            Arc::new(StringArray::from(vec!["p1"])) as ArrayRef,
            Arc::new(StringArray::from(vec!["q1"])) as ArrayRef,
            Arc::new(StringArray::from(vec!["https://example.test/r"])) as ArrayRef,
            Arc::new(StringArray::from(vec!["cache/r.html"])) as ArrayRef,
            Arc::new(StringArray::from(vec![artifact_id])) as ArrayRef,
            Arc::new(StringArray::from(vec![content_hash])) as ArrayRef,
            Arc::new(StringArray::from(vec![""])) as ArrayRef,
            Arc::new(StringArray::from(vec!["normalize_asked_v1"])) as ArrayRef,
        ];
        if confidence_as_utf8 {
            columns.push(Arc::new(StringArray::from(vec![confidence.to_string()])) as ArrayRef);
        } else {
            columns.push(Arc::new(Float64Array::from(vec![confidence])) as ArrayRef);
        }
        write_parquet(path, schema, columns).unwrap();
    }

    #[test]
    fn missing_provenance_column_fails_once() {
        let root = test_root("missing_cols");
        let norm = root.join("normalized");
        std::fs::create_dir_all(&norm).unwrap();
        let schema = Schema::new(vec![
            utf8_field("asked_id", false),
            utf8_field("source_url", false),
            utf8_field("cache_path", false),
        ]);
        write_parquet(
            &norm.join("asked.parquet"),
            schema,
            vec![
                Arc::new(StringArray::from(vec!["a1"])) as ArrayRef,
                Arc::new(StringArray::from(vec!["https://x"])) as ArrayRef,
                Arc::new(StringArray::from(vec!["c.html"])) as ArrayRef,
            ],
        )
        .unwrap();
        let details = run_normalize_provenance_checks(&root).unwrap();
        assert!(
            details
                .iter()
                .any(|d| d.check_id == "normalize.provenance_columns")
        );
    }

    #[test]
    fn blank_row_value_fails() {
        let root = test_root("blank_row");
        let norm = root.join("normalized");
        std::fs::create_dir_all(&norm).unwrap();
        let url = "https://example.test/r";
        let cache = "cache/r.html";
        let aid = artifact_id(url, cache);
        write_asked(&norm.join("asked.parquet"), &aid, "", 1.0, false);
        let details = run_normalize_provenance_checks(&root).unwrap();
        assert!(
            details
                .iter()
                .any(|d| d.check_id == "normalize.provenance_complete")
        );
    }

    #[test]
    fn valid_canonical_artifact_passes_without_graph() {
        let root = test_root("valid_artifact");
        let norm = root.join("normalized");
        std::fs::create_dir_all(&norm).unwrap();
        let url = "https://example.test/r";
        let cache = "cache/r.html";
        let aid = artifact_id(url, cache);
        write_asked(&norm.join("asked.parquet"), &aid, "abc", 0.8, false);
        let details = run_normalize_provenance_checks(&root).unwrap();
        assert!(
            details
                .iter()
                .all(|d| d.check_id != "normalize.provenance_artifact_id")
        );
    }

    #[test]
    fn wrong_artifact_id_fails() {
        let root = test_root("wrong_artifact");
        let norm = root.join("normalized");
        std::fs::create_dir_all(&norm).unwrap();
        write_asked(
            &norm.join("asked.parquet"),
            "not-the-canonical-id",
            "abc",
            1.0,
            false,
        );
        let details = run_normalize_provenance_checks(&root).unwrap();
        assert!(
            details
                .iter()
                .any(|d| d.check_id == "normalize.provenance_artifact_id")
        );
    }

    #[test]
    fn missing_from_graph_fails() {
        let root = test_root("missing_graph");
        let norm = root.join("normalized");
        let graph = root.join("graph");
        std::fs::create_dir_all(&norm).unwrap();
        std::fs::create_dir_all(&graph).unwrap();
        let url = "https://example.test/r";
        let cache = "cache/r.html";
        let aid = artifact_id(url, cache);
        write_asked(&norm.join("asked.parquet"), &aid, "abc", 1.0, false);
        let schema = Schema::new(vec![utf8_field("source_artifact_id", false)]);
        write_parquet(
            &graph.join("source_artifacts.parquet"),
            schema,
            vec![Arc::new(StringArray::from(vec!["other"])) as ArrayRef],
        )
        .unwrap();
        let details = run_normalize_provenance_checks(&root).unwrap();
        assert!(
            details
                .iter()
                .any(|d| d.check_id == "normalize.provenance_artifact_id"
                    && d.message.contains("missing from graph"))
        );
    }

    #[test]
    fn confidence_wrong_type_or_range_fails() {
        let root = test_root("confidence");
        let norm = root.join("normalized");
        std::fs::create_dir_all(&norm).unwrap();
        let url = "https://example.test/r";
        let cache = "cache/r.html";
        let aid = artifact_id(url, cache);
        write_asked(&norm.join("asked.parquet"), &aid, "abc", 1.0, true);
        let details = run_normalize_provenance_checks(&root).unwrap();
        assert!(
            details
                .iter()
                .any(|d| d.check_id == "normalize.confidence_typed")
        );

        write_asked(&norm.join("asked.parquet"), &aid, "abc", 1.5, false);
        let details = run_normalize_provenance_checks(&root).unwrap();
        assert!(
            details
                .iter()
                .any(|d| d.check_id == "normalize.confidence_typed")
        );
    }
}
