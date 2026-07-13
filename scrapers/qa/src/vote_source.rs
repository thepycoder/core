use crate::types::CheckDetail;
use crawl::paths::cache_dir;
use crawl::vote_inventory::{
    FormalVoteOccurrence, VoteInventory, inventory_vote_numbers, parse_vote_inventory,
    vote_number_gaps,
};
use identity::parquet_io::{read_all_rows, read_string_column};
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

    let mut tally_map: HashMap<String, HashMap<String, HashMap<String, usize>>> = HashMap::new();
    let tallies_path = data_dir.join(format!(
        "sessions/{SESSION_ID}/plenary/vote_tallies.parquet"
    ));
    if tallies_path.exists() {
        for batch in read_all_rows(&tallies_path)? {
            let result_ids = read_string_column(&batch, "result_id")?;
            let option_keys = read_string_column(&batch, "option_key")?;
            let dimensions = read_string_column(&batch, "dimension")?;
            let tally_kinds = read_string_column(&batch, "tally_kind")?;
            let counts = read_string_column(&batch, "count")?;
            for i in 0..batch.num_rows() {
                tally_map
                    .entry(result_ids[i].clone())
                    .or_default()
                    .entry(dimensions[i].clone())
                    .or_default()
                    .insert(
                        format!("{}:{}", tally_kinds[i], option_keys[i]),
                        counts[i].parse().unwrap_or(0),
                    );
            }
        }
    }

    let casts_path = data_dir.join("normalized/vote_casts.parquet");
    let mut casts_by_result: HashMap<String, HashMap<String, HashSet<String>>> = HashMap::new();
    if casts_path.exists() {
        for batch in read_all_rows(&casts_path)? {
            let result_ids = read_string_column(&batch, "result_id")?;
            let person_ids = read_string_column(&batch, "person_id")?;
            let positions = read_string_column(&batch, "position")?;
            for i in 0..batch.num_rows() {
                casts_by_result
                    .entry(result_ids[i].clone())
                    .or_default()
                    .entry(positions[i].clone())
                    .or_default()
                    .insert(person_ids[i].clone());
            }
        }
    }

    let mut meetings_done: HashSet<String> = HashSet::new();
    let meetings_path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/meetings.parquet"));
    if meetings_path.exists() {
        for batch in read_all_rows(&meetings_path)? {
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
                let result_occurrences = inv
                    .formal_events
                    .iter()
                    .filter(|event| event.creates_result)
                    .cloned()
                    .collect::<Vec<_>>();
                let parquet_occurrences = load_result_occurrences(data_dir, &meeting_id)?;
                details.extend(check_result_occurrences(
                    &meeting_id,
                    &result_occurrences,
                    &parquet_occurrences,
                    &source_urls[i],
                    cache_path,
                ));
                details.extend(check_compact_vs_appendix(&inv, &source_urls[i], cache_path));
                details.extend(check_appendix_bucket_counts(
                    &inv,
                    &source_urls[i],
                    cache_path,
                ));
                details.extend(check_vote_sequence(&inv, &source_urls[i], cache_path));
            }
        }
    }

    details.extend(check_vote_evidence(data_dir)?);
    details.extend(check_unresolved_vote_events(data_dir)?);

    let results_path = data_dir.join(format!(
        "sessions/{SESSION_ID}/plenary/vote_results.parquet"
    ));
    if results_path.exists() {
        for batch in read_all_rows(&results_path)? {
            let result_ids = read_string_column(&batch, "result_id")?;
            let methods = read_string_column(&batch, "method")?;
            let named = read_string_column(&batch, "named")?;
            let statuses = read_string_column(&batch, "status")?;
            let outcomes = read_string_column(&batch, "outcome")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                let result_id = &result_ids[i];
                let method = &methods[i];
                let status = &statuses[i];
                let meeting_id = &meeting_ids[i];
                let source_url = &source_urls[i];
                let cache_path = &cache_paths[i];

                if is_named_roll_call(method, &named[i]) {
                    let tallies = tally_map.get(result_id);
                    let yes_h = overall_position_count(tallies, "yes");
                    let no_h = overall_position_count(tallies, "no");
                    let abstain_h = overall_position_count(tallies, "abstain");

                    if let Some(buckets) = casts_by_result.get(result_id) {
                        let mut person_positions: HashMap<String, Vec<String>> = HashMap::new();
                        for (pos, persons) in buckets {
                            for p in persons {
                                person_positions
                                    .entry(p.clone())
                                    .or_default()
                                    .push(pos.clone());
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
                                            "person {person} in multiple buckets for result {result_id}: {}",
                                            positions.join(",")
                                        ),
                                    )
                                    .with_meeting("plenary", meeting_id)
                                    .with_entity("vote_result", result_id)
                                    .with_source(source_url, cache_path),
                                );
                            }
                        }
                    }
                    let buckets = casts_by_result.get(result_id);
                    let yes_casts = buckets
                        .and_then(|value| value.get("yes"))
                        .map_or(0, HashSet::len);
                    let no_casts = buckets
                        .and_then(|value| value.get("no"))
                        .map_or(0, HashSet::len);
                    let abstain_casts = buckets
                        .and_then(|value| value.get("abstain"))
                        .map_or(0, HashSet::len);
                    if yes_casts != yes_h || no_casts != no_h || abstain_casts != abstain_h {
                        details.push(
                            CheckDetail::new(
                                "vote.cast_count_vs_headline",
                                "warn",
                                "warn",
                                format!(
                                    "result {result_id} casts yes/no/abstain={yes_casts}/{no_casts}/{abstain_casts} vs headline {yes_h}/{no_h}/{abstain_h}"
                                ),
                            )
                            .with_meeting("plenary", meeting_id)
                            .with_entity("vote_result", result_id)
                            .with_source(source_url, cache_path),
                        );
                    }
                }

                details.extend(check_method_invariants(
                    result_id,
                    method,
                    status,
                    &outcomes[i],
                    meeting_id,
                    tallies_map_for_result(&tally_map, result_id),
                    casts_by_result.get(result_id),
                    source_url,
                    cache_path,
                ));
            }
        }
    }

    Ok(details)
}

fn is_named_roll_call(method: &str, named: &str) -> bool {
    named == "true" && matches!(method, "roll_call" | "language_group_roll_call")
}

fn overall_position_count(
    tallies: Option<&HashMap<String, HashMap<String, usize>>>,
    option: &str,
) -> usize {
    tallies
        .and_then(|t| t.get("overall"))
        .and_then(|d| d.get(&format!("position:{option}")))
        .copied()
        .unwrap_or(0)
}

fn tallies_map_for_result<'a>(
    tally_map: &'a HashMap<String, HashMap<String, HashMap<String, usize>>>,
    result_id: &str,
) -> Option<&'a HashMap<String, HashMap<String, usize>>> {
    tally_map.get(result_id)
}

fn load_result_occurrences(
    data_dir: &Path,
    meeting_id: &str,
) -> Result<Vec<FormalVoteOccurrence>, Box<dyn Error>> {
    let path = data_dir.join(format!(
        "sessions/{SESSION_ID}/plenary/vote_results.parquet"
    ));
    let mut rows = Vec::new();
    for batch in read_all_rows(&path)? {
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let source_numbers = read_string_column(&batch, "source_roll_call_number")?;
        let seqs = read_string_column(&batch, "seq")?;
        let methods = read_string_column(&batch, "method")?;
        let statuses = read_string_column(&batch, "status")?;
        for i in 0..batch.num_rows() {
            if meeting_ids[i] != meeting_id {
                continue;
            }
            let seq = seqs[i].parse::<u32>().unwrap_or(0);
            let method = if statuses[i] == "no_quorum" {
                "no_quorum".to_string()
            } else {
                methods[i].clone()
            };
            rows.push((
                seq,
                FormalVoteOccurrence {
                    source_number: source_numbers[i].clone(),
                    occurrence: 0,
                    method,
                    block_index: 0,
                    creates_result: true,
                },
            ));
        }
    }
    rows.sort_by_key(|(seq, _)| *seq);
    let mut occurrences: HashMap<String, u32> = HashMap::new();
    Ok(rows
        .into_iter()
        .map(|(_, mut event)| {
            let key = if event.source_number.is_empty() {
                format!("@{}", event.method)
            } else {
                event.source_number.clone()
            };
            let occurrence = occurrences.entry(key).or_default();
            *occurrence += 1;
            event.occurrence = *occurrence;
            event
        })
        .collect())
}

fn check_result_occurrences(
    meeting_id: &str,
    inventory: &[FormalVoteOccurrence],
    parquet: &[FormalVoteOccurrence],
    source_url: &str,
    cache_path: &str,
) -> Vec<CheckDetail> {
    let inventory_keys = inventory
        .iter()
        .map(|event| {
            (
                event.source_number.as_str(),
                event.occurrence,
                event.method.as_str(),
            )
        })
        .collect::<Vec<_>>();
    let parquet_keys = parquet
        .iter()
        .map(|event| {
            (
                event.source_number.as_str(),
                event.occurrence,
                event.method.as_str(),
            )
        })
        .collect::<Vec<_>>();
    if inventory_keys == parquet_keys {
        return Vec::new();
    }

    vec![
        CheckDetail::new(
            "vote.source_inventory_vs_parquet",
            "warn",
            "warn",
            format!(
                "meeting {meeting_id} ordered formal events differ: inventory={inventory_keys:?} parquet={parquet_keys:?}"
            ),
        )
        .with_meeting("plenary", meeting_id)
        .with_values(inventory.len().to_string(), parquet.len().to_string())
        .with_source(source_url, cache_path),
    ]
}

fn check_method_invariants(
    result_id: &str,
    method: &str,
    status: &str,
    outcome: &str,
    meeting_id: &str,
    tallies: Option<&HashMap<String, HashMap<String, usize>>>,
    casts: Option<&HashMap<String, HashSet<String>>>,
    source_url: &str,
    cache_path: &str,
) -> Vec<CheckDetail> {
    let mut details = Vec::new();
    let has_position_tallies = has_position_tallies(tallies);
    let has_casts = casts.is_some_and(|c| !c.is_empty());
    let participation = participation_count(tallies);

    match (method, status) {
        (_, "no_quorum") => {
            if has_position_tallies {
                details.push(method_detail(
                    "vote.no_quorum_invariants",
                    result_id,
                    meeting_id,
                    "no_quorum result has yes/no/abstain tallies",
                    source_url,
                    cache_path,
                ));
            }
            if has_casts {
                details.push(method_detail(
                    "vote.no_quorum_invariants",
                    result_id,
                    meeting_id,
                    "no_quorum result has vote casts",
                    source_url,
                    cache_path,
                ));
            }
            if participation == 0 {
                details.push(method_detail(
                    "vote.no_quorum_invariants",
                    result_id,
                    meeting_id,
                    "no_quorum result missing participation tally",
                    source_url,
                    cache_path,
                ));
            }
        }
        ("sitting_standing", _) => {
            if outcome.trim().is_empty() || outcome == "unknown" {
                details.push(method_detail(
                    "vote.sitting_standing_invariants",
                    result_id,
                    meeting_id,
                    "sitting_standing result missing formal outcome",
                    source_url,
                    cache_path,
                ));
            }
            if has_position_tallies {
                details.push(method_detail(
                    "vote.sitting_standing_invariants",
                    result_id,
                    meeting_id,
                    "sitting_standing result has yes/no/abstain tallies",
                    source_url,
                    cache_path,
                ));
            }
            if has_casts {
                details.push(method_detail(
                    "vote.sitting_standing_invariants",
                    result_id,
                    meeting_id,
                    "sitting_standing result has vote casts",
                    source_url,
                    cache_path,
                ));
            }
        }
        ("secret_ballot", _) => {
            if has_casts {
                details.push(method_detail(
                    "vote.secret_ballot_invariants",
                    result_id,
                    meeting_id,
                    "secret_ballot result has named vote casts",
                    source_url,
                    cache_path,
                ));
            }
            if let Some(t) = tallies {
                let voters = statistic_value(t, "voters");
                let valid = statistic_value(t, "valid");
                let blank = statistic_value(t, "blank_invalid");
                if voters.is_none() || valid.is_none() {
                    details.push(method_detail(
                        "vote.secret_ballot_invariants",
                        result_id,
                        meeting_id,
                        "secret ballot missing voters or valid tally",
                        source_url,
                        cache_path,
                    ));
                } else if let (Some(voters), Some(valid)) = (voters, valid) {
                    let blank = blank.unwrap_or(0);
                    if voters != valid + blank {
                        details.push(
                            method_detail(
                                "vote.secret_ballot_invariants",
                                result_id,
                                meeting_id,
                                format!("voters={voters} != valid={valid} + blank={blank}"),
                                source_url,
                                cache_path,
                            )
                            .with_values(
                                format!("voters={voters}"),
                                format!("valid={valid} blank={blank}"),
                            ),
                        );
                    }
                }
            } else {
                details.push(method_detail(
                    "vote.secret_ballot_invariants",
                    result_id,
                    meeting_id,
                    "secret ballot has no statistics",
                    source_url,
                    cache_path,
                ));
            }
        }
        ("language_group_roll_call", _) => {
            if let Some(t) = tallies {
                for option in ["yes", "no", "abstain"] {
                    let overall = position_value(t, "overall", option);
                    let nl = position_value(t, "nl_group", option);
                    let fr = position_value(t, "fr_group", option);
                    if let (Some(overall), Some(nl), Some(fr)) = (overall, nl, fr) {
                        if nl + fr != overall {
                            details.push(
                                method_detail(
                                    "vote.language_group_sums",
                                    result_id,
                                    meeting_id,
                                    format!("{option} nl({nl}) + fr({fr}) != overall({overall})"),
                                    source_url,
                                    cache_path,
                                )
                                .with_values(format!("nl+fr={}", nl + fr), overall.to_string()),
                            );
                        }
                    } else {
                        details.push(method_detail(
                            "vote.language_group_sums",
                            result_id,
                            meeting_id,
                            format!("{option} missing overall, NL, or FR tally"),
                            source_url,
                            cache_path,
                        ));
                    }
                }
            } else {
                details.push(method_detail(
                    "vote.language_group_sums",
                    result_id,
                    meeting_id,
                    "language-group result has no tallies",
                    source_url,
                    cache_path,
                ));
            }
        }
        ("roll_call", _) => {
            let missing = ["yes", "no", "abstain"]
                .iter()
                .filter(|option| {
                    tallies
                        .and_then(|value| position_value(value, "overall", option))
                        .is_none()
                })
                .copied()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                details.push(method_detail(
                    "vote.standard_roll_call_invariants",
                    result_id,
                    meeting_id,
                    format!(
                        "standard roll call missing overall tallies: {}",
                        missing.join(",")
                    ),
                    source_url,
                    cache_path,
                ));
            }
        }
        _ => {}
    }

    if !matches!(method, "roll_call" | "language_group_roll_call") && has_casts {
        details.push(method_detail(
            "vote.cast_method_rules",
            result_id,
            meeting_id,
            format!("{method} result has named casts"),
            source_url,
            cache_path,
        ));
    }

    details
}

fn method_detail(
    check_id: &str,
    result_id: &str,
    meeting_id: &str,
    message: impl Into<String>,
    source_url: &str,
    cache_path: &str,
) -> CheckDetail {
    CheckDetail::new(check_id, "warn", "warn", message)
        .with_meeting("plenary", meeting_id)
        .with_entity("vote_result", result_id)
        .with_source(source_url, cache_path)
}

fn has_position_tallies(tallies: Option<&HashMap<String, HashMap<String, usize>>>) -> bool {
    tallies.is_some_and(|t| {
        t.values().any(|dimension| {
            ["yes", "no", "abstain"]
                .iter()
                .any(|option| dimension.contains_key(&format!("position:{option}")))
        })
    })
}

fn participation_count(tallies: Option<&HashMap<String, HashMap<String, usize>>>) -> usize {
    tallies
        .and_then(|t| t.get("overall"))
        .and_then(|d| d.get("participation:participated"))
        .copied()
        .unwrap_or(0)
}

fn statistic_value(tallies: &HashMap<String, HashMap<String, usize>>, key: &str) -> Option<usize> {
    tallies
        .get("overall")
        .and_then(|d| d.get(&format!("statistic:{key}")))
        .copied()
}

fn position_value(
    tallies: &HashMap<String, HashMap<String, usize>>,
    dimension: &str,
    option: &str,
) -> Option<usize> {
    tallies
        .get(dimension)
        .and_then(|d| d.get(&format!("position:{option}")))
        .copied()
}

fn check_vote_evidence(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let spans_path = data_dir.join(format!(
        "derived/sessions/{SESSION_ID}/plenary/source_spans.parquet"
    ));
    if !spans_path.exists() {
        return Ok(Vec::new());
    }
    let mut roles: HashMap<(String, String), HashSet<String>> = HashMap::new();
    for batch in read_all_rows(&spans_path)? {
        let entity_types = read_string_column(&batch, "entity_type")?;
        let entity_ids = read_string_column(&batch, "entity_id")?;
        let span_roles = read_string_column(&batch, "span_role")?;
        let statuses = read_string_column(&batch, "validation_status")?;
        for i in 0..batch.num_rows() {
            if statuses[i] == "valid" {
                roles
                    .entry((entity_types[i].clone(), entity_ids[i].clone()))
                    .or_default()
                    .insert(span_roles[i].clone());
            }
        }
    }

    let mut details = Vec::new();
    let votes_path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet"));
    if votes_path.exists() {
        for batch in read_all_rows(&votes_path)? {
            let vote_ids = read_string_column(&batch, "vote_id")?;
            let result_ids = read_string_column(&batch, "result_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let vote_roles = roles
                    .get(&("Vote".to_string(), vote_ids[i].clone()))
                    .cloned()
                    .unwrap_or_default();
                let result_exists =
                    roles.contains_key(&("VoteResult".to_string(), result_ids[i].clone()));
                if !vote_roles.contains("decision_title") || !result_exists {
                    details.push(
                        method_detail(
                            "vote.decision_evidence",
                            &vote_ids[i],
                            &meeting_ids[i],
                            format!(
                                "decision evidence missing: title={} result={}",
                                vote_roles.contains("decision_title"),
                                result_exists
                            ),
                            &source_urls[i],
                            &cache_paths[i],
                        )
                        .with_entity("vote", &vote_ids[i]),
                    );
                }
            }
        }
    }

    let mut tally_kinds: HashMap<String, Vec<(String, String, bool)>> = HashMap::new();
    let tallies_path = data_dir.join(format!(
        "sessions/{SESSION_ID}/plenary/vote_tallies.parquet"
    ));
    if tallies_path.exists() {
        for batch in read_all_rows(&tallies_path)? {
            let result_ids = read_string_column(&batch, "result_id")?;
            let kinds = read_string_column(&batch, "tally_kind")?;
            let options = read_string_column(&batch, "option_key")?;
            let selected = read_string_column(&batch, "selected")?;
            for i in 0..batch.num_rows() {
                tally_kinds.entry(result_ids[i].clone()).or_default().push((
                    kinds[i].clone(),
                    options[i].clone(),
                    selected[i] == "true",
                ));
            }
        }
    }

    let results_path = data_dir.join(format!(
        "sessions/{SESSION_ID}/plenary/vote_results.parquet"
    ));
    if results_path.exists() {
        for batch in read_all_rows(&results_path)? {
            let result_ids = read_string_column(&batch, "result_id")?;
            let methods = read_string_column(&batch, "method")?;
            let named = read_string_column(&batch, "named")?;
            let statuses = read_string_column(&batch, "status")?;
            let outcomes = read_string_column(&batch, "outcome")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let result_roles = roles
                    .get(&("VoteResult".to_string(), result_ids[i].clone()))
                    .cloned()
                    .unwrap_or_default();
                let mut required = match (methods[i].as_str(), statuses[i].as_str()) {
                    (_, "no_quorum") => {
                        vec![
                            "result_reference",
                            "quorum_statement",
                            "quorum_participation",
                        ]
                    }
                    ("language_group_roll_call", _) => {
                        vec!["result_table", "result_reference", "language_group_counts"]
                    }
                    ("roll_call", _) => {
                        vec!["result_table", "result_reference", "overall_counts"]
                    }
                    ("secret_ballot", _) => vec!["secret_statistics"],
                    ("sitting_standing", _) => vec!["formal_outcome"],
                    _ => Vec::new(),
                };
                if !outcomes[i].is_empty() && outcomes[i] != "unknown" {
                    required.push("formal_outcome");
                }
                if named[i] == "true"
                    && matches!(
                        methods[i].as_str(),
                        "roll_call" | "language_group_roll_call"
                    )
                {
                    required.extend([
                        "appendix_header",
                        "appendix_bucket_count",
                        "appendix_voter_names",
                    ]);
                }
                let tallies = tally_kinds
                    .get(&result_ids[i])
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if tallies.iter().any(|(kind, _, _)| kind == "candidate") {
                    required.push("candidate_tally");
                }
                if tallies
                    .iter()
                    .any(|(kind, _, selected)| kind == "candidate" && *selected)
                {
                    required.push("proclamation");
                }
                required.sort_unstable();
                required.dedup();
                let missing = required
                    .into_iter()
                    .filter(|role| !result_roles.contains(*role))
                    .collect::<Vec<_>>();
                if !missing.is_empty() {
                    details.push(method_detail(
                        "vote.result_evidence_roles",
                        &result_ids[i],
                        &meeting_ids[i],
                        format!("missing valid evidence roles: {}", missing.join(",")),
                        &source_urls[i],
                        &cache_paths[i],
                    ));
                }
            }
        }
    }
    Ok(details)
}

fn check_unresolved_vote_events(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join(format!(
        "sessions/{SESSION_ID}/plenary/vote_unresolved_events.parquet"
    ));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let kinds = read_string_column(&batch, "event_kind")?;
        let numbers = read_string_column(&batch, "source_roll_call_number")?;
        let reasons = read_string_column(&batch, "reason")?;
        let evidence = read_string_column(&batch, "evidence_text")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            details.push(
                CheckDetail::new(
                    "vote.unresolved_events",
                    "warn",
                    "warn",
                    format!(
                        "{} source {} unresolved: {}; evidence={}",
                        kinds[i], numbers[i], reasons[i], evidence[i]
                    ),
                )
                .with_meeting("plenary", &meeting_ids[i])
                .with_entity("vote_event", format!("{}:{}", kinds[i], numbers[i]))
                .with_source(&source_urls[i], &cache_paths[i]),
            );
        }
    }
    Ok(details)
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

fn check_appendix_bucket_counts(
    inv: &VoteInventory,
    source_url: &str,
    cache_path: &str,
) -> Vec<CheckDetail> {
    inv.appendix_buckets
        .iter()
        .filter(|bucket| bucket.declared_count as usize != bucket.collected_name_count)
        .map(|bucket| {
            CheckDetail::new(
                "vote.appendix_bucket_counts",
                "warn",
                "warn",
                format!(
                    "meeting {} source {} occurrence {} {} declares {} names but inventory counted {}",
                    inv.meeting_id,
                    bucket.vote_number,
                    bucket.occurrence,
                    bucket.position,
                    bucket.declared_count,
                    bucket.collected_name_count
                ),
            )
            .with_meeting("plenary", &inv.meeting_id)
            .with_entity(
                "vote_result",
                format!("{}#{}", bucket.vote_number, bucket.occurrence),
            )
            .with_values(
                bucket.declared_count.to_string(),
                bucket.collected_name_count.to_string(),
            )
            .with_source(source_url, cache_path)
        })
        .collect()
}

fn check_vote_sequence(
    inv: &VoteInventory,
    source_url: &str,
    cache_path: &str,
) -> Vec<CheckDetail> {
    let nums = inventory_vote_numbers(inv);
    let gaps = vote_number_gaps(&nums);
    gaps.into_iter()
        .map(|gap| {
            CheckDetail::new(
                "vote.number_sequence",
                "warn",
                "warn",
                format!(
                    "meeting {} missing vote number {gap} in sequence",
                    inv.meeting_id
                ),
            )
            .with_meeting("plenary", &inv.meeting_id)
            .with_entity("vote", &gap)
            .with_source(source_url, cache_path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally_map(values: &[(&str, &str, usize)]) -> HashMap<String, HashMap<String, usize>> {
        let mut out: HashMap<String, HashMap<String, usize>> = HashMap::new();
        for (dimension, key, count) in values {
            out.entry((*dimension).to_string())
                .or_default()
                .insert((*key).to_string(), *count);
        }
        out
    }

    #[test]
    fn explicit_zero_position_tallies_are_forbidden_for_no_quorum() {
        let tallies = tally_map(&[
            ("overall", "position:yes", 0),
            ("overall", "participation:participated", 65),
        ]);
        let details = check_method_invariants(
            "result",
            "roll_call",
            "no_quorum",
            "failed",
            "110",
            Some(&tallies),
            None,
            "",
            "",
        );
        assert!(
            details
                .iter()
                .any(|detail| detail.message.contains("yes/no/abstain"))
        );
    }

    #[test]
    fn language_group_sum_checks_zero_values() {
        let tallies = tally_map(&[
            ("overall", "position:abstain", 1),
            ("nl_group", "position:abstain", 0),
            ("fr_group", "position:abstain", 0),
        ]);
        let details = check_method_invariants(
            "result",
            "language_group_roll_call",
            "complete",
            "",
            "81",
            Some(&tallies),
            None,
            "",
            "",
        );
        assert!(
            details
                .iter()
                .any(|detail| detail.check_id == "vote.language_group_sums")
        );
    }

    #[test]
    fn secret_ballot_zero_blank_is_retained_in_equation() {
        let valid = tally_map(&[
            ("overall", "statistic:voters", 10),
            ("overall", "statistic:valid", 10),
            ("overall", "statistic:blank_invalid", 0),
        ]);
        assert!(
            check_method_invariants(
                "result",
                "secret_ballot",
                "complete",
                "",
                "16",
                Some(&valid),
                None,
                "",
                "",
            )
            .is_empty()
        );
    }
}
