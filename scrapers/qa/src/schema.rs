use crate::io::parquet_row_count;
use crate::types::CheckDetail;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
struct TableSpec {
    rel_path: String,
    table_name: String,
    id_columns: Vec<String>,
    required_columns: Vec<String>,
}

fn staging_tables() -> Vec<TableSpec> {
    vec![
        TableSpec {
            rel_path: "sessions/56/members.parquet".into(),
            table_name: "members".into(),
            id_columns: vec!["first_name".into(), "last_name".into()],
            required_columns: vec!["session_id".into(), "fraction".into()],
        },
        TableSpec {
            rel_path: format!("sessions/{SESSION_ID}/plenary/meetings.parquet"),
            table_name: "plenary_meetings".into(),
            id_columns: vec!["meeting_id".into()],
            required_columns: vec!["session_id".into(), "date".into()],
        },
        TableSpec {
            rel_path: format!("sessions/{SESSION_ID}/commission/meetings.parquet"),
            table_name: "commission_meetings".into(),
            id_columns: vec!["meeting_id".into()],
            required_columns: vec!["session_id".into(), "date".into()],
        },
        TableSpec {
            rel_path: format!("sessions/{SESSION_ID}/plenary/questions.parquet"),
            table_name: "plenary_questions".into(),
            id_columns: vec!["question_id".into()],
            required_columns: vec!["session_id".into(), "meeting_id".into()],
        },
        TableSpec {
            rel_path: format!("sessions/{SESSION_ID}/commission/questions.parquet"),
            table_name: "commission_questions".into(),
            id_columns: vec!["question_id".into()],
            required_columns: vec![
                "session_id".into(),
                "meeting_id".into(),
                "internal_ids".into(),
            ],
        },
        TableSpec {
            rel_path: format!("sessions/{SESSION_ID}/written/questions.parquet"),
            table_name: "written_questions".into(),
            id_columns: vec!["question_id".into()],
            required_columns: vec!["docname".into(), "session_id".into()],
        },
        TableSpec {
            rel_path: format!("sessions/{SESSION_ID}/written/routes.parquet"),
            table_name: "written_routes".into(),
            id_columns: vec!["route_id".into()],
            required_columns: vec!["question_id".into(), "deptnum".into()],
        },
        TableSpec {
            rel_path: format!("sessions/{SESSION_ID}/written/answers.parquet"),
            table_name: "written_answers".into(),
            id_columns: vec!["answer_id".into()],
            required_columns: vec!["question_id".into()],
        },
        TableSpec {
            rel_path: format!("sessions/{SESSION_ID}/plenary/votes.parquet"),
            table_name: "plenary_votes".into(),
            id_columns: vec!["vote_id".into()],
            required_columns: vec!["session_id".into(), "meeting_id".into()],
        },
        TableSpec {
            rel_path: format!("sessions/{SESSION_ID}/dossiers.parquet"),
            table_name: "dossiers".into(),
            id_columns: vec!["id".into()],
            required_columns: vec!["session_id".into(), "title".into()],
        },
        TableSpec {
            rel_path: "lobby.parquet".into(),
            table_name: "lobby".into(),
            id_columns: vec!["name".into()],
            required_columns: vec!["name".into()],
        },
        TableSpec {
            rel_path: "remunerations.parquet".into(),
            table_name: "remunerations".into(),
            id_columns: vec![
                "first_name".into(),
                "last_name".into(),
                "year".into(),
                "mandate".into(),
                "institute".into(),
            ],
            required_columns: vec![
                "year".into(),
                "mandate".into(),
                "institute".into(),
                "remuneration_min".into(),
                "remuneration_max".into(),
            ],
        },
    ]
}

pub fn run_schema_checks(
    data_dir: &Path,
    qa_dir: &Path,
) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let prev_counts = load_row_counts(&qa_dir.join("row_counts.json"));

    let mut new_counts: HashMap<String, usize> = HashMap::new();

    for spec in staging_tables() {
        let path = data_dir.join(&spec.rel_path);
        if !path.exists() {
            details.push(
                CheckDetail::new(
                    "schema.table_loaded",
                    "error",
                    "fail",
                    format!("missing table {}", spec.rel_path),
                )
                .with_entity("table", &spec.table_name),
            );
            continue;
        }

        let row_count = parquet_row_count(&path)?;
        new_counts.insert(spec.table_name.clone(), row_count);

        if let Some(prev) = prev_counts.get(&spec.table_name) {
            let delta = row_count as i64 - *prev as i64;
            let pct = if *prev == 0 {
                100.0
            } else {
                (delta.abs() as f64 / *prev as f64) * 100.0
            };
            if pct > 25.0 && delta.abs() > 10 {
                details.push(
                    CheckDetail::new(
                        "schema.row_count_delta",
                        "warn",
                        "warn",
                        format!(
                            "table {} row count {} -> {} (delta {delta})",
                            spec.table_name, prev, row_count
                        ),
                    )
                    .with_entity("table", &spec.table_name)
                    .with_values(prev.to_string(), row_count.to_string()),
                );
            }
        }

        for batch in read_all_rows(&path)? {
            let fields: HashSet<String> = batch
                .schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect();
            for col in &spec.required_columns {
                if !fields.contains(col) {
                    details.push(
                        CheckDetail::new(
                            "schema.required_non_empty",
                            "error",
                            "fail",
                            format!("table {} missing required column {col}", spec.table_name),
                        )
                        .with_entity("table", &spec.table_name),
                    );
                } else {
                    let values = read_string_column(&batch, col)?;
                    let empty = values.iter().filter(|v| v.trim().is_empty()).count();
                    if empty > 0 {
                        details.push(
                            CheckDetail::new(
                                "schema.required_non_empty",
                                "warn",
                                "warn",
                                format!(
                                    "table {} column {col} has {empty} empty values",
                                    spec.table_name
                                ),
                            )
                            .with_entity("table", &spec.table_name)
                            .with_values("0 empty", empty.to_string()),
                        );
                    }
                }
            }

            if !spec.id_columns.is_empty() && spec.id_columns.iter().all(|c| fields.contains(c)) {
                let mut seen: HashSet<String> = HashSet::new();
                let cols: Vec<Vec<String>> = spec
                    .id_columns
                    .iter()
                    .map(|c| read_string_column(&batch, c))
                    .collect::<Result<_, _>>()?;
                for i in 0..batch.num_rows() {
                    let key: String = spec
                        .id_columns
                        .iter()
                        .enumerate()
                        .map(|(j, c)| format!("{c}={}", cols[j][i]))
                        .collect::<Vec<_>>()
                        .join("|");
                    if !seen.insert(key.clone()) {
                        details.push(
                            CheckDetail::new(
                                "schema.unique_keys",
                                "warn",
                                "warn",
                                format!("duplicate key in {}: {key}", spec.table_name),
                            )
                            .with_entity("table", &spec.table_name),
                        );
                    }
                }
            }
        }

        // Commission questions: dossier_ids must not exist (legacy mislabel)
        if spec.table_name == "commission_questions" {
            for batch in read_all_rows(&path)? {
                if batch
                    .schema()
                    .fields()
                    .iter()
                    .any(|f| f.name() == "dossier_ids")
                {
                    details.push(
                        CheckDetail::new(
                            "schema.internal_ids_present",
                            "error",
                            "fail",
                            "commission questions still has legacy dossier_ids column",
                        )
                        .with_entity("table", &spec.table_name),
                    );
                }
            }
        }
    }

    write_row_counts(&qa_dir.join("row_counts.json"), &new_counts)?;
    Ok(details)
}

fn load_row_counts(path: &Path) -> HashMap<String, usize> {
    if !path.exists() {
        return HashMap::new();
    }
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            if let Some(n) = val.as_u64() {
                out.insert(k.clone(), n as usize);
            }
        }
    }
    out
}

fn write_row_counts(path: &Path, counts: &HashMap<String, usize>) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let v: HashMap<&str, usize> = counts.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    fs::write(path, serde_json::to_string_pretty(&json!(v))?)?;
    Ok(())
}
