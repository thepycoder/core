use crate::common::{SESSION_ID, UnresolvedRow, dedupe_unresolved, reason_label, split_csv};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use identity::actor_resolver::{ActorResolution, ActorResolver};
use identity::external::department_external_id;
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::Bucket;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct NormalizedAnswerRow {
    pub answer_id: String,
    pub question_id: String,
    pub route_id: String,
    pub kind: String,
    pub text_nl: String,
    pub text_fr: String,
    pub status: String,
    pub source_kind: String,
    pub source_url: String,
    pub cache_path: String,
    pub confidence: String,
}

#[derive(Debug, Clone)]
pub struct AnsweredByRow {
    pub answered_by_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub answer_id: String,
    pub question_id: String,
    pub raw_name: String,
    pub source_url: String,
    pub cache_path: String,
    pub confidence: String,
}

pub struct WrittenAnswersOutput {
    pub answers: Vec<NormalizedAnswerRow>,
    pub answered_by: Vec<AnsweredByRow>,
    pub unresolved: Vec<UnresolvedRow>,
}

fn load_answer_paths(data_dir: &Path) -> Vec<PathBuf> {
    vec![
        data_dir.join(format!("sessions/{SESSION_ID}/written/answers.parquet")),
        data_dir.join(format!("sessions/{SESSION_ID}/commission/answers.parquet")),
        data_dir.join(format!("sessions/{SESSION_ID}/plenary/answers.parquet")),
    ]
}

pub fn normalize_written_answers(
    data_dir: &Path,
    actor_resolver: &ActorResolver,
    canonical_map: &std::collections::HashMap<String, String>,
) -> Result<WrittenAnswersOutput, Box<dyn Error>> {
    let mut answers = Vec::new();
    let mut answered_by = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen_answers: HashSet<String> = HashSet::new();
    let mut seen_answered_by: HashSet<(String, String)> = HashSet::new();

    let question_respondents = load_question_respondents(data_dir)?;
    let route_deptnums = load_route_deptnums(data_dir)?;

    for path in load_answer_paths(data_dir) {
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let answer_ids = read_string_column(&batch, "answer_id")?;
            let question_ids = read_string_column(&batch, "question_id")?;
            let route_ids = read_string_column(&batch, "route_id")?;
            let kinds = read_string_column(&batch, "kind")?;
            let text_nl = read_string_column(&batch, "text_nl")?;
            let text_fr = read_string_column(&batch, "text_fr")?;
            let statuses = read_string_column(&batch, "status")?;
            let source_kinds = read_string_column(&batch, "source_kind")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            let confidences = read_string_column(&batch, "confidence")?;

            for i in 0..batch.num_rows() {
                if answer_ids[i].is_empty() {
                    continue;
                }
                let question_id = canonical_map
                    .get(&question_ids[i])
                    .cloned()
                    .unwrap_or_else(|| question_ids[i].clone());

                if seen_answers.insert(answer_ids[i].clone()) {
                    answers.push(NormalizedAnswerRow {
                        answer_id: answer_ids[i].clone(),
                        question_id: question_id.clone(),
                        route_id: route_ids[i].clone(),
                        kind: kinds[i].clone(),
                        text_nl: text_nl[i].clone(),
                        text_fr: text_fr[i].clone(),
                        status: statuses[i].clone(),
                        source_kind: source_kinds[i].clone(),
                        source_url: source_urls[i].clone(),
                        cache_path: cache_paths[i].clone(),
                        confidence: if confidences[i].is_empty() {
                            "parsed".to_string()
                        } else {
                            confidences[i].clone()
                        },
                    });
                }

                let respondents = question_respondents
                    .get(&question_id)
                    .or_else(|| question_respondents.get(&question_ids[i]))
                    .cloned()
                    .unwrap_or_default();

                if kinds[i] == "written" && !route_ids[i].is_empty() {
                    if let Some(deptnum) = route_deptnums.get(&route_ids[i]) {
                        let entity_id = department_external_id(&deptnum);
                        let key = (entity_id.clone(), answer_ids[i].clone());
                        if seen_answered_by.insert(key) {
                            answered_by.push(AnsweredByRow {
                                answered_by_id: format!("{}_{}", answer_ids[i], entity_id),
                                entity_type: "ExternalPerson".to_string(),
                                entity_id,
                                answer_id: answer_ids[i].clone(),
                                question_id: question_id.clone(),
                                raw_name: String::new(),
                                source_url: source_urls[i].clone(),
                                cache_path: cache_paths[i].clone(),
                                confidence: "exact".to_string(),
                            });
                        }
                    }
                } else {
                    for name in respondents {
                        let detail = actor_resolver.resolve_actor_detail(&name, Bucket::Respondent);
                        match detail.resolution {
                            ActorResolution::Person(entity_id) => {
                                let entity_type = "Person";
                                let key = (entity_id.clone(), answer_ids[i].clone());
                                if seen_answered_by.insert(key) {
                                    answered_by.push(AnsweredByRow {
                                        answered_by_id: format!("{}_{}", answer_ids[i], entity_id),
                                        entity_type: entity_type.to_string(),
                                        entity_id,
                                        answer_id: answer_ids[i].clone(),
                                        question_id: question_id.clone(),
                                        raw_name: name.clone(),
                                        source_url: source_urls[i].clone(),
                                        cache_path: cache_paths[i].clone(),
                                        confidence: "parsed".to_string(),
                                    });
                                }
                            }
                            ActorResolution::ExternalPerson(entity_id) => {
                                let entity_type = "ExternalPerson";
                                let key = (entity_id.clone(), answer_ids[i].clone());
                                if seen_answered_by.insert(key) {
                                    answered_by.push(AnsweredByRow {
                                        answered_by_id: format!("{}_{}", answer_ids[i], entity_id),
                                        entity_type: entity_type.to_string(),
                                        entity_id,
                                        answer_id: answer_ids[i].clone(),
                                        question_id: question_id.clone(),
                                        raw_name: name.clone(),
                                        source_url: source_urls[i].clone(),
                                        cache_path: cache_paths[i].clone(),
                                        confidence: "parsed".to_string(),
                                    });
                                }
                            }
                            ActorResolution::Unresolved(reason) => {
                                unresolved.push(UnresolvedRow {
                                    raw_name: detail.raw_name,
                                    typo_corrected: detail.typo_corrected,
                                    norm_primary: detail.norm_primary,
                                    norm_reordered: detail.norm_reordered,
                                    reason: reason_label(&reason).to_string(),
                                    source_bucket: "answer_respondent".to_string(),
                                    role: "respondent".to_string(),
                                    context_id: answer_ids[i].clone(),
                                    context_label: format!("answer {}", answer_ids[i]),
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
        }
    }

    answers.sort_by(|a, b| a.answer_id.cmp(&b.answer_id));
    answered_by.sort_by(|a, b| {
        a.answer_id
            .cmp(&b.answer_id)
            .then(a.entity_id.cmp(&b.entity_id))
    });
    dedupe_unresolved(&mut unresolved);
    Ok(WrittenAnswersOutput {
        answers,
        answered_by,
        unresolved,
    })
}

fn load_question_respondents(
    data_dir: &Path,
) -> Result<std::collections::HashMap<String, Vec<String>>, Box<dyn Error>> {
    let mut map = std::collections::HashMap::new();
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
            let respondents = read_string_column(&batch, "respondents")?;
            for i in 0..batch.num_rows() {
                map.insert(question_ids[i].clone(), split_csv(&respondents[i]));
            }
        }
    }
    Ok(map)
}

fn load_route_deptnums(data_dir: &Path) -> Result<HashMap<String, String>, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/written/routes.parquet"));
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let mut by_route = HashMap::new();
    for batch in read_all_rows(&path)? {
        let route_ids = read_string_column(&batch, "route_id")?;
        let deptnums = read_string_column(&batch, "deptnum")?;
        for i in 0..batch.num_rows() {
            if !route_ids[i].is_empty() {
                by_route.insert(route_ids[i].clone(), deptnums[i].clone());
            }
        }
    }
    Ok(by_route)
}

pub fn write_normalized_answers(
    path: &Path,
    rows: &[NormalizedAnswerRow],
) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("answer_id", false),
        utf8_field("question_id", false),
        utf8_field("route_id", false),
        utf8_field("kind", false),
        utf8_field("text_nl", false),
        utf8_field("text_fr", false),
        utf8_field("status", false),
        utf8_field("source_kind", false),
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
            col!(|r| r.answer_id.clone()),
            col!(|r| r.question_id.clone()),
            col!(|r| r.route_id.clone()),
            col!(|r| r.kind.clone()),
            col!(|r| r.text_nl.clone()),
            col!(|r| r.text_fr.clone()),
            col!(|r| r.status.clone()),
            col!(|r| r.source_kind.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}

pub fn write_answered_by(path: &Path, rows: &[AnsweredByRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("answered_by_id", false),
        utf8_field("entity_type", false),
        utf8_field("entity_id", false),
        utf8_field("answer_id", false),
        utf8_field("question_id", false),
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
            col!(|r| r.answered_by_id.clone()),
            col!(|r| r.entity_type.clone()),
            col!(|r| r.entity_id.clone()),
            col!(|r| r.answer_id.clone()),
            col!(|r| r.question_id.clone()),
            col!(|r| r.raw_name.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}
