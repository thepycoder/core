use crate::types::CheckDetail;
use crawl::agenda_timeline::{
    MeetingKind, build_agenda_timeline, count_agenda_questions_from_cache, distinct_agenda_numbers,
};
use crawl::paths::cache_dir;
use crawl::proceeding_entities::{is_hearing_heading, is_interpellation_heading};
use crawl::report_blocks::{parse_report_blocks, read_report_html};
use crawl::utils::{ensure_question_id, normalize_site_ref};
use crawl::vote_inventory::numeric_sequence_gaps_from_one;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use scraper::Html;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;

pub fn run_agenda_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    details.extend(check_entity_counts(data_dir)?);
    details.extend(check_agenda_number_sequence(data_dir)?);
    details.extend(check_dossier_refs(data_dir)?);
    details.extend(check_hearing_headings(data_dir)?);
    details.extend(check_interpellation_headings(data_dir)?);
    details.extend(check_question_internal_ids(data_dir)?);
    details.extend(check_commission_questioners_resolved(data_dir)?);
    details.extend(check_utterance_interpellation_fk(data_dir)?);
    Ok(details)
}

fn check_entity_counts(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    for (kind, meetings_rel, questions_rel) in [
        (
            MeetingKind::Plenary,
            format!("sessions/{SESSION_ID}/plenary/meetings.parquet"),
            format!("sessions/{SESSION_ID}/plenary/questions.parquet"),
        ),
        (
            MeetingKind::Commission,
            format!("sessions/{SESSION_ID}/commission/meetings.parquet"),
            format!("sessions/{SESSION_ID}/commission/questions.parquet"),
        ),
    ] {
        let meetings_path = data_dir.join(&meetings_rel);
        let questions_path = data_dir.join(&questions_rel);
        if !meetings_path.exists() {
            continue;
        }

        let mut question_counts: HashMap<String, usize> = HashMap::new();
        if questions_path.exists() {
            for batch in read_all_rows(&questions_path)? {
                let meeting_ids = read_string_column(&batch, "meeting_id")?;
                for id in meeting_ids {
                    *question_counts.entry(id).or_default() += 1;
                }
            }
        }

        for batch in read_all_rows(&meetings_path)? {
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            for i in 0..batch.num_rows() {
                let meeting_id = &meeting_ids[i];
                let cache_path = &cache_paths[i];
                if cache_path.is_empty() {
                    continue;
                }
                let full = cache_dir().join(cache_path);
                if !full.exists() {
                    continue;
                }
                let meeting_num: u32 = meeting_id.parse().unwrap_or(0);
                let source_questions = count_agenda_questions_from_cache(
                    &full,
                    kind,
                    SESSION_ID.parse().unwrap_or(56),
                    meeting_num,
                )?;
                let parquet_questions = question_counts.get(meeting_id).copied().unwrap_or(0);
                if source_questions > 0 && parquet_questions == 0 {
                    details.push(
                        CheckDetail::new(
                            "agenda.entity_count_vs_parquet",
                            "warn",
                            "warn",
                            format!(
                                "meeting {meeting_id} source question headings {source_questions} vs parquet {parquet_questions}"
                            ),
                        )
                        .with_meeting(kind.as_str(), meeting_id)
                        .with_values(source_questions.to_string(), parquet_questions.to_string())
                        .with_source(&source_urls[i], cache_path),
                    );
                }
            }
        }
    }
    Ok(details)
}

fn check_agenda_number_sequence(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let session_id: u32 = SESSION_ID.parse().unwrap_or(56);
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
                let meeting_num: u32 = meeting_ids[i].parse().unwrap_or(0);
                let html = read_report_html(&full)?;
                let document = Html::parse_document(&html);
                let blocks = parse_report_blocks(&document);
                let items = build_agenda_timeline(&blocks, kind, session_id, meeting_num);
                let numbers = distinct_agenda_numbers(&items);
                for gap in numeric_sequence_gaps_from_one(&numbers) {
                    let gap_id = format!("{gap:02}");
                    details.push(
                        CheckDetail::new(
                            "agenda.number_sequence",
                            "warn",
                            "warn",
                            format!(
                                "meeting {} missing agenda item {gap_id} in sequence 1..max",
                                meeting_ids[i]
                            ),
                        )
                        .with_meeting(kind.as_str(), &meeting_ids[i])
                        .with_entity("agenda", &gap_id)
                        .with_source(&source_urls[i], cache_path),
                    );
                }
            }
        }
    }
    Ok(details)
}

fn check_dossier_refs(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let dossiers_path = data_dir.join(format!("sessions/{SESSION_ID}/dossiers.parquet"));
    if !dossiers_path.exists() {
        return Ok(Vec::new());
    }
    let mut dossier_ids: HashMap<String, bool> = HashMap::new();
    for batch in read_all_rows(&dossiers_path)? {
        let ids = read_string_column(&batch, "id")?;
        for id in ids {
            dossier_ids.insert(format!("{SESSION_ID}/{id}"), true);
        }
    }

    let votes_path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet"));
    let mut details = Vec::new();
    if votes_path.exists() {
        for batch in read_all_rows(&votes_path)? {
            let dossier_ids_col = read_string_column(&batch, "dossier_id")?;
            let vote_ids = read_string_column(&batch, "vote_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let raw = dossier_ids_col[i].trim();
                if raw.is_empty() {
                    continue;
                }
                let did = if raw.contains('/') {
                    raw.to_string()
                } else {
                    format!("{SESSION_ID}/{raw}")
                };
                if !dossier_ids.contains_key(&did) {
                    details.push(
                        CheckDetail::new(
                            "dossier.ref_exists",
                            "warn",
                            "warn",
                            format!(
                                "dossier ref {did} on vote {} not in dossiers.parquet",
                                vote_ids[i]
                            ),
                        )
                        .with_entity("dossier", &did)
                        .with_warning_kind("integrity")
                        .with_source(&source_urls[i], &cache_paths[i]),
                    );
                }
            }
        }
    }

    for kind in ["plenary", "commission"] {
        let agenda_path =
            data_dir.join(format!("sessions/{SESSION_ID}/{kind}/agenda_items.parquet"));
        if !agenda_path.exists() {
            continue;
        }
        for batch in read_all_rows(&agenda_path)? {
            let dossier_ids_col = read_string_column(&batch, "dossier_id")?;
            let agenda_item_ids = read_string_column(&batch, "agenda_item_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let raw = dossier_ids_col[i].trim();
                if raw.is_empty() {
                    continue;
                }
                let did = if raw.contains('/') {
                    raw.to_string()
                } else {
                    format!("{SESSION_ID}/{raw}")
                };
                if !dossier_ids.contains_key(&did) {
                    details.push(
                        CheckDetail::new(
                            "dossier.ref_exists",
                            "warn",
                            "warn",
                            format!(
                                "dossier ref {did} on agenda item {} not in dossiers.parquet",
                                agenda_item_ids[i]
                            ),
                        )
                        .with_entity("dossier", &did)
                        .with_warning_kind("integrity")
                        .with_source(&source_urls[i], &cache_paths[i]),
                    );
                }
            }
        }
    }
    Ok(details)
}

fn count_formal_hearing_headings(html: &str) -> usize {
    let re = regex::Regex::new(r"(?is)<h2[^>]*>(.*?)</h2>").unwrap();
    re.captures_iter(html)
        .filter_map(|cap| {
            let text = regex::Regex::new(r"<[^>]+>")
                .unwrap()
                .replace_all(&cap[1], " ")
                .to_string();
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if is_hearing_heading(&text.to_lowercase()) {
                Some(())
            } else {
                None
            }
        })
        .count()
}

fn count_formal_interpellation_headings(html: &str) -> usize {
    let re = regex::Regex::new(r"(?is)<h2[^>]*>(.*?)</h2>").unwrap();
    re.captures_iter(html)
        .filter_map(|cap| {
            let text = regex::Regex::new(r"<[^>]+>")
                .unwrap()
                .replace_all(&cap[1], " ")
                .to_string();
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if is_interpellation_heading(&text.to_lowercase()) {
                Some(())
            } else {
                None
            }
        })
        .count()
}

fn proceeding_counts_by_meeting(
    data_dir: &Path,
    rel_path: &str,
    id_column: &str,
) -> Result<HashMap<String, usize>, Box<dyn Error>> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let path = data_dir.join(rel_path);
    if !path.exists() {
        return Ok(counts);
    }
    for batch in read_all_rows(&path)? {
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let _ = read_string_column(&batch, id_column)?;
        for id in meeting_ids {
            *counts.entry(id).or_default() += 1;
        }
    }
    Ok(counts)
}

fn check_hearing_headings(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let meetings_path = data_dir.join(format!("sessions/{SESSION_ID}/commission/meetings.parquet"));
    let hearings_path = format!("sessions/{SESSION_ID}/commission/hearings.parquet");
    if !meetings_path.exists() {
        return Ok(details);
    }
    let hearing_counts = proceeding_counts_by_meeting(data_dir, &hearings_path, "hearing_id")?;
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
            let html = read_report_html(&full)?;
            let source_count = count_formal_hearing_headings(&html);
            if source_count == 0 {
                continue;
            }
            let parquet_count = hearing_counts.get(&meeting_ids[i]).copied().unwrap_or(0);
            if parquet_count == 0 {
                details.push(
                    CheckDetail::new(
                        "agenda.hearing_not_extracted",
                        "fail",
                        "fail",
                        format!(
                            "meeting {} has {source_count} formal hearing heading(s) but no hearings.parquet rows",
                            meeting_ids[i]
                        ),
                    )
                    .with_meeting("commission", &meeting_ids[i])
                    .with_values(source_count.to_string(), parquet_count.to_string())
                    .with_source(&source_urls[i], cache_path),
                );
            }
        }
    }
    Ok(details)
}

fn check_interpellation_headings(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let meetings_path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/meetings.parquet"));
    let interpellations_path = format!("sessions/{SESSION_ID}/plenary/interpellations.parquet");
    if !meetings_path.exists() {
        return Ok(details);
    }
    let interpellation_counts =
        proceeding_counts_by_meeting(data_dir, &interpellations_path, "interpellation_id")?;
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
            let html = read_report_html(&full)?;
            let source_count = count_formal_interpellation_headings(&html);
            if source_count == 0 {
                continue;
            }
            let parquet_count = interpellation_counts
                .get(&meeting_ids[i])
                .copied()
                .unwrap_or(0);
            if parquet_count == 0 {
                details.push(
                    CheckDetail::new(
                        "agenda.interpellation_not_extracted",
                        "fail",
                        "fail",
                        format!(
                            "meeting {} has {source_count} interpellation heading(s) but no interpellations.parquet rows",
                            meeting_ids[i]
                        ),
                    )
                    .with_meeting("plenary", &meeting_ids[i])
                    .with_values(source_count.to_string(), parquet_count.to_string())
                    .with_source(&source_urls[i], cache_path),
                );
            }
        }
    }
    Ok(details)
}

fn check_question_internal_ids(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
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
            let question_ids = read_string_column(&batch, "question_id")?;
            let internal_ids = read_string_column(&batch, "internal_ids")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                if internal_ids[i].trim().is_empty() {
                    details.push(
                        CheckDetail::new(
                            "question.grouped_internal_ids_complete",
                            "warn",
                            "warn",
                            format!("question {} missing internal_ids", question_ids[i]),
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

fn check_commission_questioners_resolved(
    data_dir: &Path,
) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let questions_path = data_dir.join(format!(
        "sessions/{SESSION_ID}/commission/questions.parquet"
    ));
    if !questions_path.exists() {
        return Ok(Vec::new());
    }

    let asked_path = data_dir.join("normalized/asked.parquet");
    let mut asked_question_ids = HashSet::new();
    for batch in read_all_rows(&asked_path)? {
        asked_question_ids.extend(read_string_column(&batch, "question_id")?);
    }

    let mut details = Vec::new();
    for batch in read_all_rows(&questions_path)? {
        let question_ids = read_string_column(&batch, "question_id")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let questioners = read_string_column(&batch, "questioners")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let question_id =
                ensure_question_id(&SESSION_ID.to_string(), "commission", &question_ids[i]);
            let raw_questioners = questioners[i].trim();
            if raw_questioners.is_empty() {
                details.push(
                    CheckDetail::new(
                        "question.questioner_resolved",
                        "warn",
                        "warn",
                        format!("commission question {question_id} has no staging questioner"),
                    )
                    .with_meeting("commission", &meeting_ids[i])
                    .with_entity("question", &question_id)
                    .with_values("non-empty questioners", raw_questioners)
                    .with_source(&source_urls[i], &cache_paths[i]),
                );
            } else if !asked_question_ids.contains(&question_id) {
                details.push(
                    CheckDetail::new(
                        "question.questioner_resolved",
                        "error",
                        "fail",
                        format!("commission question {question_id} has no resolved ASKED relation"),
                    )
                    .with_meeting("commission", &meeting_ids[i])
                    .with_entity("question", &question_id)
                    .with_values("at least one ASKED", raw_questioners)
                    .with_source(&source_urls[i], &cache_paths[i]),
                );
            }
        }
    }
    Ok(details)
}

fn check_utterance_interpellation_fk(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut canonical_ids: HashSet<(String, String, String, String)> = HashSet::new();
    let mut site_ref_targets: HashMap<(String, String, String, String), HashSet<String>> =
        HashMap::new();

    for (meeting_kind, rel_path) in [
        (
            "plenary",
            format!("sessions/{SESSION_ID}/plenary/interpellations.parquet"),
        ),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/interpellations.parquet"),
        ),
    ] {
        let path = data_dir.join(rel_path);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let ids = read_string_column(&batch, "interpellation_id")?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let internal_ids = read_string_column(&batch, "internal_ids")?;
            for i in 0..batch.num_rows() {
                let canonical_id = ensure_question_id(&session_ids[i], meeting_kind, &ids[i]);
                let scope = (
                    session_ids[i].clone(),
                    meeting_kind.to_string(),
                    meeting_ids[i].clone(),
                );
                canonical_ids.insert((
                    scope.0.clone(),
                    scope.1.clone(),
                    scope.2.clone(),
                    canonical_id.clone(),
                ));
                for site_ref in internal_ids[i].split(',') {
                    let site_ref = normalize_site_ref(site_ref);
                    if !site_ref.is_empty() {
                        site_ref_targets
                            .entry((scope.0.clone(), scope.1.clone(), scope.2.clone(), site_ref))
                            .or_default()
                            .insert(canonical_id.clone());
                    }
                }
            }
        }
    }

    let mut details = Vec::new();
    let mut seen = HashSet::new();
    for (meeting_kind, rel_path) in [
        (
            "plenary",
            format!("sessions/{SESSION_ID}/plenary/utterances.parquet"),
        ),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/utterances.parquet"),
        ),
    ] {
        let path = data_dir.join(rel_path);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let utterance_ids = read_string_column(&batch, "utterance_id")?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let item_kinds = read_string_column(&batch, "item_kind")?;
            let item_ids = read_string_column(&batch, "item_id")?;
            let question_ids = read_string_column(&batch, "question_ids")?;
            let block_starts = read_string_column(&batch, "block_start")?;
            let block_ends = read_string_column(&batch, "block_end")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                if item_kinds[i] != "interpellation" {
                    continue;
                }
                let actual_id = ensure_question_id(&session_ids[i], meeting_kind, &item_ids[i]);
                let scope = (
                    session_ids[i].clone(),
                    meeting_kind.to_string(),
                    meeting_ids[i].clone(),
                );
                if let Some(detail) = diagnose_interpellation_item_id(
                    &scope.0,
                    &scope.1,
                    &scope.2,
                    &utterance_ids[i],
                    &actual_id,
                    &question_ids[i],
                    &canonical_ids,
                    &site_ref_targets,
                    &source_urls[i],
                    &cache_paths[i],
                    &format!("{}:{}", block_starts[i], block_ends[i]),
                ) {
                    let key = (
                        detail.check_id.clone(),
                        scope.1.clone(),
                        scope.2.clone(),
                        actual_id.clone(),
                        detail.expected.clone(),
                        detail.actual.clone(),
                    );
                    if !seen.insert(key) {
                        continue;
                    }
                    details.push(detail);
                }
            }
        }
    }
    Ok(details)
}

/// Diagnose one interpellation utterance FK. Returns None when `actual_id` is already canonical.
fn diagnose_interpellation_item_id(
    session_id: &str,
    meeting_kind: &str,
    meeting_id: &str,
    utterance_id: &str,
    actual_id: &str,
    question_ids: &str,
    canonical_ids: &HashSet<(String, String, String, String)>,
    site_ref_targets: &HashMap<(String, String, String, String), HashSet<String>>,
    source_url: &str,
    cache_path: &str,
    source_block: &str,
) -> Option<CheckDetail> {
    if canonical_ids.contains(&(
        session_id.to_string(),
        meeting_kind.to_string(),
        meeting_id.to_string(),
        actual_id.to_string(),
    )) {
        return None;
    }

    let refs: Vec<String> = question_ids
        .split(',')
        .map(normalize_site_ref)
        .filter(|site_ref| !site_ref.is_empty())
        .collect();
    let mut candidates = HashSet::new();
    for site_ref in &refs {
        if let Some(targets) = site_ref_targets.get(&(
            session_id.to_string(),
            meeting_kind.to_string(),
            meeting_id.to_string(),
            site_ref.clone(),
        )) {
            candidates.extend(targets.iter().cloned());
        }
    }
    let mut candidates: Vec<_> = candidates.into_iter().collect();
    candidates.sort();
    let canonical_id = (candidates.len() == 1).then(|| candidates[0].clone());
    let (check_id, status) = if canonical_id.is_some() {
        ("utterance.interpellation_item_id_canonical", "warn")
    } else {
        ("fk.utterance_interpellation", "fail")
    };
    let expected = canonical_id.unwrap_or_else(|| "exactly one target".to_string());
    Some(
        CheckDetail::new(
            check_id,
            status,
            status,
            format!(
                "utterance {utterance_id} interpellation item_id {actual_id} does not resolve canonically"
            ),
        )
        .with_meeting(meeting_kind, meeting_id)
        .with_entity("utterance", utterance_id)
        .with_values(
            &expected,
            format!("actual={actual_id}; site_refs={}", refs.join(",")),
        )
        .with_source(source_url, cache_path)
        .with_source_block(source_block),
    )
}

#[cfg(test)]
mod interpellation_fk_tests {
    use super::*;

    fn empty_maps() -> (
        HashSet<(String, String, String, String)>,
        HashMap<(String, String, String, String), HashSet<String>>,
    ) {
        (HashSet::new(), HashMap::new())
    }

    #[test]
    fn direct_canonical_id_passes() {
        let (mut canonical, site_refs) = empty_maps();
        canonical.insert((
            "56".into(),
            "plenary".into(),
            "69".into(),
            "56_plenary_69_2".into(),
        ));
        let detail = diagnose_interpellation_item_id(
            "56",
            "plenary",
            "69",
            "u1",
            "56_plenary_69_2",
            "56000158I",
            &canonical,
            &site_refs,
            "url",
            "cache",
            "10:20",
        );
        assert!(detail.is_none());
    }

    #[test]
    fn wrong_id_with_unique_site_ref_emits_canonical_warn() {
        // Plenary 69: utterance historically used 56_plenary_69_5 for site ref 56000158I;
        // canonical entity is 56_plenary_69_2.
        let (mut canonical, mut site_refs) = empty_maps();
        canonical.insert((
            "56".into(),
            "plenary".into(),
            "69".into(),
            "56_plenary_69_2".into(),
        ));
        site_refs.insert(
            (
                "56".into(),
                "plenary".into(),
                "69".into(),
                "56000158I".into(),
            ),
            HashSet::from(["56_plenary_69_2".into()]),
        );
        let detail = diagnose_interpellation_item_id(
            "56",
            "plenary",
            "69",
            "u1",
            "56_plenary_69_5",
            "56000158I",
            &canonical,
            &site_refs,
            "url",
            "cache",
            "10:20",
        )
        .expect("finding");
        assert_eq!(
            detail.check_id,
            "utterance.interpellation_item_id_canonical"
        );
        assert_eq!(detail.status, "warn");
        assert_eq!(detail.expected, "56_plenary_69_2");
    }

    #[test]
    fn no_matching_site_ref_emits_fk_fail() {
        let (canonical, site_refs) = empty_maps();
        let detail = diagnose_interpellation_item_id(
            "56",
            "plenary",
            "69",
            "u1",
            "56_plenary_69_5",
            "56000158I",
            &canonical,
            &site_refs,
            "url",
            "cache",
            "10:20",
        )
        .expect("finding");
        assert_eq!(detail.check_id, "fk.utterance_interpellation");
        assert_eq!(detail.status, "fail");
    }

    #[test]
    fn ambiguous_site_ref_emits_fk_fail() {
        let (canonical, mut site_refs) = empty_maps();
        site_refs.insert(
            (
                "56".into(),
                "plenary".into(),
                "69".into(),
                "56000158I".into(),
            ),
            HashSet::from(["56_plenary_69_2".into(), "56_plenary_69_3".into()]),
        );
        let detail = diagnose_interpellation_item_id(
            "56",
            "plenary",
            "69",
            "u1",
            "56_plenary_69_5",
            "56000158I",
            &canonical,
            &site_refs,
            "url",
            "cache",
            "10:20",
        )
        .expect("finding");
        assert_eq!(detail.check_id, "fk.utterance_interpellation");
        assert_eq!(detail.status, "fail");
    }

    #[test]
    fn candidate_in_other_meeting_does_not_satisfy() {
        let (mut canonical, mut site_refs) = empty_maps();
        canonical.insert((
            "56".into(),
            "plenary".into(),
            "70".into(),
            "56_plenary_70_2".into(),
        ));
        site_refs.insert(
            (
                "56".into(),
                "plenary".into(),
                "70".into(),
                "56000158I".into(),
            ),
            HashSet::from(["56_plenary_70_2".into()]),
        );
        let detail = diagnose_interpellation_item_id(
            "56",
            "plenary",
            "69",
            "u1",
            "56_plenary_69_5",
            "56000158I",
            &canonical,
            &site_refs,
            "url",
            "cache",
            "10:20",
        )
        .expect("finding");
        assert_eq!(detail.check_id, "fk.utterance_interpellation");
        assert_eq!(detail.status, "fail");
    }

    #[test]
    fn repeated_utterances_with_same_bad_ref_share_diagnosis_shape() {
        let (mut canonical, mut site_refs) = empty_maps();
        canonical.insert((
            "56".into(),
            "plenary".into(),
            "69".into(),
            "56_plenary_69_2".into(),
        ));
        site_refs.insert(
            (
                "56".into(),
                "plenary".into(),
                "69".into(),
                "56000158I".into(),
            ),
            HashSet::from(["56_plenary_69_2".into()]),
        );
        let a = diagnose_interpellation_item_id(
            "56",
            "plenary",
            "69",
            "u1",
            "56_plenary_69_5",
            "56000158I",
            &canonical,
            &site_refs,
            "url",
            "cache",
            "10:20",
        )
        .unwrap();
        let b = diagnose_interpellation_item_id(
            "56",
            "plenary",
            "69",
            "u2",
            "56_plenary_69_5",
            "56000158I",
            &canonical,
            &site_refs,
            "url",
            "cache",
            "21:30",
        )
        .unwrap();
        assert_eq!(a.check_id, b.check_id);
        assert_eq!(a.expected, b.expected);
        assert_eq!(a.actual, b.actual);
    }
}
