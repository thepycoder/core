use crate::types::CheckDetail;
use crawl::agenda_timeline::MeetingKind;
use crawl::paths::cache_dir;
use crawl::qa_markers::check_markers_vs_utterances;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

pub fn run_speech_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    details.extend(check_source_markers(data_dir)?);
    details.extend(check_roundtrip_discussion(data_dir)?);
    Ok(details)
}

fn check_source_markers(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    for (kind, meetings_rel) in [
        (
            MeetingKind::Plenary,
            format!("sessions/{SESSION_ID}/plenary/meetings.parquet"),
        ),
        (
            MeetingKind::Commission,
            format!("sessions/{SESSION_ID}/commission/meetings.parquet"),
        ),
    ] {
        let meetings_path = data_dir.join(&meetings_rel);
        if !meetings_path.exists() {
            continue;
        }
        for batch in read_all_rows(&meetings_path)? {
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            for i in 0..batch.num_rows() {
                let cache_path = &cache_paths[i];
                if cache_path.is_empty() {
                    continue;
                }
                let full = cache_dir().join(cache_path);
                if !full.exists() {
                    continue;
                }
                let meeting_id: u32 = meeting_ids[i].parse().unwrap_or(0);
                let result = check_markers_vs_utterances(
                    &full,
                    kind,
                    SESSION_ID.parse().unwrap_or(56),
                    meeting_id,
                )?;
                if !result.ok {
                    details.push(
                        CheckDetail::new(
                            "utterance.source_markers_vs_normalized",
                            "warn",
                            "warn",
                            result.detail.clone(),
                        )
                        .with_meeting(kind.as_str(), &meeting_ids[i])
                        .with_entity("meeting", &meeting_ids[i])
                        .with_values(
                            format!(
                                "markers={}",
                                result.turn_markers + result.chair_markers
                            ),
                            result.utterance_rows.to_string(),
                        )
                        .with_source(&source_urls[i], cache_path),
                    );
                }
            }
        }
    }
    Ok(details)
}

fn check_roundtrip_discussion(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let utterances_path = data_dir.join("normalized/utterances.parquet");
    if !utterances_path.exists() {
        return Ok(Vec::new());
    }

    let mut by_question: HashMap<String, usize> = HashMap::new();
    for batch in read_all_rows(&utterances_path)? {
        let item_kinds = read_string_column(&batch, "item_kind")?;
        let item_ids = read_string_column(&batch, "item_id")?;
        let question_ids = read_string_column(&batch, "question_ids")?;
        for i in 0..batch.num_rows() {
            if item_kinds[i] != "question" {
                continue;
            }
            let key = if !item_ids[i].is_empty() {
                item_ids[i].clone()
            } else {
                question_ids[i].clone()
            };
            if !key.is_empty() {
                *by_question.entry(key).or_default() += 1;
            }
        }
    }

    let mut details = Vec::new();
    for rel in [
        format!("sessions/{SESSION_ID}/plenary/questions.parquet"),
        format!("sessions/{SESSION_ID}/commission/questions.parquet"),
    ] {
        let path = data_dir.join(&rel);
        if !path.exists() {
            continue;
        }
        let kind = if rel.contains("plenary") {
            "plenary"
        } else {
            "commission"
        };
        for batch in read_all_rows(&path)? {
            if !batch.schema().fields().iter().any(|f| f.name() == "discussion") {
                continue;
            }
            let question_ids = read_string_column(&batch, "question_id")?;
            let discussions = read_string_column(&batch, "discussion")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let disc_len = discussions[i].trim();
                let utterance_count = by_question.get(&question_ids[i]).copied().unwrap_or(0);
                if disc_len == "[]" && utterance_count > 0 {
                    details.push(
                        CheckDetail::new(
                            "utterance.roundtrip_discussion",
                            "warn",
                            "warn",
                            format!(
                                "question {} has {utterance_count} utterances but empty discussion JSON",
                                question_ids[i]
                            ),
                        )
                        .with_meeting(kind, &meeting_ids[i])
                        .with_entity("question", &question_ids[i])
                        .with_source(&source_urls[i], &cache_paths[i]),
                    );
                }
            }
        }
    }
    Ok(details)
}
