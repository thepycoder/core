use crate::agenda_timeline::{AgendaItem, ItemKind, MeetingKind};
use crate::report_blocks::parse_report_blocks;
use regex::Regex;
use scraper::Html;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct HearingDraft {
    pub hearing_id: String,
    pub session_id: u32,
    pub meeting_id: u32,
    pub meeting_kind: MeetingKind,
    pub agenda_id: String,
    pub title_nl: String,
    pub title_fr: String,
    pub witnesses: String,
    pub dossier_id: String,
    pub internal_ids: String,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone)]
pub struct InterpellationDraft {
    pub interpellation_id: String,
    pub session_id: u32,
    pub meeting_id: u32,
    pub meeting_kind: MeetingKind,
    pub agenda_id: String,
    pub interpellators: String,
    pub respondents: String,
    pub topics_nl: String,
    pub topics_fr: String,
    pub internal_ids: String,
    pub dossier_id: String,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone, Default)]
struct InterpellationParsed {
    interpellators: Vec<String>,
    respondents: Vec<String>,
    topics: Vec<String>,
    internal_ids: Vec<String>,
}

static INTERPELLATION_REGEX: OnceLock<Regex> = OnceLock::new();
static INTERPELLATION_BULLET_REGEX: OnceLock<Regex> = OnceLock::new();
static INTERPELLATION_ID: OnceLock<Regex> = OnceLock::new();

fn interpellation_regex() -> &'static Regex {
    INTERPELLATION_REGEX.get_or_init(|| {
        Regex::new(
            r#"(?m)(?:Interpellatie van|Interpellation de)\s+(.+?)\s+(?:aan|à)\s+(.+?)\s+(?:over|sur)\s*["'""](.+?)["'""]\s*\((\d{8}[Ii])\)"#,
        )
        .unwrap()
    })
}

fn interpellation_bullet_regex() -> &'static Regex {
    INTERPELLATION_BULLET_REGEX.get_or_init(|| {
        Regex::new(
            r#"(?m)^-\s*(.+?)\s+(?:aan|à)\s+(.+?)\s+(?:over|sur)\s*["'""](.+?)["'""]\s*\((\d{8}[Ii])\)"#,
        )
        .unwrap()
    })
}

fn interpellation_id_regex() -> &'static Regex {
    INTERPELLATION_ID.get_or_init(|| Regex::new(r"\((\d{8}[Ii])\)").unwrap())
}

pub fn is_interpellation_section(section: &str) -> bool {
    let lower = section.to_lowercase();
    lower.contains("interpellatie") || lower.contains("interpellation")
}

pub fn is_joint_interpellation_group_start(text: &str) -> bool {
    let body = heading_body(&text.to_lowercase());
    body.contains("samengevoegde interpellaties") || body.contains("interpellations jointes")
}

pub fn is_joint_interpellation_fr_header(text: &str) -> bool {
    let body = heading_body(&text.to_lowercase());
    body.contains("interpellations jointes")
}

pub fn is_interpellation_bullet_line(text: &str) -> bool {
    let clean = clean_heading(text);
    if !clean.starts_with('-') {
        return false;
    }
    interpellation_bullet_regex().is_match(&clean)
        || (interpellation_id_regex().is_match(&clean)
            && (clean.contains(" aan ") || clean.contains(" à ")))
}

pub fn is_french_interpellation_bullet(text: &str) -> bool {
    let clean = clean_heading(text);
    clean.contains(" à ") || clean.contains(" sur \"")
}

/// True when an h2 heading is a hearing or interpellation agenda item (not a question).
pub fn is_non_question_proceeding_heading(text: &str) -> bool {
    let lower = text.to_lowercase();
    if is_motion_conclusion_heading(&lower) {
        return false;
    }
    is_hearing_heading(&lower) || is_interpellation_heading(&lower)
}

pub fn is_motion_conclusion_heading(lower: &str) -> bool {
    lower.contains("motie ingediend tot besluit")
        || lower.contains("moties ingediend tot besluit")
        || lower.contains("motion déposée en conclusion")
        || lower.contains("motions déposées en conclusion")
        || lower.contains("motion deposee en conclusion")
        || lower.contains("motions deposees en conclusion")
}

fn heading_body(lower: &str) -> String {
    static AGENDA_PREFIX: OnceLock<Regex> = OnceLock::new();
    let re = AGENDA_PREFIX.get_or_init(|| Regex::new(r"^\d{2}\s+").unwrap());
    re.replace(lower.trim(), "").trim().to_string()
}

pub fn is_question_heading(lower: &str) -> bool {
    let body = heading_body(lower);
    body.starts_with("vraag van")
        || body.starts_with("question de")
        || body.contains("samengevoegde vragen")
        || body.contains("toegevoegde vragen")
        || body.contains("questions jointes")
        || body.starts_with('-')
}

pub fn is_interpellation_heading(lower: &str) -> bool {
    if is_motion_conclusion_heading(lower) {
        return false;
    }
    let body = heading_body(lower);
    body.contains("interpellatie van")
        || body.contains("interpellation de")
        || is_joint_interpellation_group_start(lower)
}

pub fn is_hearing_heading(lower: &str) -> bool {
    if is_question_heading(lower) {
        return false;
    }
    if is_interpellation_heading(lower) {
        return false;
    }
    lower.contains("hoorzitting met")
        || lower.contains("audition de:")
        || lower.ends_with("audition de")
}

pub fn classify_heading_kind(meeting_kind: MeetingKind, section: &str, h2_text: &str) -> ItemKind {
    let lower = h2_text.to_lowercase();
    let section = section.to_lowercase();

    if is_motion_conclusion_heading(&lower) {
        return ItemKind::Procedural;
    }

    if is_interpellation_heading(&lower) {
        return ItemKind::Interpellation;
    }

    if is_hearing_heading(&lower) {
        return ItemKind::Hearing;
    }

    match meeting_kind {
        MeetingKind::Plenary => {
            if section.contains("mondelinge") || section.contains("question") {
                return ItemKind::Question;
            }
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
            if is_question_heading(&lower) {
                ItemKind::Question
            } else {
                ItemKind::GeneralDebate
            }
        }
    }
}

pub fn extract_proceedings_from_agenda(
    agenda: &[AgendaItem],
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
) -> (Vec<HearingDraft>, Vec<InterpellationDraft>) {
    let mut hearings = Vec::new();
    let mut interpellations = Vec::new();
    let mut interpellation_by_site_id: HashMap<String, InterpellationDraft> = HashMap::new();
    let mut interpellation_order: Vec<String> = Vec::new();

    for item in agenda {
        if item.item_id.is_empty() {
            continue;
        }
        match item.item_kind {
            ItemKind::Hearing => hearings.push(hearing_from_agenda_item(
                item,
                meeting_kind,
                session_id,
                meeting_id,
                source_url,
                cache_path,
            )),
            ItemKind::Interpellation => {
                if let Some(draft) = interpellation_from_agenda_item(
                    item,
                    meeting_kind,
                    session_id,
                    meeting_id,
                    source_url,
                    cache_path,
                ) {
                    let merge_key = primary_internal_id(&draft.internal_ids)
                        .unwrap_or_else(|| draft.interpellation_id.clone());
                    if let Some(existing) = interpellation_by_site_id.get_mut(&merge_key) {
                        merge_interpellation_drafts(existing, &draft);
                    } else {
                        interpellation_order.push(merge_key.clone());
                        interpellation_by_site_id.insert(merge_key, draft);
                    }
                }
            }
            _ => {}
        }
    }

    interpellations.extend(
        interpellation_order
            .into_iter()
            .filter_map(|key| interpellation_by_site_id.remove(&key)),
    );

    (hearings, interpellations)
}

pub fn extract_proceedings_from_document(
    document: &Html,
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
) -> (Vec<HearingDraft>, Vec<InterpellationDraft>) {
    let blocks = parse_report_blocks(document);
    let agenda = crate::agenda_timeline::build_agenda_timeline(
        &blocks,
        meeting_kind,
        session_id,
        meeting_id,
    );
    extract_proceedings_from_agenda(
        &agenda,
        meeting_kind,
        session_id,
        meeting_id,
        source_url,
        cache_path,
    )
}

fn hearing_from_agenda_item(
    item: &AgendaItem,
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
) -> HearingDraft {
    let witnesses = extract_witnesses_from_title(&item.title_nl)
        .or_else(|| extract_witnesses_from_title(&item.title_fr))
        .unwrap_or_default();
    let mut internal_ids = item.internal_ids.clone();
    internal_ids.extend(extract_interpellation_ids_from_text(&item.title_nl));
    internal_ids.extend(extract_interpellation_ids_from_text(&item.title_fr));
    internal_ids.sort();
    internal_ids.dedup();

    HearingDraft {
        hearing_id: item.item_id.clone(),
        session_id,
        meeting_id,
        meeting_kind,
        agenda_id: item.agenda_id.clone(),
        title_nl: clean_heading(&item.title_nl),
        title_fr: clean_heading(&item.title_fr),
        witnesses,
        dossier_id: item.dossier_id.clone(),
        internal_ids: internal_ids.join(","),
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
    }
}

fn interpellation_from_agenda_item(
    item: &AgendaItem,
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
) -> Option<InterpellationDraft> {
    let nl = parse_interpellation_text(&item.title_nl);
    let fr = parse_interpellation_text(&item.title_fr);
    let topics_nl = nl.as_ref().map(|p| p.topics.join(";")).unwrap_or_default();
    let topics_fr = fr.as_ref().map(|p| p.topics.join(";")).unwrap_or_default();
    let parsed = merge_interpellation_parsed(nl.as_ref(), fr.as_ref());

    if parsed.interpellators.is_empty() && parsed.topics.is_empty() && item.internal_ids.is_empty()
    {
        return None;
    }

    let mut internal_ids = item.internal_ids.clone();
    internal_ids.extend(parsed.internal_ids.clone());
    internal_ids.sort();
    internal_ids.dedup();

    let interpellators = if parsed.interpellators.is_empty() {
        String::new()
    } else {
        parsed.interpellators.join(",")
    };
    let respondents = if parsed.respondents.is_empty() {
        String::new()
    } else {
        parsed.respondents.join(",")
    };

    Some(InterpellationDraft {
        interpellation_id: item.item_id.clone(),
        session_id,
        meeting_id,
        meeting_kind,
        agenda_id: item.agenda_id.clone(),
        interpellators,
        respondents,
        topics_nl,
        topics_fr,
        internal_ids: internal_ids.join(","),
        dossier_id: item.dossier_id.clone(),
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
    })
}

fn parse_interpellation_text(text: &str) -> Option<InterpellationParsed> {
    let clean = clean_heading(text);
    if clean.is_empty() {
        return None;
    }
    let mut parsed = InterpellationParsed::default();
    for cap in interpellation_regex()
        .captures_iter(&clean)
        .chain(interpellation_bullet_regex().captures_iter(&clean))
    {
        push_interpellation_capture(&mut parsed, &cap);
    }
    if parsed.interpellators.is_empty() {
        return None;
    }
    Some(parsed)
}

fn push_interpellation_capture(parsed: &mut InterpellationParsed, cap: &regex::Captures) {
    parsed.interpellators.push(cap[1].trim().to_string());
    let respondent = cap[2].trim().to_string();
    if !respondent.is_empty() && !parsed.respondents.contains(&respondent) {
        parsed.respondents.push(respondent);
    }
    parsed.topics.push(cap[3].trim().to_string());
    parsed
        .internal_ids
        .push(cap[4].trim().to_uppercase().replace('i', "I"));
}

fn merge_interpellation_parsed(
    nl: Option<&InterpellationParsed>,
    fr: Option<&InterpellationParsed>,
) -> InterpellationParsed {
    let mut out = InterpellationParsed::default();
    for src in nl.into_iter().chain(fr) {
        for name in &src.interpellators {
            if !out.interpellators.contains(name) {
                out.interpellators.push(name.clone());
            }
        }
        for name in &src.respondents {
            if !out.respondents.contains(name) {
                out.respondents.push(name.clone());
            }
        }
        for topic in &src.topics {
            if !out.topics.contains(topic) {
                out.topics.push(topic.clone());
            }
        }
        for id in &src.internal_ids {
            if !out.internal_ids.contains(id) {
                out.internal_ids.push(id.clone());
            }
        }
    }
    out
}

fn extract_witnesses_from_title(title: &str) -> Option<String> {
    let clean = clean_heading(title);
    let lower = clean.to_lowercase();
    let after = if let Some(idx) = lower.find("hoorzitting met:") {
        &clean[idx + "hoorzitting met:".len()..]
    } else if let Some(idx) = lower.find("audition de:") {
        &clean[idx + "audition de:".len()..]
    } else if lower.ends_with("audition de") {
        return None;
    } else {
        return None;
    };
    let witnesses = after.trim().trim_end_matches(':').trim();
    if witnesses.is_empty() {
        None
    } else {
        Some(witnesses.to_string())
    }
}

pub fn extract_interpellation_ids_from_text(text: &str) -> Vec<String> {
    interpellation_id_regex()
        .captures_iter(text)
        .map(|c| c[1].trim().to_uppercase().replace('i', "I"))
        .collect()
}

fn primary_internal_id(internal_ids: &str) -> Option<String> {
    internal_ids
        .split(',')
        .map(str::trim)
        .find(|id| !id.is_empty())
        .map(str::to_string)
}

fn merge_interpellation_drafts(existing: &mut InterpellationDraft, incoming: &InterpellationDraft) {
    if existing.topics_nl.is_empty() {
        existing.topics_nl = incoming.topics_nl.clone();
    }
    if existing.topics_fr.is_empty() {
        existing.topics_fr = incoming.topics_fr.clone();
    }
    if existing.interpellators.is_empty() {
        existing.interpellators = incoming.interpellators.clone();
    }
    if existing.respondents.is_empty() {
        existing.respondents = incoming.respondents.clone();
    }
    if existing.internal_ids.is_empty() {
        existing.internal_ids = incoming.internal_ids.clone();
    }
    // Keep the first agenda occurrence as the canonical ID. The timeline has
    // already paired later bilingual occurrences to this ID when possible.
}

fn clean_heading(text: &str) -> String {
    text.replace('\u{00A0}', " ")
        .replace("&quot;", "\"")
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report_blocks::read_report_html;

    #[test]
    fn question_about_audition_stays_question_not_hearing() {
        let lower = "06 question de pierre jadoul sur l'audition des mineurs".to_string();
        assert!(is_question_heading(&lower));
        assert!(!is_hearing_heading(&lower));
    }

    #[test]
    fn formal_hearing_heading_detected() {
        let lower = "01 de cop29 en de europese uitdagingen. hoorzitting met:".to_string();
        assert!(is_hearing_heading(&lower));
    }

    #[test]
    fn joint_interpellation_heading_detected() {
        assert!(is_interpellation_heading(
            &"01 Samengevoegde interpellaties van".to_lowercase()
        ));
        assert!(is_joint_interpellation_group_start(
            "01 Interpellations jointes de"
        ));
        assert!(!is_interpellation_heading(
            &"06 Moties ingediend tot besluit van de interpellatie van mevrouw Greet Daems"
                .to_lowercase()
        ));
    }

    #[test]
    fn parse_interpellation_bullet_extracts_fields() {
        let text = r#"- Vincent Van Quickenborne aan Jan Jambon (VEM Financiën) over "De meerwaardetaks" (56000109I)"#;
        let parsed = parse_interpellation_text(text).unwrap();
        assert_eq!(parsed.interpellators[0], "Vincent Van Quickenborne");
        assert!(parsed.respondents[0].contains("Jan Jambon"));
        assert!(parsed.topics[0].contains("meerwaardetaks"));
        assert_eq!(parsed.internal_ids[0], "56000109I");
        assert!(is_interpellation_bullet_line(text));
    }

    #[test]
    fn interpellation_heading_detected() {
        let text = "16 Interpellatie van Raoul Hedebouw aan Bart De Wever over \"De huisvestingstoelage\" (56000070I)";
        assert!(is_interpellation_heading(&text.to_lowercase()));
        assert!(is_motion_conclusion_heading(
            &"19 Motie ingediend tot besluit van de interpellatie".to_lowercase()
        ));
    }

    #[test]
    fn parse_interpellation_regex_extracts_fields() {
        let text = r#"16 Interpellatie van Raoul Hedebouw aan Bart De Wever (eerste minister) over "De huisvestingstoelage van de federale ministers" (56000070I)"#;
        let parsed = parse_interpellation_text(text).unwrap();
        assert_eq!(parsed.interpellators[0], "Raoul Hedebouw");
        assert!(parsed.respondents[0].contains("Bart De Wever"));
        assert!(parsed.topics[0].contains("huisvestingstoelage"));
        assert_eq!(parsed.internal_ids[0], "56000070I");
    }

    #[test]
    fn merged_bilingual_interpellation_keeps_first_agenda_id() {
        let item = |item_id: &str, title: &str| AgendaItem {
            agenda_id: "01".to_string(),
            item_kind: ItemKind::Interpellation,
            start_block: 1,
            end_block: 2,
            title_nl: title.to_string(),
            title_fr: String::new(),
            dossier_id: String::new(),
            document_id: String::new(),
            internal_ids: vec!["56000027I".to_string()],
            item_id: item_id.to_string(),
            source_section: "interpellaties".to_string(),
            title_blocks: vec![1],
        };
        let (_, rows) = extract_proceedings_from_agenda(
            &[
                item(
                    "56_plenary_69_9",
                    "- A aan M over \"Onderwerp\" (56000027I)",
                ),
                item("56_plenary_69_17", "- A à M sur \"Sujet\" (56000027I)"),
            ],
            MeetingKind::Plenary,
            56,
            69,
            "url",
            "cache",
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].interpellation_id, "56_plenary_69_9");
    }

    #[test]
    fn commission_hearing_fixture_yields_entity() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/commission/56-15.html");
        if !path.exists() {
            let alt = std::path::Path::new(
                "/home/victor/Projects/partijgedrag-parent/partijgedrag-3/core/cache/sessions/56/meetings/commission/56-15.html",
            );
            if !alt.exists() {
                return;
            }
            let html = read_report_html(alt).unwrap();
            let document = Html::parse_document(&html);
            let (hearings, _interpellations) = extract_proceedings_from_document(
                &document,
                MeetingKind::Commission,
                56,
                15,
                "url",
                "cache",
            );
            assert!(!hearings.is_empty());
            assert!(hearings.iter().all(|h| !h.hearing_id.is_empty()));
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let (hearings, _) = extract_proceedings_from_document(
            &document,
            MeetingKind::Commission,
            56,
            15,
            "url",
            "cache",
        );
        assert!(!hearings.is_empty());
    }

    #[test]
    fn plenary_interpellation_fixture_yields_entity() {
        for mid in [45u32, 60, 95, 97] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../cache/sessions/56/meetings/plenary/56-{mid}.html"
            ));
            let alt = std::path::PathBuf::from(format!(
                "/home/victor/Projects/partijgedrag-parent/partijgedrag-3/core/cache/sessions/56/meetings/plenary/56-{mid}.html"
            ));
            let path = if path.exists() { path } else { alt };
            if !path.exists() {
                continue;
            }
            let html = read_report_html(&path).unwrap();
            let document = Html::parse_document(&html);
            let (_, interpellations) = extract_proceedings_from_document(
                &document,
                MeetingKind::Plenary,
                56,
                mid,
                "url",
                "cache",
            );
            assert!(
                !interpellations.is_empty(),
                "expected interpellation in meeting {mid}"
            );
        }
    }

    #[test]
    fn plenary_meeting_60_has_joint_interpellations() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/plenary/56-60.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let (_, interpellations) = extract_proceedings_from_document(
            &document,
            MeetingKind::Plenary,
            56,
            60,
            "url",
            "cache",
        );
        assert!(
            interpellations.len() >= 7,
            "expected at least 7 interpellations, got {}",
            interpellations.len()
        );
        assert!(
            interpellations
                .iter()
                .any(|i| i.internal_ids.contains("56000109I")),
            "expected site id 56000109I"
        );
    }

    #[test]
    fn question_with_audition_in_title_is_not_hearing() {
        let kind = classify_heading_kind(
            MeetingKind::Commission,
            "",
            "06 Question de Pierre Jadoul sur \"L'audition des mineurs\" (56000853C)",
        );
        assert_eq!(kind, ItemKind::Question);
    }
}
