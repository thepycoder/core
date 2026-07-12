use crate::proceeding_entities::{
    classify_heading_kind, extract_interpellation_ids_from_text, is_french_interpellation_bullet,
    is_interpellation_bullet_line, is_interpellation_section, is_joint_interpellation_fr_header,
    is_joint_interpellation_group_start,
};
use crate::question_boundaries::{
    classify_question_heading_text, extends_open_question, is_questions_section,
    starts_new_question_unit, QuestionHeadingRole,
};
use crate::report_blocks::{BlockTag, ReportBlock};
use crate::utils::{clean_text, composite_scoped_id};
use regex::Regex;
use scraper::Html;
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

fn agenda_num_regex() -> &'static Regex {
    AGENDA_NUM.get_or_init(|| Regex::new(r"^(\d{2})\b").unwrap())
}

fn internal_id_regex() -> &'static Regex {
    INTERNAL_ID.get_or_init(|| Regex::new(r"\(Q(\d{6,8}[A-Za-z])\)").unwrap())
}

fn dossier_ref_regex() -> &'static Regex {
    DOSSIER_REF.get_or_init(|| Regex::new(r"\((\d+)/(\d+(?:-\d+)?)\)").unwrap())
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

fn extend_open_question(item: &mut AgendaItem, text: &str, role: QuestionHeadingRole) {
    match role {
        QuestionHeadingRole::SubQuestion => {
            if !item.title_nl.is_empty() {
                item.title_nl.push('\n');
            }
            item.title_nl.push_str(text);
        }
        QuestionHeadingRole::FrGroupHeader if item.title_fr.is_empty() => {
            item.title_fr = text.to_string();
        }
        _ => {}
    }
}

fn close_item_range(items: &mut [AgendaItem], block_index: u32) {
    if let Some(last) = items.last_mut() {
        last.end_block = block_index;
    }
}

fn should_emit_question_item(
    meeting_kind: MeetingKind,
    section: &str,
    heading_role: QuestionHeadingRole,
) -> bool {
    if !starts_new_question_unit(heading_role) {
        return false;
    }
    match meeting_kind {
        MeetingKind::Commission => true,
        MeetingKind::Plenary => is_questions_section(section),
    }
}

fn push_question_item(
    items: &mut Vec<AgendaItem>,
    open_question_idx: &mut Option<usize>,
    question_seq: &mut i32,
    document: &Html,
    block: &ReportBlock,
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    agenda_id: &str,
    current_section: &str,
    blocks_len: u32,
) {
    let item_id = composite_scoped_id(session_id, meeting_kind.as_str(), meeting_id, *question_seq);
    *question_seq += 1;
    *open_question_idx = Some(items.len());
    let internal_ids = extract_internal_ids(document, &block.text);
    let (dossier_id, document_id) = extract_dossier_refs(session_id, &block.text);
    items.push(AgendaItem {
        agenda_id: agenda_id.to_string(),
        item_kind: ItemKind::Question,
        start_block: block.index,
        end_block: blocks_len,
        title_nl: block.text.clone(),
        title_fr: String::new(),
        dossier_id,
        document_id,
        internal_ids,
        item_id,
        source_section: current_section.to_string(),
    });
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
    let mut interpellation_group_agenda_id = String::new();
    let mut interpellation_fr_phase = false;
    let mut pending_nl: Option<(u32, String)> = None;
    let mut open_question_idx: Option<usize> = None;

    for block in blocks.iter() {
        if block.tag == BlockTag::H1 {
            current_section = block.text.to_lowercase();
            interpellation_group_agenda_id.clear();
            interpellation_fr_phase = false;
            continue;
        }

        if block.tag != BlockTag::H2 {
            continue;
        }

        if let Some((start, _)) = pending_nl.take() {
            if let Some(idx) = open_question_idx {
                if items[idx].start_block == start
                    && items[idx].title_fr.is_empty()
                    && is_bilingual_fr_heading(block, &items[idx])
                {
                    items[idx].title_fr = block.text.clone();
                    continue;
                }
            } else if let Some(item) = items.last_mut() {
                if item.start_block == start
                    && item.title_fr.is_empty()
                    && is_bilingual_fr_heading(block, item)
                {
                    item.title_fr = block.text.clone();
                    continue;
                }
            }
        }

        let heading_role = classify_question_heading_text(&block.text);
        let agenda_id = extract_agenda_number(&block.text);

        if is_interpellation_section(&current_section) {
            if is_joint_interpellation_group_start(&block.text) {
                if !is_joint_interpellation_fr_header(&block.text) {
                    close_item_range(&mut items, block.index);
                }
                open_question_idx = None;
                if let Some(id) = agenda_id.as_ref() {
                    interpellation_group_agenda_id = id.clone();
                }
                interpellation_fr_phase = is_joint_interpellation_fr_header(&block.text);
                pending_nl = Some((block.index, block.text.clone()));
                continue;
            }

            if is_interpellation_bullet_line(&block.text) {
                open_question_idx = None;

                let internal_ids = extract_interpellation_ids_from_text(&block.text);
                let is_fr =
                    interpellation_fr_phase || is_french_interpellation_bullet(&block.text);

                if is_fr {
                    if let Some(site_id) = internal_ids.first() {
                        if let Some(item) = items.iter_mut().find(|it| {
                            it.item_kind == ItemKind::Interpellation
                                && it.internal_ids.iter().any(|id| id == site_id)
                        }) {
                            item.title_fr = block.text.clone();
                            item.end_block = blocks.len() as u32;
                            pending_nl = Some((block.index, block.text.clone()));
                            continue;
                        }
                    }
                }

                close_item_range(&mut items, block.index);

                let item_id = composite_scoped_id(
                    session_id,
                    meeting_kind.as_str(),
                    meeting_id,
                    interpellation_seq,
                );
                interpellation_seq += 1;
                let (dossier_id, document_id) = extract_dossier_refs(session_id, &block.text);

                pending_nl = Some((block.index, block.text.clone()));
                items.push(AgendaItem {
                    agenda_id: interpellation_group_agenda_id.clone(),
                    item_kind: ItemKind::Interpellation,
                    start_block: block.index,
                    end_block: blocks.len() as u32,
                    title_nl: if is_fr {
                        String::new()
                    } else {
                        block.text.clone()
                    },
                    title_fr: if is_fr {
                        block.text.clone()
                    } else {
                        String::new()
                    },
                    dossier_id,
                    document_id,
                    internal_ids,
                    item_id,
                    source_section: current_section.clone(),
                });
                continue;
            }
        }

        if agenda_id.is_none() {
            if extends_open_question(heading_role) {
                if let Some(idx) = open_question_idx {
                    close_item_range(&mut items, block.index);
                    extend_open_question(&mut items[idx], &block.text, heading_role);
                }
            } else if should_emit_question_item(meeting_kind, &current_section, heading_role) {
                close_item_range(&mut items, block.index);
                push_question_item(
                    &mut items,
                    &mut open_question_idx,
                    &mut question_seq,
                    document,
                    block,
                    meeting_kind,
                    session_id,
                    meeting_id,
                    "",
                    &current_section,
                    blocks.len() as u32,
                );
                pending_nl = Some((block.index, block.text.clone()));
            }
            continue;
        }

        let agenda_id = agenda_id.unwrap();
        close_item_range(&mut items, block.index);

        if heading_role == QuestionHeadingRole::Hearing {
            open_question_idx = None;
        }

        if extends_open_question(heading_role) {
            if let Some(idx) = open_question_idx {
                extend_open_question(&mut items[idx], &block.text, heading_role);
            }
            pending_nl = Some((block.index, block.text.clone()));
            continue;
        }

        let item_kind = classify_item_kind(meeting_kind, &current_section, &block.text, heading_role);

        if item_kind == ItemKind::Question && starts_new_question_unit(heading_role) {
            push_question_item(
                &mut items,
                &mut open_question_idx,
                &mut question_seq,
                document,
                block,
                meeting_kind,
                session_id,
                meeting_id,
                &agenda_id,
                &current_section,
                blocks.len() as u32,
            );
            pending_nl = Some((block.index, block.text.clone()));
            continue;
        }

        open_question_idx = None;

        let mut item_id = String::new();
        match item_kind {
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

fn classify_item_kind(
    meeting_kind: MeetingKind,
    section: &str,
    h2_text: &str,
    heading_role: QuestionHeadingRole,
) -> ItemKind {
    let proceeding_kind = classify_heading_kind(meeting_kind, section, h2_text);
    if matches!(
        proceeding_kind,
        ItemKind::Interpellation | ItemKind::Procedural | ItemKind::Hearing
    ) {
        return proceeding_kind;
    }

    match meeting_kind {
        MeetingKind::Plenary => {
            if is_questions_section(section)
                && matches!(
                    heading_role,
                    QuestionHeadingRole::GroupStart | QuestionHeadingRole::Single
                )
            {
                return ItemKind::Question;
            }
            let section = section.to_lowercase();
            if section.contains("voorstel") || section.contains("proposition") {
                return ItemKind::Proposition;
            }
            if section.contains("mededeling") {
                return ItemKind::Notice;
            }
            if section.contains("naamstemming") || section.contains("vote") {
                return ItemKind::Vote;
            }
            ItemKind::GeneralDebate
        }
        MeetingKind::Commission => {
            if matches!(
                heading_role,
                QuestionHeadingRole::GroupStart | QuestionHeadingRole::Single
            ) {
                ItemKind::Question
            } else {
                ItemKind::GeneralDebate
            }
        }
    }
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
    fn plenary_joint_interpellation_bullets_become_agenda_items() {
        let blocks = vec![
            block(0, BlockTag::H1, "Interpellaties"),
            block(1, BlockTag::H2, "01 Samengevoegde interpellaties van"),
            block(
                2,
                BlockTag::H2,
                r#"- Vincent Van Quickenborne aan Jan Jambon over "De meerwaardetaks" (56000109I)"#,
            ),
            block(3, BlockTag::H2, "01 Interpellations jointes de"),
            block(
                4,
                BlockTag::H2,
                r#"- Vincent Van Quickenborne à Jan Jambon sur "La taxe sur les plus-values" (56000109I)"#,
            ),
            block(5, BlockTag::P, "01.01 Vincent Van Quickenborne: speech"),
        ];
        let document = Html::parse_document("<html></html>");
        let items = build_agenda_timeline(
            &document,
            &blocks,
            MeetingKind::Plenary,
            56,
            60,
        );
        let interpellations: Vec<_> = items
            .iter()
            .filter(|i| i.item_kind == ItemKind::Interpellation)
            .collect();
        assert_eq!(interpellations.len(), 1);
        assert_eq!(interpellations[0].agenda_id, "01");
        assert!(interpellations[0].title_nl.contains("Van Quickenborne"));
        assert!(interpellations[0].title_fr.contains("Van Quickenborne"));
        assert!(interpellations[0]
            .internal_ids
            .iter()
            .any(|id| id == "56000109I"));
        assert_eq!(interpellations[0].end_block, blocks.len() as u32);
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
    fn commission_fixture_question_item_ids_match_scraper_seq() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/commission/56-105.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        let items = build_agenda_timeline(
            &document,
            &blocks,
            MeetingKind::Commission,
            56,
            105,
        );
        let questions: Vec<_> = items
            .iter()
            .filter(|i| i.item_kind == ItemKind::Question)
            .collect();
        assert_eq!(questions.len(), 1, "expected one question item");
        assert_eq!(questions[0].item_id, "56_commission_105_0");
        assert_eq!(questions[0].agenda_id, "01");
    }

    #[test]
    fn plenary_fixture_skips_eulogy_in_questions_section() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/plenary/56-82.html");
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
            82,
        );
        let questions: Vec<_> = items
            .iter()
            .filter(|i| i.item_kind == ItemKind::Question)
            .collect();
        assert!(
            !questions.iter().any(|q| q.title_nl.contains("Rouwhulde")),
            "eulogy must not become a Question node"
        );
        assert!(
            questions.len() >= 10,
            "expected real questions, got {}",
            questions.len()
        );
        for (seq, q) in questions.iter().enumerate() {
            assert_eq!(q.item_id, format!("56_plenary_82_{seq}"));
        }
    }

    #[test]
    fn plenary_fixture_question_count_matches_staging_shape() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/plenary/56-109.html");
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
            109,
        );
        let questions: Vec<_> = items
            .iter()
            .filter(|i| i.item_kind == ItemKind::Question)
            .collect();
        assert_eq!(questions.len(), 10);
        for (seq, q) in questions.iter().enumerate() {
            assert_eq!(q.item_id, format!("56_plenary_109_{seq}"));
        }
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
