//! Independent vote inventory parser for QA crosschecks (S2–S4, A13).

use crate::report_blocks::{parse_report_blocks, read_report_html};
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct VoteInventory {
    pub meeting_id: String,
    pub compact_vote_numbers: BTreeSet<String>,
    pub appendix_vote_numbers: BTreeSet<String>,
    pub paragraph_vote_numbers: BTreeSet<String>,
    pub appendix_buckets: Vec<AppendixBucket>,
}

#[derive(Debug, Clone)]
pub struct AppendixBucket {
    pub vote_number: String,
    pub yes_count: usize,
    pub no_count: usize,
    pub abstain_count: usize,
    pub name_paragraph_count: usize,
}

static COMPACT_VOTE: OnceLock<Regex> = OnceLock::new();
static APPENDIX_VOTE: OnceLock<Regex> = OnceLock::new();
static PARAGRAPH_VOTE: OnceLock<Regex> = OnceLock::new();
static SELECTOR_P: OnceLock<Selector> = OnceLock::new();
static SELECTOR_SPAN: OnceLock<Selector> = OnceLock::new();

fn compact_vote_re() -> &'static Regex {
    COMPACT_VOTE.get_or_init(|| Regex::new(r"(?i)(?:Stemming|vote)\s*/\s*vote\s*(\d+)").unwrap())
}

fn appendix_vote_re() -> &'static Regex {
    APPENDIX_VOTE.get_or_init(|| {
        Regex::new(r"(?i)Naamstemming\s*-\s*Vote\s*nominatif\s*:\s*(\d+)").unwrap()
    })
}

fn paragraph_vote_re() -> &'static Regex {
    PARAGRAPH_VOTE.get_or_init(|| Regex::new(r"(?i)\(Stemming/vote\s+(\d+)\)").unwrap())
}

pub fn parse_vote_inventory(cache_path: &Path, meeting_id: &str) -> Result<VoteInventory, Box<dyn std::error::Error>> {
    let html = read_report_html(cache_path)?;
    let document = Html::parse_document(&html);
    let blocks = parse_report_blocks(&document);

    let mut compact_vote_numbers = BTreeSet::new();
    let mut appendix_vote_numbers = BTreeSet::new();
    let mut paragraph_vote_numbers = BTreeSet::new();

    for block in &blocks {
        if let Some(caps) = compact_vote_re().captures(&block.text) {
            compact_vote_numbers.insert(caps[1].to_string());
        }
        if let Some(caps) = appendix_vote_re().captures(&block.text) {
            appendix_vote_numbers.insert(caps[1].to_string());
        }
        if let Some(caps) = paragraph_vote_re().captures(&block.text) {
            paragraph_vote_numbers.insert(caps[1].to_string());
        }
    }

    for span in document.select(SELECTOR_SPAN.get_or_init(|| Selector::parse("span").unwrap())) {
        let text = span.text().collect::<String>();
        if let Some(caps) = appendix_vote_re().captures(&text) {
            appendix_vote_numbers.insert(caps[1].to_string());
        }
    }

    let appendix_buckets = parse_appendix_buckets(&html);

    Ok(VoteInventory {
        meeting_id: meeting_id.to_string(),
        compact_vote_numbers,
        appendix_vote_numbers,
        paragraph_vote_numbers,
        appendix_buckets,
    })
}

fn parse_appendix_buckets(html: &str) -> Vec<AppendixBucket> {
    let mut buckets = Vec::new();
    for caps in appendix_vote_re().captures_iter(html) {
        let vote_number = caps[1].to_string();
        let name_paragraph_count = count_name_paragraphs_after(html, &vote_number);
        buckets.push(AppendixBucket {
            vote_number,
            yes_count: 0,
            no_count: 0,
            abstain_count: 0,
            name_paragraph_count,
        });
    }
    buckets
}

fn count_name_paragraphs_after(html: &str, vote_number: &str) -> usize {
    let marker = format!("Vote nominatif: {vote_number}");
    let Some(pos) = html.find(&marker) else {
        return 0;
    };
    let tail: String = html[pos..].chars().take(8000).collect();
    let mut count = 0usize;
    for p in Html::parse_fragment(&tail).select(SELECTOR_P.get_or_init(|| Selector::parse("p").unwrap())) {
        let text = p.text().collect::<String>();
        let trimmed = text.trim();
        if trimmed.len() > 3 && trimmed.chars().any(|c| c.is_alphabetic()) {
            count += 1;
        }
    }
    count
}

pub fn inventory_vote_numbers(inv: &VoteInventory) -> BTreeSet<String> {
    let mut all = inv.compact_vote_numbers.clone();
    all.extend(inv.appendix_vote_numbers.iter().cloned());
    all.extend(inv.paragraph_vote_numbers.iter().cloned());
    all
}

pub fn vote_number_gaps(numbers: &BTreeSet<String>) -> Vec<String> {
    if numbers.is_empty() {
        return Vec::new();
    }
    let min = numbers.iter().filter_map(|n| n.parse::<u32>().ok()).min().unwrap_or(1);
    let max = numbers.iter().filter_map(|n| n.parse::<u32>().ok()).max().unwrap_or(min);
    let mut gaps = Vec::new();
    for n in min..=max {
        if !numbers.contains(&n.to_string()) {
            gaps.push(n.to_string());
        }
    }
    gaps
}

pub fn votes_by_meeting_from_parquet(
    votes: &[(String, String, String, String, String, String)],
) -> HashMap<String, Vec<&(String, String, String, String, String, String)>> {
    let mut map: HashMap<String, Vec<&(String, String, String, String, String, String)>> = HashMap::new();
    for row in votes {
        map.entry(row.1.clone()).or_default().push(row);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::cache_dir;

    #[test]
    fn parse_vote_inventory_meeting_129() {
        let path = cache_dir().join("sessions/56/meetings/plenary/56-129.html");
        if !path.exists() {
            return;
        }
        let inv = parse_vote_inventory(&path, "129").expect("inventory");
        assert!(!inv.appendix_vote_numbers.is_empty());
    }
}
