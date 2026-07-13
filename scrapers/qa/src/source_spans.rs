//! Independent QA for canonical, typed `source_spans.parquet` provenance.

use crate::types::CheckDetail;
use arrow::datatypes::DataType;
use crawl::artifact_id::{
    BLOCK_PARSER_VERSION, MEETING_SCOPE_EXTRACTOR_VERSION, VOTE_EXTRACTOR_VERSION,
};
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;

const ALLOWED_ROLES: &[&str] = &[
    "meeting_scope",
    "agenda_item_scope",
    "entity_title",
    "question_participants",
    "hearing_participants",
    "hearing_body",
    "interpellation_participants",
    "interpellation_body",
    "utterance_text",
    "question_body",
    "answer_text",
    "proposition_body",
    "notice_body",
    "decision_title",
    "result_reuse",
    "result_reuse_reference",
    "result_reference",
    "result_table",
    "overall_counts",
    "language_group_counts",
    "appendix_header",
    "appendix_bucket_count",
    "appendix_voter_names",
    "quorum_statement",
    "quorum_participation",
    "secret_statistics",
    "candidate_tally",
    "formal_outcome",
    "proclamation",
];

const ALLOWED_FIELDS: &[&str] = &[
    "title_nl",
    "title_fr",
    "agenda_id",
    "internal_ids",
    "dossier_id",
    "document_id",
    "motion_id",
    "questioners",
    "respondents",
    "witnesses",
    "interpellators",
    "text",
    "raw_speaker",
    "speaker_role",
    "language",
    "question_body_nl",
    "question_body_fr",
    "treatment_mode",
    "text_nl",
    "text_fr",
    "method",
    "status",
    "outcome",
    "source_roll_call_number",
    "yes",
    "no",
    "abstain",
    "position",
    "count",
    "raw_name",
    "participated",
    "required",
    "selected",
    "result_id",
    "reuses_result",
];

const ALLOWED_UNRESOLVED_REASONS: &[&str] = &[
    "wrong_artifact",
    "stale_source_content",
    "stale_block_parser",
    "missing_entity_id",
    "invalid_half_open_range",
    "out_of_bounds",
];

#[derive(Debug, Clone)]
struct ArtifactMeta {
    block_count: u32,
    source_hash: String,
    parser_version: String,
}

#[derive(Debug, Clone)]
struct SpanRange {
    span_id: String,
    artifact_id: String,
    entity_type: String,
    entity_id: String,
    role: String,
    start: u32,
    end: u32,
    coverage: String,
    meeting_id: String,
    source_url: String,
    cache_path: String,
}

pub fn run_source_span_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let spans_path = data_dir.join(format!(
        "derived/sessions/{SESSION_ID}/plenary/source_spans.parquet"
    ));
    if !spans_path.exists() {
        return Ok(Vec::new());
    }

    let report_artifacts = load_report_artifacts(data_dir)?;
    let graph_artifacts = load_graph_artifacts(data_dir)?;
    let entity_ids = load_entity_ids(data_dir)?;
    let mut details = Vec::new();
    let mut valid_ranges = Vec::new();
    let mut seen_span_ids = HashSet::new();

    for batch in read_all_rows(&spans_path)? {
        for (column, expected) in [
            ("session_id", DataType::UInt32),
            ("meeting_id", DataType::UInt32),
            ("block_start", DataType::UInt32),
            ("block_end", DataType::UInt32),
            ("confidence", DataType::Float64),
        ] {
            let actual = batch.schema().field_with_name(column)?.data_type().clone();
            if actual != expected {
                details.push(CheckDetail::new(
                    "source.span.typed_schema",
                    "error",
                    "fail",
                    format!("{column} has type {actual}, expected {expected}"),
                ));
            }
        }

        let span_ids = read_string_column(&batch, "span_id")?;
        let artifact_ids = read_string_column(&batch, "artifact_id")?;
        let source_hashes = read_string_column(&batch, "source_content_hash")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let entity_types = read_string_column(&batch, "entity_type")?;
        let ids = read_string_column(&batch, "entity_id")?;
        let roles = read_string_column(&batch, "span_role")?;
        let starts = read_string_column(&batch, "block_start")?;
        let ends = read_string_column(&batch, "block_end")?;
        let coverage = read_string_column(&batch, "coverage_kind")?;
        let fields = read_string_column(&batch, "field_names")?;
        let extractors = read_string_column(&batch, "extractor")?;
        let parser_versions = read_string_column(&batch, "block_parser_version")?;
        let extractor_versions = read_string_column(&batch, "extractor_version")?;
        let statuses = read_string_column(&batch, "validation_status")?;
        let unresolved_reasons = read_string_column(&batch, "unresolved_reason")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            let start = starts[i].parse::<u32>().unwrap_or(u32::MAX);
            let end = ends[i].parse::<u32>().unwrap_or(u32::MAX);
            let status = statuses[i].as_str();
            let context = SpanContext {
                meeting_id: &meeting_ids[i],
                span_id: &span_ids[i],
                source_url: &source_urls[i],
                cache_path: &cache_paths[i],
            };

            if !seen_span_ids.insert(span_ids[i].clone()) {
                details.push(context.detail(
                    "source.span.overlap",
                    format!("duplicate span_id {}", span_ids[i]),
                ));
            }
            if !matches!(status, "valid" | "unresolved")
                || (status == "valid" && !unresolved_reasons[i].is_empty())
                || (status == "unresolved"
                    && !ALLOWED_UNRESOLVED_REASONS.contains(&unresolved_reasons[i].as_str()))
            {
                details.push(context.detail(
                    "source.span.validation_status",
                    format!(
                        "status={} unresolved_reason={}",
                        statuses[i], unresolved_reasons[i]
                    ),
                ));
            }
            if !report_artifacts.contains_key(&artifact_ids[i]) {
                details.push(context.detail(
                    "source.span.artifact_id",
                    format!("artifact {} absent from report_blocks", artifact_ids[i]),
                ));
            }
            if !graph_artifacts.contains_key(&artifact_ids[i]) {
                details.push(context.detail(
                    "source.span.graph_artifact",
                    format!(
                        "artifact {} absent from graph/source_artifacts",
                        artifact_ids[i]
                    ),
                ));
            }
            if status != "valid" {
                continue;
            }

            let Some(report) = report_artifacts.get(&artifact_ids[i]) else {
                continue;
            };
            if start >= end || end > report.block_count {
                details.push(
                    context
                        .detail(
                            "source.span.block_range",
                            format!(
                                "range {start}..{end} invalid for {} blocks",
                                report.block_count
                            ),
                        )
                        .with_values(
                            format!("0 <= start < end <= {}", report.block_count),
                            format!("{start}..{end}"),
                        ),
                );
            }
            if source_hashes[i] != report.source_hash
                || graph_artifacts
                    .get(&artifact_ids[i])
                    .is_some_and(|graph| graph.0 != source_hashes[i])
            {
                details.push(context.detail(
                    "source.span.source_content_stale",
                    "source_content_hash differs from report blocks or graph artifact",
                ));
            }
            if parser_versions[i] != report.parser_version
                || parser_versions[i] != BLOCK_PARSER_VERSION
                || graph_artifacts
                    .get(&artifact_ids[i])
                    .is_some_and(|graph| !graph.1.is_empty() && graph.1 != parser_versions[i])
            {
                details.push(context.detail(
                    "source.span.block_parser_stale",
                    format!("stale block_parser_version {}", parser_versions[i]),
                ));
            }
            let expected_extractor = match extractors[i].as_str() {
                "vote_assembly" => Some(VOTE_EXTRACTOR_VERSION),
                "meeting_parse" => Some(MEETING_SCOPE_EXTRACTOR_VERSION),
                _ => None,
            };
            if extractors[i].is_empty()
                || extractor_versions[i].is_empty()
                || expected_extractor.is_some_and(|expected| extractor_versions[i] != expected)
            {
                details.push(context.detail(
                    "source.span.extractor_version",
                    format!(
                        "extractor={} extractor_version={}",
                        extractors[i], extractor_versions[i]
                    ),
                ));
            }
            if !ALLOWED_ROLES.contains(&roles[i].as_str()) {
                details.push(context.detail(
                    "source.span.allowed_role",
                    format!("unknown span_role {}", roles[i]),
                ));
            }
            if coverage[i] == "extraction" {
                let invalid_fields = fields[i]
                    .split(',')
                    .map(str::trim)
                    .filter(|field| {
                        field.is_empty()
                            || !ALLOWED_FIELDS.contains(field)
                            || !field_allowed_for_entity(&entity_types[i], field)
                    })
                    .collect::<Vec<_>>();
                if invalid_fields.is_empty() && !fields[i].is_empty() {
                    // Valid extraction fields.
                } else {
                    details.push(context.detail(
                        "source.span.extraction_fields",
                        format!("invalid field_names {}", fields[i]),
                    ));
                }
            } else if coverage[i] != "scope" || !fields[i].is_empty() {
                details.push(context.detail(
                    "source.span.extraction_fields",
                    format!(
                        "coverage_kind={} requires empty scope fields or extraction fields",
                        coverage[i]
                    ),
                ));
            }
            if !entity_exists(&entity_ids, &entity_types[i], &ids[i], &meeting_ids[i]) {
                details.push(context.detail(
                    "source.span.entity_reference",
                    format!("{} {} not found", entity_types[i], ids[i]),
                ));
            }
            valid_ranges.push(SpanRange {
                span_id: span_ids[i].clone(),
                artifact_id: artifact_ids[i].clone(),
                entity_type: entity_types[i].clone(),
                entity_id: ids[i].clone(),
                role: roles[i].clone(),
                start,
                end,
                coverage: coverage[i].clone(),
                meeting_id: meeting_ids[i].clone(),
                source_url: source_urls[i].clone(),
                cache_path: cache_paths[i].clone(),
            });
        }
    }
    details.extend(check_overlaps(&valid_ranges));
    Ok(details)
}

struct SpanContext<'a> {
    meeting_id: &'a str,
    span_id: &'a str,
    source_url: &'a str,
    cache_path: &'a str,
}

impl SpanContext<'_> {
    fn detail(&self, check_id: &str, message: impl Into<String>) -> CheckDetail {
        CheckDetail::new(check_id, "warn", "warn", message)
            .with_session(SESSION_ID)
            .with_meeting("plenary", self.meeting_id)
            .with_entity("source_span", self.span_id)
            .with_source(self.source_url, self.cache_path)
    }
}

fn load_report_artifacts(data_dir: &Path) -> Result<HashMap<String, ArtifactMeta>, Box<dyn Error>> {
    let path = data_dir.join(format!(
        "derived/sessions/{SESSION_ID}/plenary/report_blocks.parquet"
    ));
    let mut out: HashMap<String, ArtifactMeta> = HashMap::new();
    if !path.exists() {
        return Ok(out);
    }
    for batch in read_all_rows(&path)? {
        let ids = read_string_column(&batch, "artifact_id")?;
        let hashes = read_string_column(&batch, "source_content_hash")?;
        let indices = read_string_column(&batch, "block_index")?;
        let parsers = read_string_column(&batch, "block_parser_version")?;
        for i in 0..batch.num_rows() {
            let block_count = indices[i].parse::<u32>().unwrap_or(0) + 1;
            let entry = out.entry(ids[i].clone()).or_insert_with(|| ArtifactMeta {
                block_count,
                source_hash: hashes[i].clone(),
                parser_version: parsers[i].clone(),
            });
            entry.block_count = entry.block_count.max(block_count);
        }
    }
    Ok(out)
}

fn load_graph_artifacts(
    data_dir: &Path,
) -> Result<HashMap<String, (String, String)>, Box<dyn Error>> {
    let path = data_dir.join("graph/source_artifacts.parquet");
    let mut out = HashMap::new();
    if !path.exists() {
        return Ok(out);
    }
    for batch in read_all_rows(&path)? {
        let ids = read_string_column(&batch, "source_artifact_id")?;
        let hashes = read_string_column(&batch, "source_content_hash")?;
        let parsers = read_string_column(&batch, "block_parser_version")?;
        for i in 0..batch.num_rows() {
            out.insert(ids[i].clone(), (hashes[i].clone(), parsers[i].clone()));
        }
    }
    Ok(out)
}

fn load_entity_ids(data_dir: &Path) -> Result<HashMap<String, HashSet<String>>, Box<dyn Error>> {
    let mut out = HashMap::new();
    for (entity_type, file, column) in [
        ("Vote", "votes.parquet", "vote_id"),
        ("VoteResult", "vote_results.parquet", "result_id"),
        ("Question", "questions.parquet", "question_id"),
        ("Answer", "answers.parquet", "answer_id"),
        ("Utterance", "utterances.parquet", "utterance_id"),
        ("Hearing", "hearings.parquet", "hearing_id"),
        (
            "Interpellation",
            "interpellations.parquet",
            "interpellation_id",
        ),
        ("Proposition", "propositions.parquet", "proposition_id"),
        ("Notice", "notices.parquet", "notice_id"),
    ] {
        let path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/{file}"));
        let mut ids = HashSet::new();
        if path.exists() {
            for batch in read_all_rows(&path)? {
                ids.extend(read_string_column(&batch, column)?);
            }
        }
        out.insert(entity_type.to_string(), ids);
    }
    Ok(out)
}

fn entity_exists(
    ids: &HashMap<String, HashSet<String>>,
    entity_type: &str,
    entity_id: &str,
    meeting_id: &str,
) -> bool {
    match entity_type {
        "Meeting" => entity_id == format!("plenary_{SESSION_ID}_{meeting_id}"),
        "AgendaItem" => !entity_id.is_empty(),
        _ => ids
            .get(entity_type)
            .is_some_and(|values| values.contains(entity_id)),
    }
}

fn field_allowed_for_entity(entity_type: &str, field: &str) -> bool {
    match entity_type {
        "Vote" => matches!(
            field,
            "title_nl"
                | "title_fr"
                | "dossier_id"
                | "document_id"
                | "motion_id"
                | "result_id"
                | "reuses_result"
        ),
        "VoteResult" => matches!(
            field,
            "method"
                | "status"
                | "outcome"
                | "source_roll_call_number"
                | "yes"
                | "no"
                | "abstain"
                | "position"
                | "count"
                | "raw_name"
                | "participated"
                | "required"
                | "selected"
                | "result_id"
        ),
        "Question" => matches!(
            field,
            "title_nl"
                | "title_fr"
                | "agenda_id"
                | "internal_ids"
                | "dossier_id"
                | "document_id"
                | "questioners"
                | "respondents"
                | "question_body_nl"
                | "question_body_fr"
                | "treatment_mode"
        ),
        "Answer" => matches!(field, "text_nl" | "text_fr"),
        "Utterance" => matches!(field, "text" | "raw_speaker" | "speaker_role" | "language"),
        "Hearing" => matches!(field, "title_nl" | "title_fr" | "witnesses"),
        "Interpellation" => matches!(
            field,
            "title_nl" | "title_fr" | "interpellators" | "respondents"
        ),
        "Proposition" | "Notice" | "AgendaItem" => matches!(
            field,
            "title_nl" | "title_fr" | "agenda_id" | "internal_ids" | "dossier_id" | "document_id"
        ),
        _ => false,
    }
}

fn check_overlaps(rows: &[SpanRange]) -> Vec<CheckDetail> {
    let mut details = Vec::new();
    for (index, left) in rows.iter().enumerate() {
        for right in &rows[index + 1..] {
            if left.artifact_id != right.artifact_id
                || left.entity_type != right.entity_type
                || left.entity_id != right.entity_id
                || left.role != right.role
                || left.coverage != "extraction"
                || right.coverage != "extraction"
            {
                continue;
            }
            if left.start < right.end && right.start < left.end {
                details.push(
                    CheckDetail::new(
                        "source.span.overlap",
                        "warn",
                        "warn",
                        format!(
                            "overlapping extraction spans {} {}..{} and {} {}..{}",
                            left.span_id,
                            left.start,
                            left.end,
                            right.span_id,
                            right.start,
                            right.end
                        ),
                    )
                    .with_session(SESSION_ID)
                    .with_meeting("plenary", &left.meeting_id)
                    .with_entity(&left.entity_type, &left.entity_id)
                    .with_source(&left.source_url, &left.cache_path),
                );
            }
        }
    }
    details
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_rules_allow_scope_and_cross_entity_overlap() {
        let base = SpanRange {
            span_id: "a".into(),
            artifact_id: "artifact".into(),
            entity_type: "Vote".into(),
            entity_id: "vote".into(),
            role: "decision_title".into(),
            start: 1,
            end: 3,
            coverage: "extraction".into(),
            meeting_id: "1".into(),
            source_url: String::new(),
            cache_path: String::new(),
        };
        let mut scope = base.clone();
        scope.span_id = "scope".into();
        scope.coverage = "scope".into();
        let mut other = base.clone();
        other.span_id = "other".into();
        other.entity_id = "other".into();
        assert!(check_overlaps(&[base, scope, other]).is_empty());
    }

    #[test]
    fn overlap_rules_reject_same_role_extraction_overlap() {
        let left = SpanRange {
            span_id: "left".into(),
            artifact_id: "artifact".into(),
            entity_type: "VoteResult".into(),
            entity_id: "result".into(),
            role: "result_table".into(),
            start: 1,
            end: 3,
            coverage: "extraction".into(),
            meeting_id: "1".into(),
            source_url: String::new(),
            cache_path: String::new(),
        };
        let mut right = left.clone();
        right.span_id = "right".into();
        right.start = 2;
        right.end = 4;
        assert_eq!(check_overlaps(&[left, right]).len(), 1);
    }
}
