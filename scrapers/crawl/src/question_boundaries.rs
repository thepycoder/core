/// How an H2 heading participates in oral-question grouping (shared by scrapers and agenda).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionHeadingRole {
    GroupStart,
    SubQuestion,
    Single,
    /// FR group header without sub-question lines, e.g. "01 Questions jointes de".
    FrGroupHeader,
    Hearing,
    Unrelated,
}

pub const QUESTIONS_SECTION_KEYWORDS: &[&str] = &[
    "mondelinge vragen",
    "questions orales",
    "question orales",
    "questions",
];

pub fn is_questions_section(section: &str) -> bool {
    let section = section.to_lowercase();
    QUESTIONS_SECTION_KEYWORDS
        .iter()
        .any(|keyword| section.contains(keyword))
}

use regex::Regex;
use std::sync::OnceLock;

static AGENDA_PREFIX: OnceLock<Regex> = OnceLock::new();

fn agenda_prefix_regex() -> &'static Regex {
    AGENDA_PREFIX.get_or_init(|| Regex::new(r"^(\d{2})\s+").unwrap())
}

fn heading_body_after_agenda(text: &str) -> String {
    let trimmed = text.trim();
    agenda_prefix_regex()
        .captures(trimmed)
        .map(|caps| trimmed[caps[0].len()..].trim().to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

pub fn is_hearing_text(text: &str) -> bool {
    if is_subquestion_text(text)
        || is_single_text(text)
        || is_group_start_text(text)
        || is_fr_group_header_text(text)
    {
        return false;
    }
    let lower = text.to_lowercase();
    lower.contains("hoorzitting") || lower.contains("audition")
}

pub fn is_group_start_text(text: &str) -> bool {
    let body = heading_body_after_agenda(text);
    let lower = body.to_lowercase();
    lower.contains("samengevoegde vragen") || lower.contains("toegevoegde vragen")
}

pub fn is_subquestion_text(text: &str) -> bool {
    text.trim().starts_with('-')
}

pub fn is_single_text(text: &str) -> bool {
    let body = heading_body_after_agenda(text);
    body.starts_with("Vraag van") || body.starts_with("Question de")
}

pub fn is_fr_group_header_text(text: &str) -> bool {
    let body = heading_body_after_agenda(text);
    let lower = body.to_lowercase();
    lower.contains("questions jointes") && !is_subquestion_text(text) && !is_single_text(text)
}

pub fn classify_question_heading_text(text: &str) -> QuestionHeadingRole {
    if is_subquestion_text(text) {
        return QuestionHeadingRole::SubQuestion;
    }
    if is_group_start_text(text) {
        return QuestionHeadingRole::GroupStart;
    }
    if is_single_text(text) {
        return QuestionHeadingRole::Single;
    }
    if is_fr_group_header_text(text) {
        return QuestionHeadingRole::FrGroupHeader;
    }
    if is_hearing_text(text) {
        return QuestionHeadingRole::Hearing;
    }
    QuestionHeadingRole::Unrelated
}

pub fn classify_question_heading_bilingual(
    nl: Option<&str>,
    fr: Option<&str>,
) -> QuestionHeadingRole {
    for text in [nl, fr].into_iter().flatten() {
        let role = classify_question_heading_text(text);
        if role != QuestionHeadingRole::Unrelated {
            return role;
        }
    }
    QuestionHeadingRole::Unrelated
}

pub fn has_pending_question_text(nl: &str, fr: &str) -> bool {
    !nl.is_empty() || !fr.is_empty()
}

/// Whether a new flushed question row / agenda Question item should begin here.
pub fn starts_new_question_unit(role: QuestionHeadingRole) -> bool {
    matches!(
        role,
        QuestionHeadingRole::GroupStart | QuestionHeadingRole::Single
    )
}

/// Whether this heading extends the open question instead of starting a new one.
pub fn extends_open_question(role: QuestionHeadingRole) -> bool {
    matches!(
        role,
        QuestionHeadingRole::SubQuestion | QuestionHeadingRole::FrGroupHeader
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_group_sub_and_single() {
        assert_eq!(
            classify_question_heading_text("01 Samengevoegde vragen van"),
            QuestionHeadingRole::GroupStart
        );
        assert_eq!(
            classify_question_heading_text("- Jan Jansen aan minister"),
            QuestionHeadingRole::SubQuestion
        );
        assert_eq!(
            classify_question_heading_text("02 Vraag van Piet Pieters"),
            QuestionHeadingRole::Single
        );
        assert_eq!(
            classify_question_heading_text("01 Questions jointes de"),
            QuestionHeadingRole::FrGroupHeader
        );
    }

    #[test]
    fn subquestion_about_hearing_followup_stays_subquestion() {
        assert_eq!(
            classify_question_heading_text(
                "- Michael Freilich aan minister over \"De opvolging van de hoorzitting met Proximus\" (56001293C)"
            ),
            QuestionHeadingRole::SubQuestion
        );
    }

    #[test]
    fn eulogy_in_questions_section_is_unrelated() {
        assert_eq!(
            classify_question_heading_text("01 Rouwhulde – de heer Geert Versnick"),
            QuestionHeadingRole::Unrelated
        );
    }
}
