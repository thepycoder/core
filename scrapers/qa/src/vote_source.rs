use crate::types::CheckDetail;
use crawl::vote_inventory::{
    inventory_vote_numbers, parse_vote_inventory, vote_number_gaps, VoteInventory,
};
use crawl::paths::cache_dir;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::common::split_csv;
use normalize::SESSION_ID;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;

pub fn run_vote_source_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let votes_path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet"));
    if !votes_path.exists() {
        return Ok(details);
    }

    let mut vote_rows: Vec<(String, String, String, String, String, String)> = Vec::new();
    for batch in read_all_rows(&votes_path)? {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let yes = read_string_column(&batch, "yes")?;
        let members_yes = read_string_column(&batch, "members_yes")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            vote_rows.push((
                vote_ids[i].clone(),
                meeting_ids[i].clone(),
                yes[i].clone(),
                members_yes[i].clone(),
                source_urls[i].clone(),
                cache_paths[i].clone(),
            ));
        }
    }

    let casts_path = data_dir.join("normalized/vote_casts.parquet");
    let mut casts_by_vote: HashMap<String, HashMap<String, HashSet<String>>> = HashMap::new();
    if casts_path.exists() {
        for batch in read_all_rows(&casts_path)? {
            let vote_ids = read_string_column(&batch, "vote_id")?;
            let person_ids = read_string_column(&batch, "person_id")?;
            let positions = read_string_column(&batch, "position")?;
            for i in 0..batch.num_rows() {
                casts_by_vote
                    .entry(vote_ids[i].clone())
                    .or_default()
                    .entry(positions[i].clone())
                    .or_default()
                    .insert(person_ids[i].clone());
            }
        }
    }

    // Per-meeting inventory from cache
    let mut meetings_done: HashSet<String> = HashSet::new();
    for batch in read_all_rows(&votes_path)? {
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        for i in 0..batch.num_rows() {
            let meeting_id = meeting_ids[i].clone();
            if !meetings_done.insert(meeting_id.clone()) {
                continue;
            }
            let cache_path = &cache_paths[i];
            if cache_path.is_empty() {
                continue;
            }
            let full_cache = cache_dir().join(cache_path);
            if !full_cache.exists() {
                continue;
            }
            let inv = parse_vote_inventory(&full_cache, &meeting_id)?;
            details.extend(check_inventory_vs_parquet(
                &inv,
                &vote_rows,
                &source_urls[i],
                cache_path,
            ));
            details.extend(check_compact_vs_appendix(&inv, &source_urls[i], cache_path));
            details.extend(check_vote_sequence(&inv, &source_urls[i], cache_path));
        }
    }

    // A9 duplicate person across buckets, A10 cast count vs headline
    for batch in read_all_rows(&votes_path)? {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let yes = read_string_column(&batch, "yes")?;
        let no = read_string_column(&batch, "no")?;
        let abstain = read_string_column(&batch, "abstain")?;
        let members_yes = read_string_column(&batch, "members_yes")?;
        let members_no = read_string_column(&batch, "members_no")?;
        let members_abstain = read_string_column(&batch, "members_abstain")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            let vote_id = &vote_ids[i];
            if let Some(buckets) = casts_by_vote.get(vote_id) {
                let mut person_positions: HashMap<String, Vec<String>> = HashMap::new();
                for (pos, persons) in buckets {
                    for p in persons {
                        person_positions.entry(p.clone()).or_default().push(pos.clone());
                    }
                }
                for (person, positions) in person_positions {
                    if positions.len() > 1 {
                        details.push(
                            CheckDetail::new(
                                "vote.duplicate_person_across_buckets",
                                "error",
                                "fail",
                                format!(
                                    "person {person} in multiple buckets for vote {vote_id}: {}",
                                    positions.join(",")
                                ),
                            )
                            .with_meeting("plenary", &meeting_ids[i])
                            .with_entity("vote", vote_id)
                            .with_source(&source_urls[i], &cache_paths[i]),
                        );
                    }
                }
                let yes_casts = buckets.get("yes").map(|s| s.len()).unwrap_or(0);
                let no_casts = buckets.get("no").map(|s| s.len()).unwrap_or(0);
                let abstain_casts = buckets.get("abstain").map(|s| s.len()).unwrap_or(0);
                let yes_h: usize = yes[i].parse().unwrap_or(0);
                let no_h: usize = no[i].parse().unwrap_or(0);
                let abstain_h: usize = abstain[i].parse().unwrap_or(0);
                if yes_casts != yes_h || no_casts != no_h || abstain_casts != abstain_h {
                    details.push(
                        CheckDetail::new(
                            "vote.cast_count_vs_headline",
                            "warn",
                            "warn",
                            format!(
                                "vote {vote_id} casts yes/no/abstain={yes_casts}/{no_casts}/{abstain_casts} vs headline {yes_h}/{no_h}/{abstain_h}"
                            ),
                        )
                        .with_meeting("plenary", &meeting_ids[i])
                        .with_entity("vote", vote_id)
                        .with_source(&source_urls[i], &cache_paths[i]),
                    );
                }
            }

            // S2 appendix bucket vs collected names (staging self-check extension)
            let my = split_csv(&members_yes[i]).len();
            let mn = split_csv(&members_no[i]).len();
            let ma = split_csv(&members_abstain[i]).len();
            let headline_yes: usize = yes[i].parse().unwrap_or(0);
            if my != headline_yes && headline_yes > 0 {
                details.push(
                    CheckDetail::new(
                        "vote.appendix_bucket_vs_collected_names",
                        "warn",
                        "warn",
                        format!(
                            "vote {vote_id} members_yes count {my} vs headline yes {headline_yes}"
                        ),
                    )
                    .with_meeting("plenary", &meeting_ids[i])
                    .with_entity("vote", vote_id)
                    .with_values(headline_yes.to_string(), my.to_string())
                    .with_source(&source_urls[i], &cache_paths[i]),
                );
            }
            let _ = (mn, ma);
        }
    }

    Ok(details)
}

fn check_inventory_vs_parquet(
    inv: &VoteInventory,
    vote_rows: &[(String, String, String, String, String, String)],
    source_url: &str,
    cache_path: &str,
) -> Vec<CheckDetail> {
    let source_nums = inventory_vote_numbers(inv);
    let parquet_count = vote_rows
        .iter()
        .filter(|r| r.1 == inv.meeting_id)
        .count();

    let mut details = Vec::new();
    if source_nums.len() != parquet_count {
        details.push(
            CheckDetail::new(
                "vote.source_inventory_vs_parquet",
                "warn",
                "warn",
                format!(
                    "meeting {} source votes {} vs parquet rows {}",
                    inv.meeting_id,
                    source_nums.len(),
                    parquet_count
                ),
            )
            .with_meeting("plenary", &inv.meeting_id)
            .with_values(source_nums.len().to_string(), parquet_count.to_string())
            .with_source(source_url, cache_path),
        );
    }
    details
}

fn check_compact_vs_appendix(
    inv: &VoteInventory,
    source_url: &str,
    cache_path: &str,
) -> Vec<CheckDetail> {
    let mut details = Vec::new();
    for num in &inv.appendix_vote_numbers {
        if !inv.compact_vote_numbers.contains(num) {
            details.push(
                CheckDetail::new(
                    "vote.compact_tables_vs_appendix_headers",
                    "warn",
                    "warn",
                    format!(
                        "meeting {} vote {num} in appendix but not in compact tables",
                        inv.meeting_id
                    ),
                )
                .with_meeting("plenary", &inv.meeting_id)
                .with_entity("vote", num)
                .with_source(source_url, cache_path),
            );
        }
    }
    details
}

fn check_vote_sequence(inv: &VoteInventory, source_url: &str, cache_path: &str) -> Vec<CheckDetail> {
    let nums = inventory_vote_numbers(inv);
    let gaps = vote_number_gaps(&nums);
    gaps.into_iter()
        .map(|gap| {
            CheckDetail::new(
                "vote.number_sequence",
                "warn",
                "warn",
                format!("meeting {} missing vote number {gap} in sequence", inv.meeting_id),
            )
            .with_meeting("plenary", &inv.meeting_id)
            .with_entity("vote", &gap)
            .with_source(source_url, cache_path)
        })
        .collect()
}
