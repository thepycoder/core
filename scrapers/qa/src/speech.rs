use crate::coverage_baseline::load_coverage_baseline;
use crate::types::{CheckDetail, CoverageBaselineRow};
use chrono::Utc;
use crawl::agenda_timeline::MeetingKind;
use crawl::paths::cache_dir;
use crawl::qa_coverage::{count_document_words_from_cache, word_count};
use crawl::qa_markers::check_markers_vs_utterances;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use arrow::record_batch::RecordBatch;
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

const CHECK_ID: &str = "utterance.speech_char_coverage";
const P5_MIN_MEETINGS: usize = 10;
const BASELINE_REGRESSION_FACTOR: f64 = 0.85;

pub struct SpeechCheckResult {
    pub details: Vec<CheckDetail>,
    pub coverage_snapshots: Vec<CoverageBaselineRow>,
}

pub fn run_speech_checks(data_dir: &Path) -> Result<SpeechCheckResult, Box<dyn Error>> {
    let saved = load_saved_document_words(data_dir)?;
    let coverage = check_speech_char_coverage(data_dir, &saved)?;
    let mut details = Vec::new();
    details.extend(check_source_markers(data_dir)?);
    details.extend(check_roundtrip_discussion(data_dir)?);
    details.extend(coverage.details);
    Ok(SpeechCheckResult {
        details,
        coverage_snapshots: coverage.snapshots,
    })
}

fn meeting_key(kind: &str, meeting_id: &str) -> (String, String) {
    (kind.to_string(), meeting_id.to_string())
}

fn add_words(counts: &mut HashMap<(String, String), usize>, key: (String, String), text: &str) {
    if !text.is_empty() {
        *counts.entry(key).or_default() += word_count(text);
    }
}

fn sum_row_words(batch: &RecordBatch, row: usize, columns: &[&str]) -> usize {
    let schema = batch.schema();
    columns
        .iter()
        .filter(|col| schema.fields().iter().any(|f| f.name() == **col))
        .map(|col| {
            read_string_column(batch, col)
                .map(|values| word_count(&values[row]))
                .unwrap_or(0)
        })
        .sum()
}

fn load_saved_document_words(
    data_dir: &Path,
) -> Result<HashMap<(String, String), usize>, Box<dyn Error>> {
    let mut counts: HashMap<(String, String), usize> = HashMap::new();

    for (kind, rel) in [
        ("plenary", format!("sessions/{SESSION_ID}/plenary/utterances.parquet")),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/utterances.parquet"),
        ),
    ] {
        let path = data_dir.join(&rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            for i in 0..batch.num_rows() {
                let key = meeting_key(kind, &meeting_ids[i]);
                *counts.entry(key).or_default() +=
                    sum_row_words(&batch, i, &["text", "raw_speaker"]);
            }
        }
    }

    for (kind, rel) in [
        ("plenary", format!("sessions/{SESSION_ID}/plenary/questions.parquet")),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/questions.parquet"),
        ),
    ] {
        let path = data_dir.join(&rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            for i in 0..batch.num_rows() {
                let key = meeting_key(kind, &meeting_ids[i]);
                *counts.entry(key).or_default() += sum_row_words(
                    &batch,
                    i,
                    &["topics_nl", "topics_fr", "questioners", "respondents"],
                );
            }
        }
    }

    let votes_path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet"));
    if votes_path.exists() {
        for batch in read_all_rows(&votes_path)? {
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            for i in 0..batch.num_rows() {
                let key = meeting_key("plenary", &meeting_ids[i]);
                *counts.entry(key).or_default() += sum_row_words(
                    &batch,
                    i,
                    &[
                        "title_nl",
                        "title_fr",
                        "members_yes",
                        "members_no",
                        "members_abstain",
                    ],
                );
            }
        }
    }

    for rel in [
        format!("sessions/{SESSION_ID}/plenary/propositions.parquet"),
        format!("sessions/{SESSION_ID}/plenary/notices.parquet"),
    ] {
        let path = data_dir.join(&rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            for i in 0..batch.num_rows() {
                let key = meeting_key("plenary", &meeting_ids[i]);
                *counts.entry(key).or_default() +=
                    sum_row_words(&batch, i, &["title_nl", "title_fr"]);
            }
        }
    }

    for rel in [format!("sessions/{SESSION_ID}/commission/meetings.parquet")] {
        let path = data_dir.join(&rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            if !batch.schema().fields().iter().any(|f| f.name() == "chair") {
                continue;
            }
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let chairs = read_string_column(&batch, "chair")?;
            for i in 0..batch.num_rows() {
                let key = meeting_key("commission", &meeting_ids[i]);
                add_words(&mut counts, key, &chairs[i]);
            }
        }
    }

    Ok(counts)
}

struct MeetingCoverage {
    meeting_kind: String,
    meeting_id: String,
    source_url: String,
    cache_path: String,
    source_words: usize,
    saved_words: usize,
    ratio: f64,
}

struct CoverageCheckOutput {
    details: Vec<CheckDetail>,
    snapshots: Vec<CoverageBaselineRow>,
}

fn check_speech_char_coverage(
    data_dir: &Path,
    saved_by_meeting: &HashMap<(String, String), usize>,
) -> Result<CoverageCheckOutput, Box<dyn Error>> {
    let baseline_path = data_dir.join("qa/speech_coverage_baseline.parquet");
    let baseline = load_coverage_baseline(&baseline_path)?;

    let mut meetings: Vec<MeetingCoverage> = Vec::new();

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
                let source_words = count_document_words_from_cache(&full)?;
                if source_words == 0 {
                    continue;
                }
                let key = meeting_key(kind.as_str(), &meeting_ids[i]);
                let saved_words = saved_by_meeting.get(&key).copied().unwrap_or(0);
                let ratio = saved_words as f64 / source_words as f64;
                meetings.push(MeetingCoverage {
                    meeting_kind: kind.as_str().to_string(),
                    meeting_id: meeting_ids[i].clone(),
                    source_url: source_urls[i].clone(),
                    cache_path: cache_path.clone(),
                    source_words,
                    saved_words,
                    ratio,
                });
            }
        }
    }

    let p5_plenary = percentile_5(
        &meetings
            .iter()
            .filter(|m| m.meeting_kind == "plenary")
            .map(|m| m.ratio)
            .collect::<Vec<_>>(),
    );
    let p5_commission = percentile_5(
        &meetings
            .iter()
            .filter(|m| m.meeting_kind == "commission")
            .map(|m| m.ratio)
            .collect::<Vec<_>>(),
    );

    let plenary_count = meetings
        .iter()
        .filter(|m| m.meeting_kind == "plenary")
        .count();
    let commission_count = meetings
        .iter()
        .filter(|m| m.meeting_kind == "commission")
        .count();

    let mut details = Vec::new();
    let now = Utc::now().to_rfc3339();
    let mut snapshots = Vec::with_capacity(meetings.len());

    for meeting in &meetings {
        snapshots.push(CoverageBaselineRow {
            meeting_kind: meeting.meeting_kind.clone(),
            meeting_id: meeting.meeting_id.clone(),
            source_words: meeting.source_words,
            saved_words: meeting.saved_words,
            ratio: meeting.ratio,
            updated_at: now.clone(),
        });

        let mut reasons = Vec::new();

        let p5 = if meeting.meeting_kind == "plenary" {
            p5_plenary
        } else {
            p5_commission
        };
        let kind_count = if meeting.meeting_kind == "plenary" {
            plenary_count
        } else {
            commission_count
        };

        if kind_count >= P5_MIN_MEETINGS && meeting.ratio < p5 {
            reasons.push(format!("below_p5({p5:.3})"));
        }

        if let Some(base) = baseline.get(&(
            meeting.meeting_kind.clone(),
            meeting.meeting_id.clone(),
        )) {
            if meeting.ratio < base.ratio * BASELINE_REGRESSION_FACTOR {
                reasons.push(format!(
                    "below_baseline({:.3} baseline={:.3})",
                    meeting.ratio, base.ratio
                ));
            }
        }

        if reasons.is_empty() {
            continue;
        }

        details.push(
            CheckDetail::new(
                CHECK_ID,
                "warn",
                "warn",
                format!(
                    "meeting {} {} document word coverage {:.3}: saved={} source={}; {}",
                    meeting.meeting_kind,
                    meeting.meeting_id,
                    meeting.ratio,
                    meeting.saved_words,
                    meeting.source_words,
                    reasons.join(", ")
                ),
            )
            .with_session(SESSION_ID)
            .with_meeting(&meeting.meeting_kind, &meeting.meeting_id)
            .with_entity("meeting", &meeting.meeting_id)
            .with_values(
                format!("source_words={}", meeting.source_words),
                format!(
                    "saved_words={} ratio={:.3}",
                    meeting.saved_words, meeting.ratio
                ),
            )
            .with_source(&meeting.source_url, &meeting.cache_path),
        );
    }

    Ok(CoverageCheckOutput { details, snapshots })
}

fn percentile_5(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((sorted.len() as f64) * 0.05).floor() as usize;
    sorted[idx.min(sorted.len() - 1)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_5_picks_lowest_bucket() {
        let values: Vec<f64> = (1..=20).map(|n| n as f64 / 20.0).collect();
        let p5 = percentile_5(&values);
        assert!((p5 - 0.05).abs() < 0.01 || (p5 - 0.1).abs() < 0.01);
    }

    #[test]
    fn baseline_regression_detected() {
        let baseline_ratio = 0.80;
        let current_ratio = 0.50;
        assert!(current_ratio < baseline_ratio * BASELINE_REGRESSION_FACTOR);
    }
}
