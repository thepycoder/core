//! Oral question rows derived from agenda timeline items.

use crate::agenda_timeline::{AgendaItem, ItemKind, MeetingKind};
use regex::Regex;
use std::collections::HashMap;
use std::error::Error;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OralQuestionDraft {
    pub question_id: String,
    pub session_id: u32,
    pub meeting_id: u32,
    pub questioners: String,
    /// Addressee(s) parsed from the question heading ("Vraag van X aan Y").
    /// Upstream renamed this from `respondents`: the actual respondents are
    /// derived from discussion speakers and may differ from the addressee.
    pub questionees: String,
    pub topics_nl: String,
    pub topics_fr: String,
    pub internal_ids: String,
    pub source_url: String,
    pub cache_path: String,
}

struct QuestionData {
    questioners: Vec<String>,
    questionees: Vec<String>,
    topics: Vec<String>,
    internal_ids: Vec<String>,
}

static PLENARY_QUESTION_REGEX: OnceLock<Regex> = OnceLock::new();
static COMMISSION_QUESTION_REGEX: OnceLock<Regex> = OnceLock::new();
static QUESTION_PREFIX: OnceLock<Regex> = OnceLock::new();
static AGENDA_PREFIX: OnceLock<Regex> = OnceLock::new();

fn plenary_question_regex() -> &'static Regex {
    // NOTE: Handles question IDs in the format of `(56001442P)`
    PLENARY_QUESTION_REGEX.get_or_init(|| {
        Regex::new(
            r#"(?m)(?:(?:Vraag van|Question de)\s)?([^\n]+?)\s+(?:aan|à)\s+([^\n]+?)\s*\([^)]*\)\s*(?:over|sur)\s*(.+?)(?:\s*\((\d{8}[A-Z])\))?\s*$"#,
        )
        .unwrap()
    })
}

fn commission_question_regex() -> &'static Regex {
    // NOTE: Handles question IDs in the format of `(56002763C)`, `(nr. 6003263c)` and `(n° 6003263c)`
    // NOTE: Handles both ” and " quotes (which is a mistake in meeting 157 question 8)
    // NOTE: Handles missing questionee (which is a mistake in meeting 357 question 35)
    COMMISSION_QUESTION_REGEX.get_or_init(|| {
        Regex::new(
            r#"(?m)(?:(?:Vraag van|Question de)\s)?([^\n]+?)(?:\s+(?:aan|à|au)\s+([^\n]+?))?(?:\s*\(.*?\))?\s*(?:over|sur)\s*["'“”](.+?)["'“”]\s*\(?(?:n[°ro]\.?\s*)?(\d{6,8}[A-Za-z])\)?"#,
        )
        .unwrap()
    })
}

fn question_prefix_regex() -> &'static Regex {
    QUESTION_PREFIX.get_or_init(|| Regex::new(r"(?i)^(?:vraag van|question de)\s+").unwrap())
}

fn agenda_prefix_regex() -> &'static Regex {
    AGENDA_PREFIX.get_or_init(|| Regex::new(r"^\d{2}\s+").unwrap())
}

/// Strip scrape artefacts from a captured questioner field (commission sub-question lines).
pub fn normalize_questioner_name(raw: &str) -> Option<String> {
    let mut name = raw.trim().trim_start_matches('-').trim().to_string();
    if name.is_empty() {
        return None;
    }

    name = agenda_prefix_regex().replace(&name, "").trim().to_string();
    name = question_prefix_regex()
        .replace(&name, "")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }

    name = name.replace("- ", "").replace("de heer ", "");
    name = name.trim().trim_end_matches('-').trim().to_string();
    if name.is_empty() {
        return None;
    }

    Some(name)
}

pub fn extract_questions_from_agenda(
    agenda: &[AgendaItem],
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    typo_map: &HashMap<String, String>,
    source_url: &str,
    cache_path: &str,
) -> Result<Vec<OralQuestionDraft>, Box<dyn Error>> {
    agenda
        .iter()
        .filter(|item| item.item_kind == ItemKind::Question)
        .map(|item| {
            let data_nl = extract_question_data(meeting_kind, typo_map, &item.title_nl)?;
            let data_fr = extract_question_data(meeting_kind, typo_map, &item.title_fr)?;
            let mut internal_ids = item.internal_ids.clone();
            internal_ids.extend(data_nl.internal_ids);
            internal_ids.extend(data_fr.internal_ids);
            internal_ids.sort();
            internal_ids.dedup();
            Ok(OralQuestionDraft {
                question_id: item.item_id.clone(),
                session_id,
                meeting_id,
                questioners: data_nl.questioners.join(","),
                questionees: data_nl.questionees.join(","),
                topics_nl: data_nl.topics.join(";"),
                topics_fr: data_fr.topics.join(";"),
                internal_ids: internal_ids.join(","),
                source_url: source_url.to_string(),
                cache_path: cache_path.to_string(),
            })
        })
        .collect()
}

fn extract_question_data(
    meeting_kind: MeetingKind,
    typo_map: &HashMap<String, String>,
    question_text: &str,
) -> Result<QuestionData, Box<dyn Error>> {
    let mut questioners = Vec::new();
    let mut topics = Vec::new();
    let mut questionees = Vec::new();
    let mut internal_ids = Vec::new();

    match meeting_kind {
        MeetingKind::Plenary => {
            for capture in plenary_question_regex().captures_iter(question_text) {
                let Some(questioner_raw) = normalize_questioner_name(&capture[1]) else {
                    continue;
                };
                let questioner = typo_map
                    .get(&questioner_raw)
                    .cloned()
                    .unwrap_or(questioner_raw);
                let questionee = capture[2].trim().to_string();
                let topic = capture
                    .get(3)
                    .or_else(|| capture.get(4))
                    .or_else(|| capture.get(5))
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default();
                let internal_id = capture
                    .get(4)
                    .map(|m| format!("Q{}", m.as_str().trim()))
                    .unwrap_or_default();

                questioners.push(questioner);
                if !questionees.contains(&questionee) {
                    questionees.push(questionee);
                }
                internal_ids.push(internal_id);
                topics.push(topic);
            }
        }
        MeetingKind::Commission => {
            for capture in commission_question_regex().captures_iter(question_text) {
                let Some(questioner) = normalize_questioner_name(&capture[1]) else {
                    continue;
                };
                let questionee = capture
                    .get(2)
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_else(|| "Onbekend".to_string());
                let topic = capture[3].trim().to_string();
                let internal_id = format!("Q{}", capture[4].trim());

                questioners.push(questioner);
                if !questionees.contains(&questionee) {
                    questionees.push(questionee);
                }
                internal_ids.push(internal_id);
                topics.push(topic);
            }
        }
    }

    Ok(QuestionData {
        questioners,
        questionees,
        topics,
        internal_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_questioner_strips_subquestion_header() {
        assert_eq!(
            normalize_questioner_name("-Vraag van Xavier Dubois"),
            Some("Xavier Dubois".to_string())
        );
        assert_eq!(
            normalize_questioner_name("-Natalie Eggermont"),
            Some("Natalie Eggermont".to_string())
        );
        assert_eq!(
            normalize_questioner_name("Question de François De Smet"),
            Some("François De Smet".to_string())
        );
        assert_eq!(
            normalize_questioner_name("03 Question de Michel De Maegd"),
            Some("Michel De Maegd".to_string())
        );
    }

    #[test]
    fn plenary_questioner_strips_agenda_number_and_heading() {
        let text = "03 Question de Michel De Maegd à Alexander De Croo (test) over test";
        let data = extract_question_data(MeetingKind::Plenary, &HashMap::new(), text).unwrap();
        assert_eq!(data.questioners, vec!["Michel De Maegd".to_string()]);
    }

    #[test]
    fn extract_question_data_parses_merged_subquestion_block() {
        let text = "-Vraag van Xavier Dubois aan Bernard Quintin (Veiligheid) over \"test topic\" (56001234C)";
        let data = extract_question_data(MeetingKind::Commission, &HashMap::new(), text).unwrap();
        assert_eq!(data.questioners, vec!["Xavier Dubois".to_string()]);
        assert_eq!(data.questionees, vec!["Bernard Quintin".to_string()]);
    }

    fn commission_fixture_questions(meeting_id: u32) -> Option<Vec<OralQuestionDraft>> {
        use crate::agenda_timeline::build_agenda_timeline;
        use crate::report_blocks::{parse_report_blocks, read_report_html};
        use scraper::Html;

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../cache/sessions/56/meetings/commission/56-{meeting_id}.html"
        ));
        if !path.exists() {
            return None;
        }
        let html = read_report_html(&path).ok()?;
        let document = Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        let agenda = build_agenda_timeline(&blocks, MeetingKind::Commission, 56, meeting_id);
        extract_questions_from_agenda(
            &agenda,
            MeetingKind::Commission,
            56,
            meeting_id,
            &HashMap::new(),
            "url",
            "cache",
        )
        .ok()
    }

    #[test]
    fn commission_meeting_6_has_no_empty_internal_ids() {
        let Some(questions) = commission_fixture_questions(6) else {
            return;
        };
        assert!(
            questions.iter().all(|q| !q.internal_ids.is_empty()),
            "every question must have internal_ids: {:?}",
            questions
                .iter()
                .filter(|q| q.internal_ids.is_empty())
                .map(|q| &q.question_id)
                .collect::<Vec<_>>()
        );
        assert!(
            questions
                .iter()
                .any(|q| q.internal_ids.contains("Q56000070C")),
            "grouped question ids must be present"
        );
    }

    #[test]
    fn commission_meeting_58_grouped_question_ids_present() {
        let Some(questions) = commission_fixture_questions(58) else {
            return;
        };
        let all_ids: String = questions
            .iter()
            .map(|q| q.internal_ids.as_str())
            .collect::<Vec<_>>()
            .join(",");
        for id in ["Q56001293C", "Q56001331C", "Q56001332C"] {
            assert!(all_ids.contains(id), "missing {id} in {all_ids}");
        }
        assert!(
            questions
                .iter()
                .all(|q| !q.questioners.is_empty() && !q.internal_ids.is_empty()),
            "no fully empty question rows"
        );
    }

    #[test]
    fn commission_meeting_157_questioners_have_no_heading_prefix() {
        let Some(questions) = commission_fixture_questions(157) else {
            return;
        };
        assert!(
            questions.iter().all(|q| {
                !q.questioners.contains("Vraag van") && !q.questioners.contains("Question de")
            }),
            "questioner headings must be stripped: {:?}",
            questions
                .iter()
                .map(|q| &q.questioners)
                .filter(|name| name.contains("Vraag van") || name.contains("Question de"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn commission_meeting_15_hearing_only_has_no_questions() {
        use crate::agenda_timeline::build_agenda_timeline;
        use crate::report_blocks::{parse_report_blocks, read_report_html};
        use scraper::Html;

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/commission/56-15.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        let agenda = build_agenda_timeline(&blocks, MeetingKind::Commission, 56, 15);
        let question_items: Vec<_> = agenda
            .iter()
            .filter(|i| i.item_kind == ItemKind::Question)
            .collect();
        assert!(
            question_items.is_empty(),
            "hearing-only meeting must not emit Question agenda items"
        );
    }

    #[test]
    fn commission_meeting_6_question_ids_match_agenda() {
        use crate::meeting_parse::parse_commission_meeting_report;
        use crate::report_blocks::read_report_html;
        use scraper::Html;

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/commission/56-6.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let parsed = parse_commission_meeting_report(&document, 56, 6, "url", "cache", "hash");
        let questions = extract_questions_from_agenda(
            &parsed.agenda,
            MeetingKind::Commission,
            56,
            6,
            &HashMap::new(),
            "url",
            "cache",
        )
        .unwrap();
        let agenda_ids: std::collections::HashSet<_> = parsed
            .agenda
            .iter()
            .filter(|i| i.item_kind == ItemKind::Question)
            .map(|i| i.item_id.as_str())
            .collect();
        let question_ids: std::collections::HashSet<_> =
            questions.iter().map(|q| q.question_id.as_str()).collect();
        assert_eq!(agenda_ids, question_ids);
        assert!(questions.iter().all(|q| !q.internal_ids.is_empty()));
    }
}
