//! Parse *mondelinge vragen schriftelijk behandeld* / *questions orales traitées par écrit*
//! sections from integraal verslag HTML (commission and plenary).

use crate::agenda_timeline::{build_agenda_timeline, AgendaItem, ItemKind, MeetingKind};
use crate::answer_io::AnswerDraft;
use crate::qrva_text::inline_answer_id;
use crate::report_blocks::{BlockTag, ReportBlock};
use crate::utils::clean_text;
use regex::Regex;
use scraper::Html;
use std::sync::OnceLock;

static ANSWER_DELIM: OnceLock<Regex> = OnceLock::new();

fn answer_delim_regex() -> &'static Regex {
    ANSWER_DELIM.get_or_init(|| {
        Regex::new(r"(?i)^Antwoord\s*[-–]\s*Réponse\s*:?\s*$").unwrap()
    })
}

pub fn is_written_oral_section_heading(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("schriftelijk behandelde mondelinge vragen")
        || lower.contains("questions orales traitées par écrit")
}

/// First block index inside the written oral Q&A zone (inclusive).
pub fn find_written_oral_zone_start(blocks: &[ReportBlock]) -> Option<u32> {
    blocks
        .iter()
        .find(|b| b.tag == BlockTag::H1 && is_written_oral_section_heading(&b.text))
        .map(|b| b.index)
}

fn paragraph_language(text: &str, block_lang: Option<&str>) -> &'static str {
    if let Some(lang) = block_lang {
        let u = lang.to_uppercase();
        if u.starts_with("FR") {
            return "fr";
        }
        if u.starts_with("NL") {
            return "nl";
        }
    }
    let lower = text.to_lowercase();
    if lower.contains("monsieur le ministre")
        || lower.contains("madame la")
        || lower.contains("question ")
        || lower.contains("réponse")
    {
        return "fr";
    }
    if lower.contains("geachte minister")
        || lower.contains("mevrouw de")
        || lower.contains("vraag ")
    {
        return "nl";
    }
    "nl"
}

fn append_lang_field(target_nl: &mut String, target_fr: &mut String, lang: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    let dest = if lang == "fr" { target_fr } else { target_nl };
    if !dest.is_empty() {
        dest.push('\n');
    }
    dest.push_str(text);
}

fn split_question_answer(paragraphs: &[(String, String)]) -> (String, String, String, String) {
    let mut q_nl = String::new();
    let mut q_fr = String::new();
    let mut a_nl = String::new();
    let mut a_fr = String::new();
    let mut in_answer = false;

    for (text, lang) in paragraphs {
        if !in_answer && answer_delim_regex().is_match(text.trim()) {
            in_answer = true;
            continue;
        }
        if in_answer {
            append_lang_field(&mut a_nl, &mut a_fr, lang, text);
        } else {
            append_lang_field(&mut q_nl, &mut q_fr, lang, text);
        }
    }

    (q_nl, q_fr, a_nl, a_fr)
}

fn collect_item_paragraphs(
    blocks: &[ReportBlock],
    item: &AgendaItem,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for block in blocks.iter() {
        if block.index < item.start_block || block.index >= item.end_block {
            continue;
        }
        if block.tag != BlockTag::P {
            continue;
        }
        let text = clean_text(&block.text);
        if text.is_empty() {
            continue;
        }
        let lang = paragraph_language(&text, block.lang.as_deref()).to_string();
        out.push((text, lang));
    }
    out
}

/// Extract written oral answer rows for questions in the written-treatment section.
pub fn extract_written_oral_answers(
    document: &Html,
    blocks: &[ReportBlock],
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
) -> Vec<AnswerDraft> {
    let Some(zone_start) = find_written_oral_zone_start(blocks) else {
        return Vec::new();
    };

    let timeline = build_agenda_timeline(document, blocks, meeting_kind, session_id, meeting_id);
    let mut answers = Vec::new();

    for item in timeline.iter().filter(|i| i.item_kind == ItemKind::Question) {
        if item.start_block < zone_start {
            continue;
        }
        let paragraphs = collect_item_paragraphs(blocks, item);
        if paragraphs.is_empty() {
            continue;
        }
        let (q_nl, q_fr, a_nl, a_fr) = split_question_answer(&paragraphs);
        if a_nl.is_empty() && a_fr.is_empty() {
            continue;
        }

        answers.push(AnswerDraft {
            answer_id: inline_answer_id(&item.item_id),
            question_id: item.item_id.clone(),
            route_id: String::new(),
            session_id,
            meeting_id: meeting_id.to_string(),
            meeting_kind: meeting_kind.as_str().to_string(),
            agenda_id: item.agenda_id.clone(),
            answer_slot: 1,
            kind: "oral_written".to_string(),
            text_nl: a_nl,
            text_fr: a_fr,
            question_body_nl: q_nl,
            question_body_fr: q_fr,
            status: String::new(),
            answer_num: String::new(),
            publication_ref: String::new(),
            casa: String::new(),
            source_kind: "integraal".to_string(),
            confidence: "parsed".to_string(),
            source_url: source_url.to_string(),
            cache_path: cache_path.to_string(),
        });
    }

    answers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report_blocks::{parse_report_blocks, read_report_html};

    #[test]
    fn detects_written_section_heading() {
        assert!(is_written_oral_section_heading(
            "Questions orales traitées par écrit"
        ));
        assert!(is_written_oral_section_heading(
            "Schriftelijk behandelde mondelinge vragen"
        ));
        assert!(!is_written_oral_section_heading("Mondelinge vragen"));
    }

    #[test]
    fn splits_on_answer_delimiter() {
        let paras = vec![
            ("Monsieur le Ministre,".to_string(), "fr".to_string()),
            ("1) Pouvez-vous…".to_string(), "fr".to_string()),
            ("Antwoord - Réponse:".to_string(), "nl".to_string()),
            ("Question 1".to_string(), "fr".to_string()),
            ("En tant que ministre…".to_string(), "fr".to_string()),
        ];
        let (q_nl, q_fr, a_nl, a_fr) = split_question_answer(&paras);
        assert!(q_fr.contains("Monsieur"));
        assert!(a_fr.contains("ministre"));
        assert!(q_nl.is_empty());
        assert!(a_nl.is_empty());
    }

    #[test]
    fn commission_407_written_section_fixture() {
        let candidates = [
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../cache/sessions/56/meetings/commission/56-407.html"),
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../partijgedrag/core/cache/sessions/56/meetings/commission/56-407.html"),
        ];
        let path = candidates.iter().find(|p| p.exists());
        let Some(path) = path else {
            return;
        };

        let html = read_report_html(path).unwrap();
        let document = Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        let answers = extract_written_oral_answers(
            &document,
            &blocks,
            MeetingKind::Commission,
            56,
            407,
            "https://example.test/ic407",
            "sessions/56/meetings/commission/56-407.html",
        );
        assert_eq!(answers.len(), 5, "expected 5 written answer blocks in 407");
        assert!(answers.iter().all(|a| !a.text_fr.is_empty() || !a.text_nl.is_empty()));
    }
}
