use crate::types::CheckDetail;
use crawl::agenda_timeline::{MeetingKind, count_agenda_questions_from_cache};
use crawl::paths::cache_dir;
use crawl::proceeding_entities::{is_hearing_heading, is_interpellation_heading};
use crawl::report_blocks::read_report_html;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

pub fn run_agenda_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    details.extend(check_entity_counts(data_dir)?);
    details.extend(check_dossier_refs(data_dir)?);
    details.extend(check_hearing_headings(data_dir)?);
    details.extend(check_interpellation_headings(data_dir)?);
    details.extend(check_question_internal_ids(data_dir)?);
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
                let did = dossier_ids_col[i].trim();
                if did.is_empty() || !did.contains('/') {
                    continue;
                }
                if !dossier_ids.contains_key(did) {
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
                        .with_entity("dossier", did)
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
