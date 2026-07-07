//! Regression guards for speaker turn parsing and name resolution.

use crate::common::{UnresolvedRow, SESSION_ID};
use crate::utterances::UtteranceRow;
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use identity::actor_resolver::{ActorResolution, ActorResolver};
use identity::normalize::clean_raw_name;
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::{Bucket, Resolution, Resolver};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SpeakerCheckDetail {
    pub check_id: String,
    pub severity: String,
    pub status: String,
    pub session_id: String,
    pub meeting_kind: String,
    pub meeting_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub expected: String,
    pub actual: String,
    pub message: String,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone)]
pub struct AliasCandidate {
    pub raw_name: String,
    pub cleaned_name: String,
    pub matched_person_id: String,
    pub source_bucket: String,
    pub context_id: String,
    pub check_id: String,
    pub confidence: String,
}

#[derive(Debug, Clone)]
pub struct CheckSummary {
    pub table: String,
    pub check: String,
    pub status: String,
    pub count: usize,
    pub detail: String,
}

pub struct SpeakerQaOutput {
    pub details: Vec<SpeakerCheckDetail>,
    pub alias_candidates: Vec<AliasCandidate>,
    pub summaries: Vec<CheckSummary>,
}

pub fn run_speaker_qa(
    data_dir: &Path,
    utterances: &[UtteranceRow],
    unresolved: &[UnresolvedRow],
    actor_resolver: &ActorResolver,
    person_resolver: &Resolver,
) -> Result<SpeakerQaOutput, Box<dyn Error>> {
    let mut details = Vec::new();
    let mut alias_candidates = Vec::new();
    let mut seen_alias: HashSet<(String, String, String)> = HashSet::new();

    scan_resolvable_unresolved(
        unresolved,
        actor_resolver,
        person_resolver,
        &mut details,
        &mut alias_candidates,
        &mut seen_alias,
    );
    scan_utterance_id_duplicates(data_dir, utterances, &mut details)?;
    scan_digit_prefix_resolvable(utterances, actor_resolver, &mut details, &mut alias_candidates, &mut seen_alias);

    let summaries = summarize_checks(&details);
    Ok(SpeakerQaOutput {
        details,
        alias_candidates,
        summaries,
    })
}

fn scan_resolvable_unresolved(
    unresolved: &[UnresolvedRow],
    _actor_resolver: &ActorResolver,
    person_resolver: &Resolver,
    details: &mut Vec<SpeakerCheckDetail>,
    alias_candidates: &mut Vec<AliasCandidate>,
    seen_alias: &mut HashSet<(String, String, String)>,
) {
    for row in unresolved {
        if !matches!(row.source_bucket.as_str(), "speakers" | "questioners") {
            continue;
        }
        let bucket = match row.source_bucket.as_str() {
            "questioners" => Bucket::Questioner,
            _ => Bucket::Speaker,
        };

        let cleaned = clean_raw_name(&row.raw_name);
        let person_detail = person_resolver.resolve_detail(&cleaned, bucket);
        if let Resolution::Resolved(person_id) = person_detail.resolution {
            emit_regression(
                "speaker.cleaned_re_resolves",
                &row.raw_name,
                &cleaned,
                &person_id,
                &row.source_bucket,
                &row.context_id,
                &format!(
                    "unresolved `{}` ({}) re-resolves to person {} after cleaning",
                    row.raw_name, row.reason, person_id
                ),
                &row.source_url,
                &row.cache_path,
                "",
                details,
                alias_candidates,
                seen_alias,
            );
        }
    }
}

fn scan_utterance_id_duplicates(
    data_dir: &Path,
    normalized_utterances: &[UtteranceRow],
    details: &mut Vec<SpeakerCheckDetail>,
) -> Result<(), Box<dyn Error>> {
    let mut by_id: HashMap<String, HashSet<String>> = HashMap::new();
    let mut meta: HashMap<String, (String, String, String, String)> = HashMap::new();

    for rel in [
        format!("sessions/{SESSION_ID}/plenary/utterances.parquet"),
        format!("sessions/{SESSION_ID}/commission/utterances.parquet"),
    ] {
        let path = data_dir.join(&rel);
        if !path.exists() {
            continue;
        }
        let meeting_kind = if rel.contains("plenary") {
            "plenary"
        } else {
            "commission"
        };
        if let Ok(batches) = read_all_rows(&path) {
            for batch in batches {
                let utterance_ids = read_string_column(&batch, "utterance_id")?;
                let raw_speakers = read_string_column(&batch, "raw_speaker")?;
                let meeting_ids = read_string_column(&batch, "meeting_id")?;
                let source_urls = read_string_column(&batch, "source_url")?;
                let cache_paths = read_string_column(&batch, "cache_path")?;
                for i in 0..batch.num_rows() {
                    by_id
                        .entry(utterance_ids[i].clone())
                        .or_default()
                        .insert(raw_speakers[i].clone());
                    meta.entry(utterance_ids[i].clone()).or_insert_with(|| {
                        (
                            meeting_kind.to_string(),
                            meeting_ids[i].clone(),
                            source_urls[i].clone(),
                            cache_paths[i].clone(),
                        )
                    });
                }
            }
        }
    }

    if by_id.is_empty() {
        for u in normalized_utterances {
            by_id
                .entry(u.utterance_id.clone())
                .or_default()
                .insert(u.raw_speaker.clone());
            meta.entry(u.utterance_id.clone()).or_insert_with(|| {
                (
                    u.meeting_kind.clone(),
                    u.meeting_id.clone(),
                    u.source_url.clone(),
                    u.cache_path.clone(),
                )
            });
        }
    }

    for (utterance_id, speakers) in by_id {
        if speakers.len() <= 1 {
            continue;
        }
        let (meeting_kind, meeting_id, source_url, cache_path) =
            meta.get(&utterance_id).cloned().unwrap_or_default();
        let speaker_list: Vec<_> = {
            let mut v: Vec<_> = speakers.into_iter().collect();
            v.sort();
            v
        };
        details.push(SpeakerCheckDetail {
            check_id: "speaker.utterance_id_duplicates".to_string(),
            severity: "warn".to_string(),
            status: "warn".to_string(),
            session_id: SESSION_ID.to_string(),
            meeting_kind,
            meeting_id,
            entity_type: "utterance".to_string(),
            entity_id: utterance_id.clone(),
            expected: "unique raw_speaker per utterance_id".to_string(),
            actual: speaker_list.join(" | "),
            message: format!(
                "utterance_id {} shared by {} distinct speakers",
                utterance_id,
                speaker_list.len()
            ),
            source_url,
            cache_path,
        });
    }
    Ok(())
}

fn scan_digit_prefix_resolvable(
    utterances: &[UtteranceRow],
    actor_resolver: &ActorResolver,
    details: &mut Vec<SpeakerCheckDetail>,
    alias_candidates: &mut Vec<AliasCandidate>,
    seen_alias: &mut HashSet<(String, String, String)>,
) {
    for u in utterances {
        let raw = u.raw_speaker.trim();
        if !has_digit_prefix(raw) {
            continue;
        }
        let detail = actor_resolver.resolve_actor_detail(raw, Bucket::Speaker);
        if let ActorResolution::Person(person_id) = detail.resolution {
            emit_regression(
                "speaker.digit_prefix_resolvable",
                raw,
                &detail.typo_corrected,
                &person_id,
                "speakers",
                &u.utterance_id,
                &format!(
                    "digit-prefixed speaker `{}` resolves to person {} after cleaning",
                    raw, person_id
                ),
                &u.source_url,
                &u.cache_path,
                &u.meeting_id,
                details,
                alias_candidates,
                seen_alias,
            );
        }
    }
}

fn has_digit_prefix(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.len() < 3 {
        return false;
    }
    if !bytes[0].is_ascii_digit() || bytes[1].is_ascii_digit() || bytes[1] == b'.' {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i < bytes.len() && !bytes[i].is_ascii_whitespace()
}

fn emit_regression(
    check_id: &str,
    raw_name: &str,
    cleaned_name: &str,
    person_id: &str,
    source_bucket: &str,
    context_id: &str,
    message: &str,
    source_url: &str,
    cache_path: &str,
    meeting_id: &str,
    details: &mut Vec<SpeakerCheckDetail>,
    alias_candidates: &mut Vec<AliasCandidate>,
    seen_alias: &mut HashSet<(String, String, String)>,
) {
    details.push(SpeakerCheckDetail {
        check_id: check_id.to_string(),
        severity: "warn".to_string(),
        status: "warn".to_string(),
        session_id: SESSION_ID.to_string(),
        meeting_kind: String::new(),
        meeting_id: meeting_id.to_string(),
        entity_type: "person".to_string(),
        entity_id: person_id.to_string(),
        expected: "resolved at parse time".to_string(),
        actual: raw_name.to_string(),
        message: message.to_string(),
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
    });

    if seen_alias.insert((
        raw_name.to_string(),
        check_id.to_string(),
        context_id.to_string(),
    )) {
        alias_candidates.push(AliasCandidate {
            raw_name: raw_name.to_string(),
            cleaned_name: cleaned_name.to_string(),
            matched_person_id: person_id.to_string(),
            source_bucket: source_bucket.to_string(),
            context_id: context_id.to_string(),
            check_id: check_id.to_string(),
            confidence: "exact".to_string(),
        });
    }
}

fn summarize_checks(details: &[SpeakerCheckDetail]) -> Vec<CheckSummary> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for d in details {
        *counts.entry(d.check_id.clone()).or_default() += 1;
    }

    let check_ids = [
        "speaker.cleaned_re_resolves",
        "speaker.utterance_id_duplicates",
        "speaker.digit_prefix_resolvable",
    ];
    check_ids
        .iter()
        .map(|check_id| {
            let count = counts.get(*check_id).copied().unwrap_or(0);
            CheckSummary {
                table: "speakers".to_string(),
                check: (*check_id).to_string(),
                status: if count == 0 { "pass" } else { "warn" }.to_string(),
                count,
                detail: if count == 0 {
                    "no regressions".to_string()
                } else {
                    format!("{count} regression signal(s)")
                },
            }
        })
        .collect()
}

pub fn write_speaker_check_details(
    path: &Path,
    rows: &[SpeakerCheckDetail],
) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("check_id", false),
        utf8_field("severity", false),
        utf8_field("status", false),
        utf8_field("session_id", false),
        utf8_field("meeting_kind", false),
        utf8_field("meeting_id", false),
        utf8_field("entity_type", false),
        utf8_field("entity_id", false),
        utf8_field("expected", false),
        utf8_field("actual", false),
        utf8_field("message", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
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
            col!(|r| r.check_id.clone()),
            col!(|r| r.severity.clone()),
            col!(|r| r.status.clone()),
            col!(|r| r.session_id.clone()),
            col!(|r| r.meeting_kind.clone()),
            col!(|r| r.meeting_id.clone()),
            col!(|r| r.entity_type.clone()),
            col!(|r| r.entity_id.clone()),
            col!(|r| r.expected.clone()),
            col!(|r| r.actual.clone()),
            col!(|r| r.message.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
        ],
    )
}

pub fn write_alias_candidates(
    path: &Path,
    rows: &[AliasCandidate],
) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("raw_name", false),
        utf8_field("cleaned_name", false),
        utf8_field("matched_person_id", false),
        utf8_field("source_bucket", false),
        utf8_field("context_id", false),
        utf8_field("check_id", false),
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
            col!(|r| r.raw_name.clone()),
            col!(|r| r.cleaned_name.clone()),
            col!(|r| r.matched_person_id.clone()),
            col!(|r| r.source_bucket.clone()),
            col!(|r| r.context_id.clone()),
            col!(|r| r.check_id.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}

pub fn write_check_summaries(
    path: &Path,
    rows: &[CheckSummary],
) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("table", false),
        utf8_field("check", false),
        utf8_field("status", false),
        utf8_field("count", false),
        utf8_field("detail", false),
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
            col!(|r| r.table.clone()),
            col!(|r| r.check.clone()),
            col!(|r| r.status.clone()),
            col!(|r| r.count.to_string()),
            col!(|r| r.detail.clone()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_digit_prefix_detects_leaked_intervention_index() {
        assert!(has_digit_prefix("0 Steven Coenegrachts"));
        assert!(has_digit_prefix("7     Axel Ronse"));
        assert!(!has_digit_prefix("Steven Coenegrachts"));
        assert!(!has_digit_prefix("02.150 Steven Coenegrachts"));
    }
}
