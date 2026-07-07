use crate::types::CheckDetail;
use crawl::paths::cache_dir;
use crawl::report_blocks::read_report_html;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;
use std::sync::OnceLock;

pub fn run_remaining_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    details.extend(check_meeting_metadata(data_dir)?);
    details.extend(check_vote_plausibility(data_dir)?);
    details.extend(check_motion_ids(data_dir)?);
    details.extend(check_fk_meetings(data_dir)?);
    details.extend(check_encoding(data_dir)?);
    details.extend(check_speaker_without_vote(data_dir)?);
    Ok(details)
}

fn check_meeting_metadata(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let path = data_dir.join(format!("sessions/{SESSION_ID}/commission/meetings.parquet"));
    if !path.exists() {
        return Ok(details);
    }
    for batch in read_all_rows(&path)? {
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let chairs = read_string_column(&batch, "chair")?;
        let dates = read_string_column(&batch, "date")?;
        let start_times = read_string_column(&batch, "start_time")?;
        let end_times = read_string_column(&batch, "end_time")?;
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
            let lower = html.to_lowercase();

            if !chairs[i].is_empty()
                && !lower.contains(&chairs[i].to_lowercase())
                && !lower.contains("voorgezeten")
                && !lower.contains("présidé")
            {
                details.push(
                    CheckDetail::new(
                        "meeting.chair_source_vs_parquet",
                        "warn",
                        "warn",
                        format!(
                            "meeting {} chair `{}` not found in source text",
                            meeting_ids[i], chairs[i]
                        ),
                    )
                    .with_meeting("commission", &meeting_ids[i])
                    .with_entity("meeting", &meeting_ids[i])
                    .with_source(&source_urls[i], cache_path),
                );
            }

            if !dates[i].is_empty() && !html.contains(&dates[i]) {
                details.push(
                    CheckDetail::new(
                        "meeting.date_source_vs_parquet",
                        "info",
                        "info",
                        format!("meeting {} date {} not verbatim in source", meeting_ids[i], dates[i]),
                    )
                    .with_meeting("commission", &meeting_ids[i])
                    .with_source(&source_urls[i], cache_path),
                );
            }

            if !start_times[i].is_empty() && !html.contains(&start_times[i].replace(':', ".")) {
                details.push(
                    CheckDetail::new(
                        "meeting.times_source_vs_parquet",
                        "info",
                        "info",
                        format!(
                            "meeting {} start_time {} not found in source",
                            meeting_ids[i], start_times[i]
                        ),
                    )
                    .with_meeting("commission", &meeting_ids[i])
                    .with_source(&source_urls[i], cache_path),
                );
            }
            let _ = &end_times[i];
        }
    }
    Ok(details)
}

fn check_vote_plausibility(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet"));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let yes = read_string_column(&batch, "yes")?;
        let no = read_string_column(&batch, "no")?;
        let abstain = read_string_column(&batch, "abstain")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let total: i32 = yes[i].parse().unwrap_or(0)
                + no[i].parse().unwrap_or(0)
                + abstain[i].parse().unwrap_or(0);
            if total > 150 {
                details.push(
                    CheckDetail::new(
                        "vote.total_plausibility",
                        "warn",
                        "warn",
                        format!("vote {} total {total} > 150", vote_ids[i]),
                    )
                    .with_meeting("plenary", &meeting_ids[i])
                    .with_entity("vote", &vote_ids[i])
                    .with_source(&source_urls[i], &cache_paths[i]),
                );
            }
        }
    }
    Ok(details)
}

fn motion_title_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)motie|motion").unwrap())
}

fn check_motion_ids(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet"));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let titles = read_string_column(&batch, "title_nl")?;
        let motion_ids = read_string_column(&batch, "motion_id")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            if motion_title_re().is_match(&titles[i]) && motion_ids[i].trim().is_empty() {
                details.push(
                    CheckDetail::new(
                        "vote.motion_id_when_referenced",
                        "info",
                        "info",
                        format!("vote {} title references motion but motion_id empty", vote_ids[i]),
                    )
                    .with_meeting("plenary", &meeting_ids[i])
                    .with_entity("vote", &vote_ids[i])
                    .with_source(&source_urls[i], &cache_paths[i]),
                );
            }
        }
    }
    Ok(details)
}

fn check_fk_meetings(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut meeting_ids: HashSet<String> = HashSet::new();
    for rel in [
        format!("sessions/{SESSION_ID}/plenary/meetings.parquet"),
        format!("sessions/{SESSION_ID}/commission/meetings.parquet"),
    ] {
        let path = data_dir.join(&rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            meeting_ids.extend(read_string_column(&batch, "meeting_id")?);
        }
    }

    let mut details = Vec::new();
    for (rel, kind) in [
        (
            format!("sessions/{SESSION_ID}/plenary/questions.parquet"),
            "plenary",
        ),
        (
            format!("sessions/{SESSION_ID}/commission/questions.parquet"),
            "commission",
        ),
        (
            format!("sessions/{SESSION_ID}/plenary/votes.parquet"),
            "plenary",
        ),
    ] {
        let path = data_dir.join(&rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let mids = read_string_column(&batch, "meeting_id")?;
            let ids = if rel.contains("votes") {
                read_string_column(&batch, "vote_id")?
            } else {
                read_string_column(&batch, "question_id")?
            };
            for i in 0..batch.num_rows() {
                if !meeting_ids.contains(&mids[i]) {
                    details.push(
                        CheckDetail::new(
                            "fk.questions_votes_to_meetings",
                            "error",
                            "fail",
                            format!(
                                "{} meeting_id {} not in meetings.parquet",
                                ids[i], mids[i]
                            ),
                        )
                        .with_meeting(kind, &mids[i])
                        .with_entity("row", &ids[i]),
                    );
                }
            }
        }
    }
    Ok(details)
}

fn check_encoding(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/meetings.parquet"));
    if !path.exists() {
        return Ok(details);
    }
    for batch in read_all_rows(&path)? {
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        for i in 0..batch.num_rows() {
            let cp = cache_paths[i].trim();
            if cp.is_empty() {
                continue;
            }
            let full = cache_dir().join(cp);
            if !full.exists() {
                continue;
            }
            let bytes = std::fs::read(&full)?;
            if bytes.contains(&0xFF) {
                details.push(
                    CheckDetail::new(
                        "source.encoding_bytes",
                        "info",
                        "info",
                        format!("meeting {} cache has 0xFF bytes", meeting_ids[i]),
                    )
                    .with_meeting("plenary", &meeting_ids[i])
                    .with_source(&source_urls[i], cp),
                );
            }
        }
    }
    Ok(details)
}

fn check_speaker_without_vote(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let utterances_path = data_dir.join("normalized/utterances.parquet");
    let casts_path = data_dir.join("normalized/vote_casts.parquet");
    if !utterances_path.exists() || !casts_path.exists() {
        return Ok(Vec::new());
    }

    let mut voters: HashSet<String> = HashSet::new();
    for batch in read_all_rows(&casts_path)? {
        let person_ids = read_string_column(&batch, "person_id")?;
        for p in person_ids {
            if !p.is_empty() {
                voters.insert(p);
            }
        }
    }

    let mut speakers: HashMap<String, String> = HashMap::new();
    for batch in read_all_rows(&utterances_path)? {
        let person_ids = read_string_column(&batch, "speaker_person_id")?;
        let meeting_kinds = read_string_column(&batch, "meeting_kind")?;
        let utterance_ids = read_string_column(&batch, "utterance_id")?;
        for i in 0..batch.num_rows() {
            if meeting_kinds[i] != "plenary" {
                continue;
            }
            let pid = person_ids[i].trim();
            if !pid.is_empty() && !voters.contains(pid) {
                speakers.insert(pid.to_string(), utterance_ids[i].clone());
            }
        }
    }

    let details: Vec<CheckDetail> = speakers
        .into_iter()
        .map(|(person_id, utterance_id)| {
            CheckDetail::new(
                "person.speaker_without_vote",
                "info",
                "info",
                format!("person {person_id} spoke in plenary but has no vote cast"),
            )
            .with_entity("person", &person_id)
            .with_source_block(&utterance_id)
        })
        .collect();
    Ok(details)
}
