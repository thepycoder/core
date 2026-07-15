use crate::types::CheckDetail;
use chrono::{NaiveDate, Utc};
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use std::error::Error;
use std::path::Path;

const DATE_CHRONOLOGY_CHECK: &str = "dossier.date_chronology";

/// Flag source-backed impossible chronology on dossiers and future subdocument dates.
pub fn run_dossier_chronology_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    details.extend(check_dossier_dates(data_dir)?);
    details.extend(check_future_subdocument_dates(data_dir)?);
    Ok(details)
}

fn check_dossier_dates(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/dossiers.parquet"));
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let ids = read_string_column(&batch, "id")?;
        let submissions = read_string_column(&batch, "submission_date")?;
        let votes = read_string_column(&batch, "vote_date")?;
        let ends = read_string_column(&batch, "end_date")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            let graph_id = format!("{SESSION_ID}/{}", ids[i]);
            let submission = parse_iso_date(&submissions[i]);
            let vote = parse_iso_date(&votes[i]);
            let end = parse_iso_date(&ends[i]);

            if let (Some(sub), Some(v)) = (submission, vote) {
                if sub > v {
                    details.push(chronology_detail(
                        &graph_id,
                        &ids[i],
                        format!("submission_date={}", submissions[i]),
                        format!("vote_date={}", votes[i]),
                        format!(
                            "dossier {graph_id} source submission {} is after vote_date {}",
                            submissions[i], votes[i]
                        ),
                        &source_urls[i],
                        &cache_paths[i],
                    ));
                }
            }
            if let (Some(sub), Some(e)) = (submission, end) {
                if sub > e {
                    details.push(chronology_detail(
                        &graph_id,
                        &ids[i],
                        format!("submission_date={}", submissions[i]),
                        format!("end_date={}", ends[i]),
                        format!(
                            "dossier {graph_id} source submission {} is after end_date {}",
                            submissions[i], ends[i]
                        ),
                        &source_urls[i],
                        &cache_paths[i],
                    ));
                }
            }
        }
    }
    Ok(details)
}

fn check_future_subdocument_dates(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/subdocuments.parquet"));
    if !path.exists() {
        return Ok(Vec::new());
    }

    let today = Utc::now().date_naive();
    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let ids = read_string_column(&batch, "id")?;
        let dossier_ids = read_string_column(&batch, "dossier_id")?;
        let dates = read_string_column(&batch, "date")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            let Some(date) = parse_iso_date(&dates[i]) else {
                continue;
            };
            if date <= today {
                continue;
            }
            let graph_id = format!("{SESSION_ID}/{}", dossier_ids[i]);
            details.push(
                CheckDetail::new(
                    DATE_CHRONOLOGY_CHECK,
                    "warn",
                    "warn",
                    format!(
                        "subdocument {} on dossier {graph_id} has future source date {}",
                        ids[i], dates[i]
                    ),
                )
                .with_entity("subdocument", &ids[i])
                .with_graph_node("Dossier", &graph_id)
                .with_warning_kind("source_anomaly")
                .with_values(format!("date<={today}"), format!("date={}", dates[i]))
                .with_source(&source_urls[i], &cache_paths[i]),
            );
        }
    }
    Ok(details)
}

fn chronology_detail(
    graph_id: &str,
    dossier_id: &str,
    expected: String,
    actual: String,
    message: String,
    source_url: &str,
    cache_path: &str,
) -> CheckDetail {
    CheckDetail::new(DATE_CHRONOLOGY_CHECK, "warn", "warn", message)
        .with_entity("dossier", dossier_id)
        .with_graph_node("Dossier", graph_id)
        .with_warning_kind("source_anomaly")
        .with_values(expected, actual)
        .with_source(source_url, cache_path)
}

fn parse_iso_date(raw: &str) -> Option<NaiveDate> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso_date_accepts_canonical() {
        assert_eq!(
            parse_iso_date("2029-06-30"),
            Some(NaiveDate::from_ymd_opt(2029, 6, 30).unwrap())
        );
        assert_eq!(parse_iso_date(""), None);
        assert_eq!(parse_iso_date("30/06/2029"), None);
    }
}
