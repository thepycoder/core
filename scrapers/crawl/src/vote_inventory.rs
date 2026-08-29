//! Independent, method-aware source inventory used only by QA.
//!
//! This deliberately does not call the production vote event scanner or assembler.

use crate::report_blocks::{BlockTag, ReportBlock, parse_report_blocks, read_report_html};
use crate::vote_patterns::{
    VoteBucket, VoteSectionKind, formal_outcome, formal_vote_begin_re, is_language_group_header,
    quorum_failure_re, reuse_result_re, vote_bucket_label, votes_section_heading,
};
use scraper::Html;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormalVoteOccurrence {
    pub source_number: String,
    pub occurrence: u32,
    pub method: String,
    pub block_index: u32,
    pub creates_result: bool,
}

#[derive(Debug, Clone)]
pub struct VoteInventory {
    pub meeting_id: String,
    pub compact_vote_numbers: BTreeSet<String>,
    pub appendix_vote_numbers: BTreeSet<String>,
    pub paragraph_vote_numbers: BTreeSet<String>,
    pub formal_events: Vec<FormalVoteOccurrence>,
    pub appendix_buckets: Vec<AppendixBucket>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendixBucket {
    pub vote_number: String,
    pub occurrence: u32,
    pub position: String,
    pub declared_count: u32,
    pub collected_name_count: usize,
}

pub fn parse_compact_vote_number(text: &str) -> Option<String> {
    crate::vote_patterns::parse_compact_vote_number(text)
}

pub fn parse_appendix_vote_number(text: &str) -> Option<String> {
    crate::vote_patterns::parse_appendix_vote_number(text)
}

pub fn parse_paragraph_vote_number(text: &str) -> Option<String> {
    crate::vote_patterns::parse_paragraph_vote_number(text)
}

pub fn appendix_marker_for_vote(text: &str, vote_index: &str) -> bool {
    parse_appendix_vote_number(text).as_deref() == Some(vote_index)
}

pub fn parse_vote_inventory(
    cache_path: &Path,
    meeting_id: &str,
) -> Result<VoteInventory, Box<dyn std::error::Error>> {
    let html = read_report_html(cache_path)?;
    let document = Html::parse_document(&html);
    Ok(parse_vote_inventory_blocks(
        &parse_report_blocks(&document),
        meeting_id,
    ))
}

fn parse_vote_inventory_blocks(blocks: &[ReportBlock], meeting_id: &str) -> VoteInventory {
    let mut compact_vote_numbers = BTreeSet::new();
    let mut appendix_vote_numbers = BTreeSet::new();
    let mut paragraph_vote_numbers = BTreeSet::new();
    let mut formal_events = Vec::new();
    let mut source_occurrences: HashMap<String, u32> = HashMap::new();
    let mut section = VoteSectionKind::None;
    let mut formal_zone = false;

    for (idx, block) in blocks.iter().enumerate() {
        let heading = votes_section_heading(&block.text);
        if heading != VoteSectionKind::None {
            section = heading;
        } else if block.tag == BlockTag::H1 {
            section = VoteSectionKind::None;
            formal_zone = false;
        }
        if formal_vote_begin_re().is_match(&block.text) {
            formal_zone = true;
        }

        if let Some(number) = parse_appendix_vote_number(&block.text) {
            appendix_vote_numbers.insert(number);
            continue;
        }

        if block.tag == BlockTag::Table {
            if (section == VoteSectionKind::RollCall || formal_zone)
                && let Some(number) = table_source_number(block)
            {
                compact_vote_numbers.insert(number.clone());
                let method = if table_is_language_group(block) {
                    "language_group_roll_call"
                } else {
                    "roll_call"
                };
                push_event(
                    &mut formal_events,
                    &mut source_occurrences,
                    number,
                    method,
                    block.index,
                    true,
                );
                if section != VoteSectionKind::RollCall {
                    formal_zone = false;
                }
                continue;
            }
            if section == VoteSectionKind::SecretBallot && table_is_secret_result(block) {
                push_event(
                    &mut formal_events,
                    &mut source_occurrences,
                    String::new(),
                    "secret_ballot",
                    block.index,
                    true,
                );
                continue;
            }
        }

        if block.tag != BlockTag::P {
            continue;
        }
        if let Some(number) = parse_paragraph_vote_number(&block.text) {
            paragraph_vote_numbers.insert(number.clone());
            let nearby = blocks[idx..blocks.len().min(idx + 4)]
                .iter()
                .map(|candidate| candidate.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let previous = blocks[idx.saturating_sub(3)..idx]
                .iter()
                .map(|candidate| candidate.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if reuse_result_re().is_match(&previous) || reuse_result_re().is_match(&nearby) {
                push_event(
                    &mut formal_events,
                    &mut source_occurrences,
                    number,
                    "reuse",
                    block.index,
                    false,
                );
            } else if (section == VoteSectionKind::RollCall || formal_zone)
                && quorum_failure_re().is_match(&nearby)
            {
                push_event(
                    &mut formal_events,
                    &mut source_occurrences,
                    number,
                    "no_quorum",
                    block.index,
                    true,
                );
                if section != VoteSectionKind::RollCall {
                    formal_zone = false;
                }
            }
        }
        let lower = block.text.to_lowercase();
        if (lower.contains("zitten en opstaan") || lower.contains("assis et levé"))
            && formal_outcome(&block.text).is_some()
        {
            let bilingual_duplicate =
                formal_events
                    .last()
                    .is_some_and(|event: &FormalVoteOccurrence| {
                        event.method == "sitting_standing" && event.block_index + 1 == block.index
                    });
            if !bilingual_duplicate {
                push_event(
                    &mut formal_events,
                    &mut source_occurrences,
                    String::new(),
                    "sitting_standing",
                    block.index,
                    true,
                );
            }
        }
    }

    VoteInventory {
        meeting_id: meeting_id.to_string(),
        compact_vote_numbers,
        appendix_vote_numbers,
        paragraph_vote_numbers,
        formal_events,
        appendix_buckets: parse_appendix_buckets(blocks),
    }
}

fn push_event(
    events: &mut Vec<FormalVoteOccurrence>,
    occurrences: &mut HashMap<String, u32>,
    source_number: String,
    method: &str,
    block_index: u32,
    creates_result: bool,
) {
    let key = if !creates_result {
        format!("@reuse:{source_number}")
    } else if source_number.is_empty() {
        format!("@{method}")
    } else {
        source_number.clone()
    };
    let occurrence = occurrences.entry(key).or_default();
    *occurrence += 1;
    events.push(FormalVoteOccurrence {
        source_number,
        occurrence: *occurrence,
        method: method.to_string(),
        block_index,
        creates_result,
    });
}

fn table_source_number(block: &ReportBlock) -> Option<String> {
    let first = block.table_rows.as_ref()?.first()?;
    parse_compact_vote_number(
        &first
            .cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn table_is_language_group(block: &ReportBlock) -> bool {
    block
        .table_rows
        .as_ref()
        .and_then(|rows| rows.get(1))
        .is_some_and(|row| {
            let cells = row
                .cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<Vec<_>>();
            is_language_group_header(&cells)
        })
}

fn table_is_secret_result(block: &ReportBlock) -> bool {
    block.table_rows.as_ref().is_some_and(|rows| {
        rows.iter().any(|row| {
            row.cells
                .first()
                .and_then(|cell| vote_bucket_label(&cell.text))
                .is_some_and(|bucket| {
                    matches!(
                        bucket,
                        VoteBucket::Voters
                            | VoteBucket::Valid
                            | VoteBucket::BlankInvalid
                            | VoteBucket::MajorityThreshold
                    )
                })
        })
    })
}

fn parse_appendix_buckets(blocks: &[ReportBlock]) -> Vec<AppendixBucket> {
    let mut buckets = Vec::new();
    let mut occurrences: HashMap<String, u32> = HashMap::new();
    for (marker_idx, marker) in blocks.iter().enumerate() {
        let Some(vote_number) = parse_appendix_vote_number(&marker.text) else {
            continue;
        };
        let occurrence = occurrences.entry(vote_number.clone()).or_default();
        *occurrence += 1;
        let end = blocks[marker_idx + 1..]
            .iter()
            .position(|block| {
                parse_appendix_vote_number(&block.text).is_some()
                    || crate::vote_patterns::parse_electronic_count_number(&block.text).is_some()
            })
            .map_or(blocks.len(), |offset| marker_idx + 1 + offset);
        let mut current_bucket: Option<usize> = None;
        for block in &blocks[marker_idx + 1..end] {
            if let Some(rows) = &block.table_rows {
                for row in rows {
                    let Some(label) = row.cells.first() else {
                        continue;
                    };
                    let Some(position) = bucket_position(&label.text) else {
                        continue;
                    };
                    let declared_count = row
                        .cells
                        .iter()
                        .skip(1)
                        .find_map(|cell| cell.text.trim().parse::<u32>().ok())
                        .unwrap_or(0);
                    buckets.push(AppendixBucket {
                        vote_number: vote_number.clone(),
                        occurrence: *occurrence,
                        position: position.to_string(),
                        declared_count,
                        collected_name_count: 0,
                    });
                    current_bucket = Some(buckets.len() - 1);
                }
            } else if block.tag == BlockTag::P
                && let Some(bucket_idx) = current_bucket
            {
                buckets[bucket_idx].collected_name_count += count_names(&block.text);
            }
        }
    }
    buckets
}

fn bucket_position(label: &str) -> Option<&'static str> {
    match vote_bucket_label(label)? {
        VoteBucket::Yes => Some("yes"),
        VoteBucket::No => Some("no"),
        VoteBucket::Abstain => Some("abstain"),
        _ => None,
    }
}

fn count_names(text: &str) -> usize {
    text.split([',', ';'])
        .filter(|part| part.trim().chars().any(char::is_alphabetic))
        .count()
}

pub fn inventory_vote_numbers(inv: &VoteInventory) -> BTreeSet<String> {
    let mut all = inv.compact_vote_numbers.clone();
    all.extend(inv.appendix_vote_numbers.iter().cloned());
    all.extend(inv.paragraph_vote_numbers.iter().cloned());
    all
}

pub fn numeric_sequence_gaps_from_one(numbers: &BTreeSet<u32>) -> Vec<u32> {
    let Some(max) = numbers.iter().max().copied() else {
        return Vec::new();
    };
    if max == 0 {
        return Vec::new();
    }
    (1..=max)
        .filter(|number| !numbers.contains(number))
        .collect()
}

pub fn vote_number_gaps(numbers: &BTreeSet<String>) -> Vec<String> {
    let parsed: BTreeSet<u32> = numbers
        .iter()
        .filter_map(|number| number.parse::<u32>().ok())
        .collect();
    let Some(min) = parsed.iter().min().copied() else {
        return Vec::new();
    };
    let max = parsed.iter().max().copied().unwrap_or(min);
    (min..=max)
        .filter(|number| !parsed.contains(number))
        .map(|number| number.to_string())
        .collect()
}

type VoteRow<'a> = &'a (String, String, String, String, String, String);

pub fn votes_by_meeting_from_parquet(
    votes: &[(String, String, String, String, String, String)],
) -> HashMap<String, Vec<VoteRow<'_>>> {
    let mut map = HashMap::new();
    for row in votes {
        map.entry(row.1.clone()).or_insert_with(Vec::new).push(row);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> VoteInventory {
        let html = include_str!(concat!(
            "../tests/fixtures/votes/",
            "roll_call_compact.html"
        ));
        let source = match name {
            "language" => include_str!("../tests/fixtures/votes/language_group_roll_call.html"),
            "secret" => include_str!("../tests/fixtures/votes/secret_ballot_candidates.html"),
            "quorum" => include_str!("../tests/fixtures/votes/quorum_failure_formal_sequence.html"),
            "reuse" => include_str!("../tests/fixtures/votes/result_reuse.html"),
            "sitting" => include_str!("../tests/fixtures/votes/sitting_standing.html"),
            "appendix" => include_str!("../tests/fixtures/votes/appendix_reverse_order.html"),
            "debate" => include_str!("../tests/fixtures/votes/debate_quoted_table.html"),
            "electronic" => {
                include_str!("../tests/fixtures/votes/electronic_count_between_appendix.html")
            }
            _ => html,
        };
        let document = Html::parse_document(source);
        parse_vote_inventory_blocks(&parse_report_blocks(&document), "1")
    }

    #[test]
    fn inventories_all_formal_methods_and_reuse() {
        assert_eq!(
            fixture("language").formal_events[0].method,
            "language_group_roll_call"
        );
        assert_eq!(fixture("secret").formal_events[0].method, "secret_ballot");
        assert!(
            fixture("quorum")
                .formal_events
                .iter()
                .all(|event| event.method == "no_quorum")
        );
        assert!(
            fixture("reuse")
                .formal_events
                .iter()
                .any(|event| !event.creates_result)
        );
        assert_eq!(
            fixture("sitting").formal_events[0].method,
            "sitting_standing"
        );
    }

    #[test]
    fn appendix_keeps_occurrence_and_real_declared_count() {
        let inventory = fixture("appendix");
        assert_eq!(inventory.appendix_buckets[0].vote_number, "1");
        assert_eq!(inventory.appendix_buckets[0].occurrence, 1);
        assert_eq!(inventory.appendix_buckets[0].declared_count, 61);
        assert_eq!(inventory.appendix_buckets[0].collected_name_count, 2);
    }

    #[test]
    fn electronic_count_header_ends_inventory_appendix_slice() {
        // meeting 12: do not attribute electronic-count Oui/128 or header to naamstemming 1
        let inventory = fixture("electronic");
        let vote1: Vec<_> = inventory
            .appendix_buckets
            .iter()
            .filter(|b| b.vote_number == "1")
            .collect();
        assert_eq!(vote1.len(), 3);
        assert_eq!(vote1[0].position, "yes");
        assert_eq!(vote1[0].declared_count, 64);
        assert_eq!(vote1[0].collected_name_count, 2);
        assert_eq!(vote1[2].position, "abstain");
        assert_eq!(vote1[2].declared_count, 0);
        assert_eq!(vote1[2].collected_name_count, 0);
        assert!(
            !vote1.iter().any(|b| b.declared_count == 128),
            "electronic-count Oui must not leak into vote 1 inventory buckets"
        );
        let vote3: Vec<_> = inventory
            .appendix_buckets
            .iter()
            .filter(|b| b.vote_number == "3")
            .collect();
        assert_eq!(vote3.len(), 1);
        assert_eq!(vote3[0].declared_count, 2);
        assert_eq!(vote3[0].collected_name_count, 2);
    }

    #[test]
    fn ordered_repeated_occurrences_are_retained() {
        let inventory = fixture("quorum");
        assert_eq!(inventory.formal_events.len(), 2);
        assert_eq!(inventory.formal_events[0].source_number, "1");
        assert_eq!(inventory.formal_events[1].source_number, "2");
    }

    #[test]
    fn debate_vote_shaped_table_is_not_a_formal_event() {
        assert!(fixture("debate").formal_events.is_empty());
    }

    #[test]
    fn numeric_sequence_gaps_from_one_flags_missing_low_numbers() {
        use std::collections::BTreeSet;

        let numbers = BTreeSet::from([1, 2, 4, 10]);
        assert_eq!(
            numeric_sequence_gaps_from_one(&numbers),
            vec![3, 5, 6, 7, 8, 9]
        );
    }

    #[test]
    fn vote_number_gaps_uses_parsed_values() {
        use std::collections::BTreeSet;

        let numbers = BTreeSet::from(["01".into(), "03".into(), "04".into()]);
        assert_eq!(vote_number_gaps(&numbers), vec!["2"]);
    }
}
