use crate::types::{AliasCandidate, CheckDetail};
use identity::actor_resolver::{ActorResolution, ActorResolver};
use identity::normalize::clean_raw_name;
use identity::parquet_io::{read_all_rows, read_string_column};
use identity::resolver::{Bucket, Resolution, Resolver};
use normalize::common::UnresolvedRow;
use normalize::utterances::UtteranceRow;
use normalize::SESSION_ID;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;

pub struct SpeakerQaOutput {
    pub details: Vec<CheckDetail>,
    pub alias_candidates: Vec<AliasCandidate>,
}

pub fn run_speaker_checks(
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
        person_resolver,
        &mut details,
        &mut alias_candidates,
        &mut seen_alias,
    );
    scan_utterance_id_duplicates(data_dir, utterances, &mut details)?;
    scan_digit_prefix_resolvable(
        utterances,
        actor_resolver,
        &mut details,
        &mut alias_candidates,
        &mut seen_alias,
    );

    Ok(SpeakerQaOutput {
        details,
        alias_candidates,
    })
}

fn scan_resolvable_unresolved(
    unresolved: &[UnresolvedRow],
    person_resolver: &Resolver,
    details: &mut Vec<CheckDetail>,
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
    details: &mut Vec<CheckDetail>,
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
        for batch in read_all_rows(&path)? {
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
        let mut speaker_list: Vec<_> = speakers.into_iter().collect();
        speaker_list.sort();
        details.push(
            CheckDetail::new(
                "speaker.utterance_id_duplicates",
                "warn",
                "warn",
                format!(
                    "utterance_id {} shared by {} distinct speakers",
                    utterance_id,
                    speaker_list.len()
                ),
            )
            .with_session(SESSION_ID)
            .with_meeting(meeting_kind, meeting_id)
            .with_entity("utterance", &utterance_id)
            .with_values("unique raw_speaker per utterance_id", speaker_list.join(" | "))
            .with_source(source_url, cache_path),
        );
    }
    Ok(())
}

fn scan_digit_prefix_resolvable(
    utterances: &[UtteranceRow],
    actor_resolver: &ActorResolver,
    details: &mut Vec<CheckDetail>,
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
    details: &mut Vec<CheckDetail>,
    alias_candidates: &mut Vec<AliasCandidate>,
    seen_alias: &mut HashSet<(String, String, String)>,
) {
    details.push(
        CheckDetail::new(check_id, "warn", "warn", message)
            .with_session(SESSION_ID)
            .with_meeting("", meeting_id)
            .with_entity("person", person_id)
            .with_values("resolved at parse time", raw_name)
            .with_source(source_url, cache_path),
    );

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
