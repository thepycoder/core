use crate::types::CheckDetail;
use crawl::paths::cache_dir;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

pub fn run_infrastructure_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    details.extend(check_scraped_at(data_dir)?);
    details.extend(check_meeting_gaps(data_dir)?);
    details.extend(check_unresolved_rollup(data_dir)?);
    details.extend(check_cache_exists(data_dir)?);
    Ok(details)
}

fn check_scraped_at(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join("graph/source_artifacts.parquet");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let ids = read_string_column(&batch, "source_artifact_id")?;
        let scraped = read_string_column(&batch, "scraped_at")?;
        for i in 0..batch.num_rows() {
            if scraped[i].trim().is_empty() {
                details.push(
                    CheckDetail::new(
                        "artifact.scraped_at_populated",
                        "info",
                        "info",
                        format!("artifact {} missing scraped_at", ids[i]),
                    )
                    .with_entity("artifact", &ids[i]),
                );
            }
        }
    }
    Ok(details)
}

fn check_meeting_gaps(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join(format!(
        "sessions/{SESSION_ID}/commission/meeting_gaps.parquet"
    ));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let reasons = read_string_column(&batch, "reason")?;
        let detail_cols = read_string_column(&batch, "detail")?;
        for i in 0..batch.num_rows() {
            details.push(
                CheckDetail::new(
                    "commission.meeting_gaps",
                    "info",
                    "info",
                    format!(
                        "commission meeting {} gap: {} ({})",
                        meeting_ids[i], reasons[i], detail_cols[i]
                    ),
                )
                .with_session(SESSION_ID)
                .with_meeting("commission", &meeting_ids[i])
                .with_entity("meeting", &meeting_ids[i])
                .with_values("scraped row", &reasons[i]),
            );
        }
    }
    Ok(details)
}

fn check_unresolved_rollup(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join("normalized/unresolved_persons.parquet");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut buckets: HashMap<(String, String), usize> = HashMap::new();
    for batch in read_all_rows(&path)? {
        let source_buckets = read_string_column(&batch, "source_bucket")?;
        let reasons = read_string_column(&batch, "reason")?;
        for i in 0..batch.num_rows() {
            *buckets
                .entry((source_buckets[i].clone(), reasons[i].clone()))
                .or_default() += 1;
        }
    }

    let details: Vec<CheckDetail> = buckets
        .into_iter()
        .map(|((bucket, reason), count)| {
            CheckDetail::new(
                "normalize.unresolved_persons_by_bucket",
                "info",
                "info",
                format!("{count} unresolved in bucket {bucket} ({reason})"),
            )
            .with_entity("bucket", &format!("{bucket}:{reason}"))
            .with_values("0", count.to_string())
        })
        .collect();
    Ok(details)
}

fn check_cache_exists(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let cache_root = cache_dir();
    let tables = [
        "graph/source_artifacts.parquet",
        &format!("sessions/{SESSION_ID}/plenary/votes.parquet"),
        &format!("sessions/{SESSION_ID}/plenary/questions.parquet"),
    ];

    for rel in tables {
        let path = data_dir.join(rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            if !batch
                .schema()
                .fields()
                .iter()
                .any(|f| f.name() == "cache_path")
            {
                continue;
            }
            let cache_paths = read_string_column(&batch, "cache_path")?;
            let has_source_url = batch
                .schema()
                .fields()
                .iter()
                .any(|f| f.name() == "source_url");
            let source_urls = if has_source_url {
                read_string_column(&batch, "source_url")?
            } else {
                vec![String::new(); batch.num_rows()]
            };
            for i in 0..batch.num_rows() {
                let cp = cache_paths[i].trim();
                if cp.is_empty() {
                    continue;
                }
                let full = cache_root.join(cp);
                if !full.exists() {
                    details.push(
                        CheckDetail::new(
                            "source.cache_exists",
                            "warn",
                            "warn",
                            format!("cache_path missing on disk: {cp}"),
                        )
                        .with_source(source_urls[i].clone(), cp),
                    );
                }
            }
        }
    }
    Ok(details)
}
