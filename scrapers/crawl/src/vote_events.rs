//! Block-native vote event detection helpers.

use crate::report_blocks::{ReportBlock, TableRow};
use crate::vote_patterns::{
    VoteBucket, is_language_group_header, parse_appendix_vote_number, parse_compact_vote_number,
    parse_electronic_count_number, parse_paragraph_vote_number, vote_bucket_label,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollCallTableShape {
    Standard,
    LanguageGroup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollCallCounts {
    pub yes: Option<u32>,
    pub no: Option<u32>,
    pub abstain: Option<u32>,
    pub yes_nl: Option<u32>,
    pub yes_fr: Option<u32>,
    pub no_nl: Option<u32>,
    pub no_fr: Option<u32>,
    pub abstain_nl: Option<u32>,
    pub abstain_fr: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRollCallTable {
    pub source_number: String,
    pub shape: RollCallTableShape,
    pub counts: RollCallCounts,
    pub invalid_language_group_buckets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSecretTable {
    pub voters: Option<u32>,
    pub valid: Option<u32>,
    pub blank_invalid: Option<u32>,
    pub majority_threshold: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendixBucket {
    pub position: String,
    pub count: u32,
    pub names: Vec<String>,
    pub count_block: u32,
    pub name_blocks: Vec<u32>,
}

pub fn parse_roll_call_table(block: &ReportBlock) -> Option<ParsedRollCallTable> {
    let rows = block.table_rows.as_ref()?;
    if rows.is_empty() {
        return None;
    }
    let source_number = parse_compact_vote_number(
        &rows[0]
            .cells
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    )?;
    let shape = if rows.len() > 1 {
        let labels: Vec<_> = rows[1].cells.iter().map(|c| c.text.as_str()).collect();
        if is_language_group_header(&labels) {
            RollCallTableShape::LanguageGroup
        } else {
            RollCallTableShape::Standard
        }
    } else {
        RollCallTableShape::Standard
    };

    let mut counts = RollCallCounts {
        yes: None,
        no: None,
        abstain: None,
        yes_nl: None,
        yes_fr: None,
        no_nl: None,
        no_fr: None,
        abstain_nl: None,
        abstain_fr: None,
    };

    let mut invalid_language_group_buckets = Vec::new();
    for row in rows.iter().skip(1) {
        if row.cells.is_empty() {
            continue;
        }
        let label = row.cells[0].text.as_str();
        if vote_bucket_label(label) == Some(VoteBucket::Total) {
            continue;
        }
        let Some(bucket) = vote_bucket_label(label) else {
            continue;
        };
        match shape {
            RollCallTableShape::Standard => {
                let Some(value) = row.cells.get(1).and_then(|c| c.text.trim().parse().ok()) else {
                    continue;
                };
                apply_standard_bucket(&mut counts, bucket, value);
            }
            RollCallTableShape::LanguageGroup => {
                let Some(nl) = row.cells.get(1).and_then(|c| c.text.trim().parse().ok()) else {
                    continue;
                };
                let Some(total) = row.cells.get(2).and_then(|c| c.text.trim().parse().ok()) else {
                    continue;
                };
                let Some(fr) = row.cells.get(3).and_then(|c| c.text.trim().parse().ok()) else {
                    continue;
                };
                if nl + fr != total {
                    invalid_language_group_buckets.push(bucket_key(bucket).to_string());
                    continue;
                }
                apply_language_group_bucket(&mut counts, bucket, nl, total, fr);
            }
        }
    }

    Some(ParsedRollCallTable {
        source_number,
        shape,
        counts,
        invalid_language_group_buckets,
    })
}

fn apply_standard_bucket(counts: &mut RollCallCounts, bucket: VoteBucket, value: u32) {
    match bucket {
        VoteBucket::Yes => counts.yes = Some(value),
        VoteBucket::No => counts.no = Some(value),
        VoteBucket::Abstain => counts.abstain = Some(value),
        _ => {}
    }
}

fn apply_language_group_bucket(
    counts: &mut RollCallCounts,
    bucket: VoteBucket,
    nl: u32,
    total: u32,
    fr: u32,
) {
    match bucket {
        VoteBucket::Yes => {
            counts.yes = Some(total);
            counts.yes_nl = Some(nl);
            counts.yes_fr = Some(fr);
        }
        VoteBucket::No => {
            counts.no = Some(total);
            counts.no_nl = Some(nl);
            counts.no_fr = Some(fr);
        }
        VoteBucket::Abstain => {
            counts.abstain = Some(total);
            counts.abstain_nl = Some(nl);
            counts.abstain_fr = Some(fr);
        }
        _ => {}
    }
}

fn bucket_key(bucket: VoteBucket) -> &'static str {
    match bucket {
        VoteBucket::Yes => "yes",
        VoteBucket::No => "no",
        VoteBucket::Abstain => "abstain",
        _ => "unknown",
    }
}

pub fn parse_secret_ballot_table(block: &ReportBlock) -> ParsedSecretTable {
    let mut parsed = ParsedSecretTable {
        voters: None,
        valid: None,
        blank_invalid: None,
        majority_threshold: None,
    };
    let Some(rows) = block.table_rows.as_ref() else {
        return parsed;
    };
    for row in rows {
        if row.cells.is_empty() {
            continue;
        }
        let label = row.cells[0].text.as_str();
        let value = row.cells.get(1).and_then(|c| c.text.trim().parse().ok());
        match vote_bucket_label(label) {
            Some(VoteBucket::Voters) => parsed.voters = value,
            Some(VoteBucket::Valid) => parsed.valid = value,
            Some(VoteBucket::BlankInvalid) => parsed.blank_invalid = value,
            Some(VoteBucket::MajorityThreshold) => parsed.majority_threshold = value,
            _ => {}
        }
    }
    parsed
}

pub fn parse_appendix_bucket_row(row: &TableRow) -> Option<(String, u32)> {
    if row.cells.len() < 2 {
        return None;
    }
    let label = row.cells[0].text.as_str();
    let bucket = vote_bucket_label(label)?;
    let position = match bucket {
        VoteBucket::Yes => "yes",
        VoteBucket::No => "no",
        VoteBucket::Abstain => "abstain",
        _ => return None,
    };
    let count = row.cells[1].text.trim().parse().ok()?;
    Some((position.to_string(), count))
}

pub fn block_source_number(block: &ReportBlock) -> Option<String> {
    parse_compact_vote_number(&block.text)
        .or_else(|| parse_paragraph_vote_number(&block.text))
        .or_else(|| parse_appendix_vote_number(&block.text))
}

pub fn parse_appendix_from_blocks(
    blocks: &[ReportBlock],
    start: usize,
    _vote_number: &str,
) -> Vec<AppendixBucket> {
    let mut buckets = Vec::new();
    let mut idx = start + 1;
    while idx < blocks.len() {
        let block = &blocks[idx];
        if block.tag == crate::report_blocks::BlockTag::H1 {
            break;
        }
        if parse_appendix_vote_number(&block.text).is_some() {
            break;
        }
        if parse_electronic_count_number(&block.text).is_some() {
            break;
        }
        if let Some(rows) = block.table_rows.as_ref() {
            for row in rows {
                if let Some((position, count)) = parse_appendix_bucket_row(row) {
                    let mut names = Vec::new();
                    let mut name_blocks = Vec::new();
                    let mut name_idx = idx + 1;
                    while name_idx < blocks.len() {
                        let name_block = &blocks[name_idx];
                        if name_block.tag == crate::report_blocks::BlockTag::Table
                            || parse_appendix_vote_number(&name_block.text).is_some()
                            || parse_electronic_count_number(&name_block.text).is_some()
                        {
                            break;
                        }
                        if name_block.tag == crate::report_blocks::BlockTag::P
                            && crate::vote_patterns::looks_like_voter_names(&name_block.text)
                        {
                            name_blocks.push(name_block.index);
                            names.extend(
                                name_block
                                    .text
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty()),
                            );
                            name_idx += 1;
                        } else {
                            break;
                        }
                    }
                    buckets.push(AppendixBucket {
                        position,
                        count,
                        names,
                        count_block: block.index,
                        name_blocks,
                    });
                    idx = name_idx.saturating_sub(1);
                }
            }
        }
        idx += 1;
    }
    buckets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report_blocks::{parse_report_blocks, read_report_html};
    use scraper::Html;
    use std::path::Path;

    fn blocks(name: &str) -> Vec<ReportBlock> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/votes")
            .join(name);
        let html = read_report_html(&path).unwrap();
        parse_report_blocks(&Html::parse_document(&html))
    }

    #[test]
    fn parses_standard_roll_call_table() {
        let blocks = blocks("roll_call_compact.html");
        let table = blocks
            .iter()
            .find(|b| b.text.contains("Stemming/vote 1") && b.table_rows.is_some())
            .unwrap();
        let parsed = parse_roll_call_table(table).unwrap();
        assert_eq!(parsed.source_number, "1");
        assert_eq!(parsed.shape, RollCallTableShape::Standard);
        assert_eq!(parsed.counts.yes, Some(74));
        assert_eq!(parsed.counts.no, Some(19));
    }

    #[test]
    fn parses_language_group_table() {
        let blocks = blocks("language_group_roll_call.html");
        let table = blocks
            .iter()
            .find(|b| b.text.contains("Stemming/vote 8"))
            .unwrap();
        let parsed = parse_roll_call_table(table).unwrap();
        assert_eq!(parsed.shape, RollCallTableShape::LanguageGroup);
        assert_eq!(parsed.counts.no, Some(119));
        assert_eq!(parsed.counts.no_nl, Some(67));
        assert_eq!(parsed.counts.no_fr, Some(52));
    }

    #[test]
    fn parses_secret_ballot_fixture() {
        let blocks = blocks("secret_ballot_aggregate.html");
        let table = blocks.iter().find(|b| b.text.contains("Stemmen")).unwrap();
        let parsed = parse_secret_ballot_table(table);
        assert_eq!(parsed.voters, Some(82));
        assert_eq!(parsed.valid, Some(82));
        assert_eq!(parsed.majority_threshold, Some(42));
    }

    #[test]
    fn electronic_count_header_ends_appendix_before_leaking_names_or_buckets() {
        // meeting 12: abstentions 0, then electronic count 2, then naamstemming 3
        let blocks = blocks("electronic_count_between_appendix.html");
        let marker_idx = blocks
            .iter()
            .position(|b| b.text.contains("Naamstemming: 1"))
            .unwrap();
        let buckets = parse_appendix_from_blocks(&blocks, marker_idx, "1");
        assert_eq!(buckets.len(), 3);
        assert_eq!(buckets[0].position, "yes");
        assert_eq!(buckets[0].count, 64);
        assert_eq!(buckets[0].names.len(), 2);
        assert_eq!(buckets[1].position, "no");
        assert_eq!(buckets[1].count, 63);
        assert_eq!(buckets[2].position, "abstain");
        assert_eq!(buckets[2].count, 0);
        assert!(
            buckets[2].names.is_empty(),
            "electronic-count header must not become abstain names: {:?}",
            buckets[2].names
        );
        assert!(
            !buckets
                .iter()
                .any(|b| b.position == "yes" && b.count == 128),
            "electronic-count Oui table must not attach to prior naamstemming"
        );
    }
}
