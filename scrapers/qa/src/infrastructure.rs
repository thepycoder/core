use crate::types::CheckDetail;
use crawl::paths::cache_dir;
use crawl::{
    ACCEPTED_GAP_REASONS, FRESHNESS_POLICIES, MANIFEST_STATUSES, days_between_rfc3339,
    is_known_manifest_status, meta_path_for, now_rfc3339, read_cache_metadata,
};
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;

pub fn run_infrastructure_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    details.extend(check_scraped_at(data_dir)?);
    details.extend(check_meeting_gaps(data_dir)?);
    details.extend(check_source_manifest_complete(data_dir)?);
    details.extend(check_source_cache_metadata(data_dir)?);
    details.extend(check_source_freshness(data_dir)?);
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
    let mut details = Vec::new();
    for kind in ["commission", "plenary"] {
        let path = data_dir.join(format!("sessions/{SESSION_ID}/{kind}/meeting_gaps.parquet"));
        if !path.exists() {
            // Plenary may not have been republished yet; commission should exist.
            if kind == "commission" {
                details.push(
                    CheckDetail::new(
                        "meeting.gaps",
                        "warn",
                        "warn",
                        format!("missing meeting gaps table {}", path.display()),
                    )
                    .with_entity("table", &format!("{kind}_meeting_gaps")),
                );
            }
            continue;
        }
        for batch in read_all_rows(&path)? {
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let reasons = read_string_column(&batch, "reason")?;
            let detail_cols = read_string_column(&batch, "detail")?;
            let kinds = if batch
                .schema()
                .fields()
                .iter()
                .any(|f| f.name() == "meeting_kind")
            {
                read_string_column(&batch, "meeting_kind")?
            } else {
                vec![kind.to_string(); batch.num_rows()]
            };
            for i in 0..batch.num_rows() {
                let reason = reasons[i].as_str();
                let meeting_kind = if kinds[i].is_empty() {
                    kind
                } else {
                    kinds[i].as_str()
                };
                if !ACCEPTED_GAP_REASONS.contains(&reason) {
                    details.push(
                        CheckDetail::new(
                            "meeting.gaps",
                            "error",
                            "fail",
                            format!(
                                "{meeting_kind} meeting {} has disallowed gap reason {reason} ({})",
                                meeting_ids[i], detail_cols[i]
                            ),
                        )
                        .with_session(SESSION_ID)
                        .with_meeting(meeting_kind, &meeting_ids[i])
                        .with_entity("meeting", &meeting_ids[i])
                        .with_warning_kind("integrity")
                        .with_values("accepted gap reason", reason),
                    );
                    continue;
                }
                details.push(
                    CheckDetail::new(
                        "meeting.gaps",
                        "info",
                        "info",
                        format!(
                            "{meeting_kind} meeting {} gap: {} ({})",
                            meeting_ids[i], reasons[i], detail_cols[i]
                        ),
                    )
                    .with_session(SESSION_ID)
                    .with_meeting(meeting_kind, &meeting_ids[i])
                    .with_entity("meeting", &meeting_ids[i])
                    .with_warning_kind("source_gap")
                    .with_values("scraped row", &reasons[i]),
                );
            }
        }
    }
    Ok(details)
}

fn check_source_manifest_complete(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let manifest_dir = data_dir.join("source_manifests");
    let expected = [
        "commission_meetings",
        "plenary_meetings",
        "sessions",
        "members",
        "commissions",
        "lobby",
        "remunerations",
        "dossiers",
    ];

    for source in expected {
        let path = manifest_dir.join(format!("{source}.parquet"));
        if !path.exists() {
            // Soft until scrapers have been re-run under the new contract.
            details.push(
                CheckDetail::new(
                    "source.manifest_complete",
                    "warn",
                    "warn",
                    format!("missing source manifest for {source}"),
                )
                .with_entity("source", source),
            );
            continue;
        }

        let mut keys: HashSet<(String, String, String, String)> = HashSet::new();
        for batch in read_all_rows(&path)? {
            let sources = read_string_column(&batch, "source")?;
            let sessions = read_string_column(&batch, "session_id")?;
            let kinds = read_string_column(&batch, "item_kind")?;
            let ids = read_string_column(&batch, "native_item_id")?;
            let statuses = read_string_column(&batch, "status")?;
            for i in 0..batch.num_rows() {
                if !is_known_manifest_status(&statuses[i]) {
                    details.push(
                        CheckDetail::new(
                            "source.manifest_complete",
                            "error",
                            "fail",
                            format!(
                                "unknown manifest status {:?} for {}/{} (allowed: {})",
                                statuses[i],
                                sources[i],
                                ids[i],
                                MANIFEST_STATUSES.join(", ")
                            ),
                        )
                        .with_entity("manifest_row", &format!("{}:{}", sources[i], ids[i]))
                        .with_values(MANIFEST_STATUSES.join("|"), &statuses[i]),
                    );
                }
                let key = (
                    sources[i].clone(),
                    sessions[i].clone(),
                    kinds[i].clone(),
                    ids[i].clone(),
                );
                if !keys.insert(key) {
                    details.push(
                        CheckDetail::new(
                            "source.manifest_complete",
                            "error",
                            "fail",
                            format!(
                                "duplicate manifest key {}/{}/{}/{}",
                                sources[i], sessions[i], kinds[i], ids[i]
                            ),
                        )
                        .with_entity("manifest_row", &format!("{}:{}", sources[i], ids[i])),
                    );
                }
            }
        }
    }
    Ok(details)
}

fn check_source_freshness(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let manifest_dir = data_dir.join("source_manifests");
    let now = now_rfc3339();

    for policy in FRESHNESS_POLICIES {
        let path = manifest_dir.join(format!("{}.parquet", policy.source));
        if !path.exists() {
            continue;
        }

        let mut oldest_checked = String::new();
        for batch in read_all_rows(&path)? {
            let checked = read_string_column(&batch, "checked_at")?;
            for ts in checked {
                if ts.trim().is_empty() {
                    continue;
                }
                if oldest_checked.is_empty() || ts.as_str() < oldest_checked.as_str() {
                    oldest_checked = ts;
                }
            }
        }

        if oldest_checked.is_empty() {
            details.push(
                CheckDetail::new(
                    "source.freshness",
                    "warn",
                    "warn",
                    format!(
                        "source {} manifest has no checked_at timestamps",
                        policy.source
                    ),
                )
                .with_entity("source", policy.source),
            );
            continue;
        }

        if let Some(age) = days_between_rfc3339(&oldest_checked, &now) {
            if age > policy.max_age_days {
                details.push(
                    CheckDetail::new(
                        "source.freshness",
                        "warn",
                        "warn",
                        format!(
                            "source {} oldest checked_at is {age} days ago (policy max {})",
                            policy.source, policy.max_age_days
                        ),
                    )
                    .with_entity("source", policy.source)
                    .with_values(policy.max_age_days.to_string(), age.to_string()),
                );
            }
        }
    }
    Ok(details)
}

fn check_source_cache_metadata(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let cache_root = cache_dir();
    let manifest_dir = data_dir.join("source_manifests");
    if !manifest_dir.exists() {
        return Ok(details);
    }

    for entry in std::fs::read_dir(&manifest_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("parquet") {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let statuses = read_string_column(&batch, "status")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            let hashes = read_string_column(&batch, "content_hash")?;
            let fetched = read_string_column(&batch, "fetched_at")?;
            let checked = read_string_column(&batch, "checked_at")?;
            let ids = read_string_column(&batch, "native_item_id")?;
            let sources = read_string_column(&batch, "source")?;
            for i in 0..batch.num_rows() {
                if statuses[i] != "parsed" && statuses[i] != "unsupported_format" {
                    continue;
                }
                let cp = cache_paths[i].trim();
                if cp.is_empty() {
                    details.push(
                        CheckDetail::new(
                            "source.cache_metadata",
                            "error",
                            "fail",
                            format!(
                                "{} item {} status {} lacks cache_path",
                                sources[i], ids[i], statuses[i]
                            ),
                        )
                        .with_entity("manifest_row", &format!("{}:{}", sources[i], ids[i])),
                    );
                    continue;
                }
                let full = cache_root.join(cp);
                if !full.exists() {
                    details.push(
                        CheckDetail::new(
                            "source.cache_metadata",
                            "error",
                            "fail",
                            format!("{} item {} cache missing on disk: {cp}", sources[i], ids[i]),
                        )
                        .with_entity("manifest_row", &format!("{}:{}", sources[i], ids[i]))
                        .with_source("", cp),
                    );
                    continue;
                }
                // Prefer sidecar when present; otherwise require non-empty manifest hash.
                match read_cache_metadata(&full)? {
                    Some(meta) => {
                        if !hashes[i].is_empty() && meta.content_hash != hashes[i] {
                            details.push(
                                CheckDetail::new(
                                    "source.cache_metadata",
                                    "error",
                                    "fail",
                                    format!(
                                        "{} item {} content_hash mismatch (manifest vs sidecar)",
                                        sources[i], ids[i]
                                    ),
                                )
                                .with_entity("manifest_row", &format!("{}:{}", sources[i], ids[i]))
                                .with_values(&meta.content_hash, &hashes[i])
                                .with_source("", cp),
                            );
                        }
                        if !meta.fetched_at.is_empty()
                            && !meta.checked_at.is_empty()
                            && meta.checked_at < meta.fetched_at
                        {
                            details.push(
                                CheckDetail::new(
                                    "source.cache_metadata",
                                    "error",
                                    "fail",
                                    format!(
                                        "{} item {} checked_at before fetched_at",
                                        sources[i], ids[i]
                                    ),
                                )
                                .with_entity("manifest_row", &format!("{}:{}", sources[i], ids[i]))
                                .with_values(&meta.fetched_at, &meta.checked_at)
                                .with_source("", cp),
                            );
                        }
                        let _ = meta_path_for(&full);
                    }
                    None => {
                        if hashes[i].is_empty() {
                            details.push(
                                CheckDetail::new(
                                    "source.cache_metadata",
                                    "warn",
                                    "warn",
                                    format!(
                                        "{} item {} has cache bytes but no .meta.json sidecar yet",
                                        sources[i], ids[i]
                                    ),
                                )
                                .with_entity("manifest_row", &format!("{}:{}", sources[i], ids[i]))
                                .with_source("", cp),
                            );
                        }
                        if !fetched[i].is_empty()
                            && !checked[i].is_empty()
                            && checked[i] < fetched[i]
                        {
                            details.push(
                                CheckDetail::new(
                                    "source.cache_metadata",
                                    "error",
                                    "fail",
                                    format!(
                                        "{} item {} checked_at before fetched_at (manifest)",
                                        sources[i], ids[i]
                                    ),
                                )
                                .with_entity("manifest_row", &format!("{}:{}", sources[i], ids[i]))
                                .with_values(&fetched[i], &checked[i]),
                            );
                        }
                    }
                }
            }
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
