use crate::proceeding_entities::classify_heading_kind;
use crate::report_blocks::{BlockTag, ReportBlock};
use crate::utils::{clean_text, composite_scoped_id};
use regex::Regex;
use scraper::{Html, Selector};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingKind {
    Plenary,
    Commission,
}

impl MeetingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MeetingKind::Plenary => "plenary",
            MeetingKind::Commission => "commission",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Question,
    Hearing,
    Interpellation,
    GeneralDebate,
    Proposition,
    Notice,
    Vote,
    VoteExplanation,
    Opening,
    Closing,
    Procedural,
    Unknown,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Question => "question",
            ItemKind::Hearing => "hearing",
            ItemKind::Interpellation => "interpellation",
            ItemKind::GeneralDebate => "general_debate",
            ItemKind::Proposition => "proposition",
            ItemKind::Notice => "notice",
            ItemKind::Vote => "vote",
            ItemKind::VoteExplanation => "vote_explanation",
            ItemKind::Opening => "opening",
            ItemKind::Closing => "closing",
            ItemKind::Procedural => "procedural",
            ItemKind::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgendaItem {
    pub agenda_id: String,
    pub item_kind: ItemKind,
    pub start_block: u32,
    pub end_block: u32,
    pub title_nl: String,
    pub title_fr: String,
    pub dossier_id: String,
    pub document_id: String,
    pub internal_ids: Vec<String>,
    pub item_id: String,
    pub source_section: String,
}

static AGENDA_NUM: OnceLock<Regex> = OnceLock::new();
static INTERNAL_ID: OnceLock<Regex> = OnceLock::new();
static DOSSIER_REF: OnceLock<Regex> = OnceLock::new();
static SELECTOR_SPAN: OnceLock<Selector> = OnceLock::new();

fn agenda_num_regex() -> &'static Regex {
    AGENDA_NUM.get_or_init(|| Regex::new(r"^(\d{2})\b").unwrap())
}

fn internal_id_regex() -> &'static Regex {
    INTERNAL_ID.get_or_init(|| Regex::new(r"\(Q(\d{6,8}[A-Za-z])\)").unwrap())
}

fn dossier_ref_regex() -> &'static Regex {
    DOSSIER_REF.get_or_init(|| Regex::new(r"\((\d+)/(\d+(?:-\d+)?)\)").unwrap())
}

fn selector_span() -> &'static Selector {
    SELECTOR_SPAN.get_or_init(|| Selector::parse("span").unwrap())
}

fn looks_like_fr_heading(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("question de")
        || lower.contains("questions jointes")
        || lower.contains("audition")
}

fn is_bilingual_fr_heading(block: &ReportBlock, item: &AgendaItem) -> bool {
    if let Some(agenda_id) = extract_agenda_number(&block.text) {
        return agenda_id == item.agenda_id;
    }
    looks_like_fr_heading(&block.text)
}

pub fn build_agenda_timeline(
    document: &Html,
    blocks: &[ReportBlock],
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
) -> Vec<AgendaItem> {
    let mut items: Vec<AgendaItem> = Vec::new();
    let mut current_section = String::new();
    let mut question_seq = 0i32;
    let mut hearing_seq = 0i32;
    let mut interpellation_seq = 0i32;
    let mut pending_nl: Option<(u32, String)> = None;

    for block in blocks.iter() {
        if block.tag == BlockTag::H1 {
            current_section = block.text.to_lowercase();
            continue;
        }

        if block.tag != BlockTag::H2 {
            continue;
        }

        if let Some((start, _)) = pending_nl.take() {
            if let Some(item) = items.last_mut() {
                if item.start_block == start
                    && item.title_fr.is_empty()
                    && is_bilingual_fr_heading(block, item)
                {
                    item.title_fr = block.text.clone();
                    continue;
                }
            }
        }

        let Some(agenda_id) = extract_agenda_number(&block.text) else {
            continue;
        };

        if let Some(last) = items.last_mut() {
            last.end_block = block.index;
        }

        let item_kind = classify_heading_kind(meeting_kind, &current_section, &block.text);

        let mut item_id = String::new();
        match item_kind {
            ItemKind::Question => {
                item_id =
                    composite_scoped_id(session_id, meeting_kind.as_str(), meeting_id, question_seq);
                question_seq += 1;
            }
            ItemKind::Hearing => {
                item_id =
                    composite_scoped_id(session_id, meeting_kind.as_str(), meeting_id, hearing_seq);
                hearing_seq += 1;
            }
            ItemKind::Interpellation => {
                item_id = composite_scoped_id(
                    session_id,
                    meeting_kind.as_str(),
                    meeting_id,
                    interpellation_seq,
                );
                interpellation_seq += 1;
            }
            _ => {}
        }

        let internal_ids = extract_internal_ids(document, &block.text);
        let (dossier_id, document_id) = extract_dossier_refs(session_id, &block.text);

        pending_nl = Some((block.index, block.text.clone()));

        items.push(AgendaItem {
            agenda_id,
            item_kind,
            start_block: block.index,
            end_block: blocks.len() as u32,
            title_nl: block.text.clone(),
            title_fr: String::new(),
            dossier_id,
            document_id,
            internal_ids,
            item_id,
            source_section: current_section.clone(),
        });
    }

    if let Some((start, nl_title)) = pending_nl {
        if let Some(item) = items.iter_mut().find(|it| it.start_block == start) {
            item.title_nl = nl_title;
        }
    }

    items
}

fn extract_agenda_number(text: &str) -> Option<String> {
    agenda_num_regex()
        .captures(text.trim())
        .map(|c| c[1].to_string())
}

fn extract_internal_ids(_document: &Html, text: &str) -> Vec<String> {
    internal_id_regex()
        .captures_iter(text)
        .map(|c| format!("Q{}", &c[1]))
        .collect()
}

fn extract_dossier_refs(session_id: u32, text: &str) -> (String, String) {
    dossier_ref_regex()
        .captures(text)
        .map(|c| {
            (
                format!("{}/{}", session_id, &c[1]),
                c[2].trim().to_string(),
            )
        })
        .unwrap_or_default()
}

pub fn agenda_item_for_block<'a>(items: &'a [AgendaItem], block_index: u32) -> Option<&'a AgendaItem> {
    items
        .iter()
        .rev()
        .find(|item| block_index >= item.start_block && block_index < item.end_block)
}

/// Count question agenda items from a cached meeting report (for QA crosschecks).
pub fn count_agenda_questions_from_cache(
    cache_path: &std::path::Path,
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
) -> Result<usize, Box<dyn std::error::Error>> {
    use crate::report_blocks::{parse_report_blocks, read_report_html};
    let html = read_report_html(cache_path)?;
    let document = Html::parse_document(&html);
    let blocks = parse_report_blocks(&document);
    let items = build_agenda_timeline(&document, &blocks, meeting_kind, session_id, meeting_id);
    Ok(items
        .iter()
        .filter(|a| a.item_kind == ItemKind::Question)
        .count())
}

pub fn heading_text_from_block(block: &ReportBlock) -> String {
    clean_text(&block.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report_blocks::{parse_report_blocks, read_report_html};

    fn block(index: u32, tag: BlockTag, text: &str) -> ReportBlock {
        ReportBlock {
            index,
            tag,
            text: text.to_string(),
            lang: None,
            class: None,
            has_oraspr: false,
        }
    }

    #[test]
    fn bilingual_h2_pair_does_not_extend_item_past_next_heading() {
        let blocks = vec![
            block(0, BlockTag::H1, "Mondelinge vragen"),
            block(1, BlockTag::H2, "01 Vraag van Jan Jansen"),
            block(2, BlockTag::H2, "01 Question de Jan Jansen"),
            block(3, BlockTag::P, "Antwoord content item 1"),
            block(4, BlockTag::H2, "02 Vraag van Piet Pieters"),
            block(5, BlockTag::H2, "02 Question de Piet Pieters"),
            block(6, BlockTag::P, "Antwoord content item 2"),
        ];
        let document = Html::parse_document("<html></html>");
        let items = build_agenda_timeline(
            &document,
            &blocks,
            MeetingKind::Plenary,
            56,
            1,
        );

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].start_block, 1);
        assert_eq!(items[0].end_block, 4, "item 1 must end before item 2 heading");
        assert_eq!(items[1].start_block, 4);
        assert_eq!(items[1].end_block, blocks.len() as u32);

        assert!(agenda_item_for_block(&items, 3).is_some_and(|i| i.agenda_id == "01"));
        assert!(
            agenda_item_for_block(&items, 6).is_some_and(|i| i.agenda_id == "02"),
            "paragraph after item 2 headings must belong to item 2, not item 1"
        );
        assert!(
            !agenda_item_for_block(&items, 6).is_some_and(|i| i.agenda_id == "01"),
            "item 1 range must not swallow item 2 content"
        );
    }

    #[test]
    fn bilingual_fr_without_agenda_number_does_not_swallow_next_item() {
        let blocks = vec![
            block(0, BlockTag::H1, "Mondelinge vragen"),
            block(1, BlockTag::H2, "01 Samengevoegde vragen van"),
            block(2, BlockTag::H2, "- Jan Jansen aan minister (56000001P)"),
            block(3, BlockTag::H2, "Questions jointes de"),
            block(4, BlockTag::P, "01.01 Speaker: text"),
            block(5, BlockTag::H2, "02 Vraag van Piet Pieters"),
            block(6, BlockTag::P, "02.01 Speaker: text"),
        ];
        let document = Html::parse_document("<html></html>");
        let items = build_agenda_timeline(
            &document,
            &blocks,
            MeetingKind::Plenary,
            56,
            1,
        );

        assert_eq!(items.len(), 2, "expected two agenda items, got {:?}", items);
        assert_eq!(items[0].end_block, 5);
        assert!(agenda_item_for_block(&items, 4).is_some_and(|i| i.agenda_id == "01"));
        assert!(agenda_item_for_block(&items, 6).is_some_and(|i| i.agenda_id == "02"));
    }

    #[test]
    fn bilingual_pairs_on_plenary_fixture_have_non_overlapping_ranges() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/plenary/56-117.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        let items = build_agenda_timeline(
            &document,
            &blocks,
            MeetingKind::Plenary,
            56,
            117,
        );

        for window in items.windows(2) {
            assert!(
                window[0].end_block <= window[1].start_block,
                "item {} end_block {} overlaps item {} start_block {}",
                window[0].agenda_id,
                window[0].end_block,
                window[1].agenda_id,
                window[1].start_block
            );
        }

        for block in &blocks {
            if block.tag != BlockTag::P {
                continue;
            }
            if let Some(item) = agenda_item_for_block(&items, block.index) {
                assert!(
                    block.index < item.end_block,
                    "block {} matched item {} with end_block {}",
                    block.index,
                    item.agenda_id,
                    item.end_block
                );
            }
        }
    }
}
