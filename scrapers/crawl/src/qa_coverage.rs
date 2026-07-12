//! Whole-document word counts for meeting report coverage QA.

use crate::report_blocks::{parse_report_blocks, read_report_html, ReportBlock};
use scraper::Html;
use std::path::Path;

/// Visible text word count across all report blocks (h1, h2, p, table).
pub fn count_document_words(blocks: &[ReportBlock]) -> usize {
    blocks.iter().map(|b| word_count(&b.text)).sum()
}

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().filter(|token| !token.is_empty()).count()
}

pub fn count_document_words_from_cache(
    cache_path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let html = read_report_html(cache_path)?;
    let document = Html::parse_document(&html);
    let blocks = parse_report_blocks(&document);
    Ok(count_document_words(&blocks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agenda_timeline::MeetingKind;
    use crate::meeting_report::extract_utterances_from_document;

    fn fixture_cache_root() -> Option<std::path::PathBuf> {
        let candidates = [
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../partijgedrag/core/cache/sessions/56/meetings"),
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../cache/sessions/56/meetings"),
        ];
        candidates.into_iter().find(|p| p.exists())
    }

    fn load_fixture(kind: &str, meeting_id: u32) -> Option<(Vec<ReportBlock>, Html)> {
        let root = fixture_cache_root()?;
        let path = root.join(kind).join(format!("56-{meeting_id}.html"));
        if !path.exists() {
            return None;
        }
        let html = read_report_html(&path).ok()?;
        let document = Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        Some((blocks, document))
    }

    #[test]
    fn document_words_positive_on_fixtures() {
        for (kind, meeting_id) in [("plenary", 117u32), ("commission", 17)] {
            let Some((blocks, _)) = load_fixture(kind, meeting_id) else {
                continue;
            };
            let words = count_document_words(&blocks);
            assert!(words > 0, "{kind} {meeting_id}: expected document words > 0");
        }
    }

    #[test]
    fn utterance_words_do_not_exceed_whole_document() {
        for (kind, meeting_id) in [
            ("plenary", 117u32),
            ("plenary", 19),
            ("commission", 17),
            ("commission", 57),
        ] {
            let Some((blocks, document)) = load_fixture(kind, meeting_id) else {
                continue;
            };
            let meeting_kind = if kind == "plenary" {
                MeetingKind::Plenary
            } else {
                MeetingKind::Commission
            };
            let source_words = count_document_words(&blocks);
            let utterances = extract_utterances_from_document(
                &document,
                meeting_kind,
                56,
                meeting_id,
                "fixture",
                "cache",
            );
            let utterance_words: usize = utterances
                .iter()
                .map(|u| word_count(&u.text) + word_count(&u.raw_speaker))
                .sum();
            assert!(
                utterance_words <= source_words,
                "{kind} {meeting_id}: utterance words {utterance_words} > document words {source_words}"
            );
        }
    }
}
