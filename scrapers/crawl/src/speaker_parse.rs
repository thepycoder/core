use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerRole {
    Mp,
    Minister,
    Chair,
    External,
    Unknown,
}

impl SpeakerRole {
    pub fn as_str(self) -> &'static str {
        match self {
            SpeakerRole::Mp => "mp",
            SpeakerRole::Minister => "minister",
            SpeakerRole::Chair => "chair",
            SpeakerRole::External => "external",
            SpeakerRole::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub enum TurnStart {
    Numbered {
        turn_number: String,
        raw_label: String,
    },
    GenericChair,
    NamedChair {
        raw_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct ParsedSpeaker {
    pub raw_speaker: String,
    pub speaker_role: SpeakerRole,
    pub party: Option<String>,
}

static TURN_START: OnceLock<Regex> = OnceLock::new();
static CHAIR_GENERIC: OnceLock<Regex> = OnceLock::new();
static CHAIR_NAMED: OnceLock<Regex> = OnceLock::new();
static TITLES_PREFIX: OnceLock<Regex> = OnceLock::new();
static ROLE_SUFFIX: OnceLock<Regex> = OnceLock::new();
static PARTY_SUFFIX: OnceLock<Regex> = OnceLock::new();
static ARTICLE_FP: OnceLock<Regex> = OnceLock::new();

fn turn_start_regex() -> &'static Regex {
    TURN_START.get_or_init(|| {
        Regex::new(
            r"(?xi)^\s*
            (?P<turn>\d{2}\.\d{2}\d?)
            [\s\u00A0\u202F\u00AD]*
            (?P<label>[^:]+?)
            \s*:\s*
            ",
        )
        .unwrap()
    })
}

fn chair_generic_regex() -> &'static Regex {
    CHAIR_GENERIC
        .get_or_init(|| Regex::new(r"(?i)^\s*(?:De\s+voorzitter|Le\s+président)\s*:\s*").unwrap())
}

fn chair_named_regex() -> &'static Regex {
    CHAIR_NAMED.get_or_init(|| {
        Regex::new(r"(?i)^\s*(?P<name>[^:\n]{3,80}),\s*(?:voorzitter|président|president)\s*:\s*")
            .unwrap()
    })
}

fn titles_prefix_regex() -> &'static Regex {
    TITLES_PREFIX.get_or_init(|| {
        Regex::new(
            r"(?i)^(Minister|De heer|Mevrouw|Le ministre|La ministre|Monsieur|Madame|Eerste minister|Staatssecretaris)\s+",
        )
        .unwrap()
    })
}

fn role_suffix_regex() -> &'static Regex {
    ROLE_SUFFIX.get_or_init(|| {
        Regex::new(r"(?i),\s*(?:ministre|minister|staatssecretaris|rapporteur)(?:\s+[^,]+)?$")
            .unwrap()
    })
}

fn party_suffix_regex() -> &'static Regex {
    PARTY_SUFFIX.get_or_init(|| Regex::new(r"\(([^)]+)\)\s*$").unwrap())
}

fn article_fp_regex() -> &'static Regex {
    ARTICLE_FP.get_or_init(|| Regex::new(r"(?i)(?:artikel|article|articles|artikelen)").unwrap())
}

pub fn detect_turn_start(paragraph: &str) -> Option<(TurnStart, usize)> {
    let trimmed = paragraph.trim();
    if trimmed.is_empty() {
        return None;
    }

    let prefix: String = trimmed.chars().take(120).collect();
    if article_fp_regex().is_match(&prefix) {
        if turn_start_regex().find(trimmed).is_none() {
            return None;
        }
    }

    if let Some(cap) = turn_start_regex().captures(trimmed) {
        let full = cap.get(0).unwrap();
        return Some((
            TurnStart::Numbered {
                turn_number: cap["turn"].to_string(),
                raw_label: cap["label"].trim().to_string(),
            },
            full.end(),
        ));
    }

    if let Some(cap) = chair_generic_regex().find(trimmed) {
        return Some((TurnStart::GenericChair, cap.end()));
    }

    if let Some(cap) = chair_named_regex().captures(trimmed) {
        let full = cap.get(0).unwrap();
        return Some((
            TurnStart::NamedChair {
                raw_name: cap["name"].trim().to_string(),
            },
            full.end(),
        ));
    }

    None
}

pub fn parse_speaker_label(raw_label: &str, item_kind: &str) -> ParsedSpeaker {
    let mut label = raw_label.trim().to_string();
    label = titles_prefix_regex().replace(&label, "").trim().to_string();

    let party = party_suffix_regex()
        .captures(&label)
        .map(|c| c[1].trim().to_string());
    if party.is_some() {
        label = party_suffix_regex().replace(&label, "").trim().to_string();
    }

    let role_from_suffix = role_suffix_regex().is_match(&label);
    label = role_suffix_regex().replace(&label, "").trim().to_string();

    let speaker_role = if role_from_suffix
        || label.to_lowercase().contains("minist")
        || label.to_lowercase().contains("staatssecretaris")
    {
        SpeakerRole::Minister
    } else if party.is_some() {
        SpeakerRole::Mp
    } else if item_kind == "hearing" {
        SpeakerRole::External
    } else {
        SpeakerRole::Unknown
    };

    ParsedSpeaker {
        raw_speaker: if label.is_empty() {
            raw_label.trim().to_string()
        } else {
            label
        },
        speaker_role,
        party,
    }
}

pub fn parse_turn_start(start: &TurnStart, item_kind: &str) -> (String, SpeakerRole) {
    match start {
        TurnStart::Numbered { raw_label, .. } => {
            let parsed = parse_speaker_label(raw_label, item_kind);
            (parsed.raw_speaker, parsed.speaker_role)
        }
        TurnStart::GenericChair => ("Voorzitter".to_string(), SpeakerRole::Chair),
        TurnStart::NamedChair { raw_name } => (raw_name.clone(), SpeakerRole::Chair),
    }
}

pub fn language_from_class(class: Option<&str>) -> String {
    match class.unwrap_or("") {
        c if c.contains("FR") || c == "NormalFR" || c == "italFR" => "FR".to_string(),
        _ => "NL".to_string(),
    }
}

/// Agenda prefix and base turn index (`DD.MM` or `DD.MMD` intervention marker).
pub fn base_turn_parts(turn_number: &str) -> Option<(String, u32)> {
    let parts: Vec<&str> = turn_number.split('.').collect();
    if parts.len() != 2 || parts[0].len() != 2 {
        return None;
    }
    let agenda_id = parts[0].to_string();
    let base = match parts[1].len() {
        2 => parts[1].parse().ok()?,
        3 => parts[1][..2].parse().ok()?,
        _ => return None,
    };
    Some((agenda_id, base))
}

pub fn collect_base_turns_by_agenda(
    blocks: &[crate::report_blocks::ReportBlock],
) -> std::collections::HashMap<String, std::collections::BTreeSet<u32>> {
    use crate::report_blocks::BlockTag;
    use std::collections::{BTreeSet, HashMap};

    let mut by_agenda: HashMap<String, BTreeSet<u32>> = HashMap::new();
    for block in blocks {
        if block.tag != BlockTag::P {
            continue;
        }
        let Some((start, _)) = detect_turn_start(&block.text) else {
            continue;
        };
        let TurnStart::Numbered { turn_number, .. } = start else {
            continue;
        };
        let Some((agenda_id, base)) = base_turn_parts(&turn_number) else {
            continue;
        };
        by_agenda.entry(agenda_id).or_default().insert(base);
    }
    by_agenda
}

pub fn count_source_markers(blocks: &[crate::report_blocks::ReportBlock]) -> (usize, usize) {
    use crate::report_blocks::BlockTag;

    let mut turns = 0usize;
    let mut chairs = 0usize;
    for block in blocks {
        if block.tag != BlockTag::P {
            continue;
        }
        let Some((start, _)) = detect_turn_start(&block.text) else {
            continue;
        };
        match start {
            TurnStart::Numbered { .. } => turns += 1,
            TurnStart::GenericChair | TurnStart::NamedChair { .. } => chairs += 1,
        }
    }
    (turns, chairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_nbsp_turn() {
        let p = "01.01\u{00a0}Annick Ponthier (VB): Mijnheer de voorzitter";
        let (start, _) = detect_turn_start(p).unwrap();
        assert!(matches!(start, TurnStart::Numbered { .. }));
    }

    #[test]
    fn detects_empty_colon_turn() {
        let p = "02.44\u{00a0}Greet Daems (PVDA-PTB):";
        assert!(detect_turn_start(p).is_some());
    }

    #[test]
    fn detects_named_chair() {
        let p = "Denis Ducarme, voorzitter: Collega's";
        let (start, _) = detect_turn_start(p).unwrap();
        assert!(matches!(start, TurnStart::NamedChair { .. }));
    }

    #[test]
    fn intervention_turn_captures_full_marker() {
        let p = "02.150 Steven Coenegrachts (Open Vld): Mijnheer de voorzitter";
        let (start, _) = detect_turn_start(p).unwrap();
        match start {
            TurnStart::Numbered {
                turn_number,
                raw_label,
            } => {
                assert_eq!(turn_number, "02.150");
                let parsed = parse_speaker_label(&raw_label, "general_debate");
                assert_eq!(parsed.raw_speaker, "Steven Coenegrachts");
            }
            _ => panic!("expected numbered turn"),
        }
    }

    #[test]
    fn main_turn_without_intervention_digit() {
        let p = "02.15 Stefaan Van Hecke (Groen): Mijnheer de voorzitter";
        let (start, _) = detect_turn_start(p).unwrap();
        match start {
            TurnStart::Numbered {
                turn_number,
                raw_label,
            } => {
                assert_eq!(turn_number, "02.15");
                let parsed = parse_speaker_label(&raw_label, "general_debate");
                assert_eq!(parsed.raw_speaker, "Stefaan Van Hecke");
            }
            _ => panic!("expected numbered turn"),
        }
    }

    #[test]
    fn intervention_turn_02_110_not_digit_speaker() {
        let p = "02.110 Steven Vandeput (N-VA): Mijnheer de voorzitter";
        let (start, _) = detect_turn_start(p).unwrap();
        match start {
            TurnStart::Numbered {
                turn_number,
                raw_label,
            } => {
                assert_eq!(turn_number, "02.110");
                let parsed = parse_speaker_label(&raw_label, "general_debate");
                assert!(!parsed.raw_speaker.starts_with('0'));
                assert_eq!(parsed.raw_speaker, "Steven Vandeput");
            }
            _ => panic!("expected numbered turn"),
        }
    }

    #[test]
    fn plenary_48_intervention_turns_fixture() {
        use crate::meeting_report::extract_utterances_from_document;
        use crate::report_blocks::read_report_html;
        use scraper::Html;

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/plenary/56-48.html");
        if !path.exists() {
            return;
        }

        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let utterances = extract_utterances_from_document(
            &document,
            crate::agenda_timeline::MeetingKind::Plenary,
            56,
            48,
            "fixture",
            path.to_string_lossy().as_ref(),
        );

        let coenegrachts: Vec<_> = utterances
            .iter()
            .filter(|u| u.raw_speaker.contains("Coenegrachts"))
            .collect();
        assert!(
            coenegrachts.iter().all(|u| !u
                .raw_speaker
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())),
            "Coenegrachts speakers should not have digit prefix: {:?}",
            coenegrachts
                .iter()
                .map(|u| &u.raw_speaker)
                .collect::<Vec<_>>()
        );

        let turn_02_15 = utterances
            .iter()
            .find(|u| u.turn_number == "02.15")
            .map(|u| u.utterance_id.clone());
        let turn_02_150 = utterances
            .iter()
            .find(|u| u.turn_number == "02.150")
            .map(|u| u.utterance_id.clone());
        if let (Some(id_15), Some(id_150)) = (turn_02_15, turn_02_150) {
            assert_ne!(
                id_15, id_150,
                "intervention turn 02.150 must have distinct utterance_id from main turn 02.15"
            );
        }

        let vandeput_110 = utterances
            .iter()
            .find(|u| u.turn_number == "02.110" && u.raw_speaker.contains("Vandeput"));
        if let Some(u) = vandeput_110 {
            assert_eq!(u.raw_speaker, "Steven Vandeput");
        }
    }

    #[test]
    fn base_turn_parts_strips_intervention_digit() {
        assert_eq!(
            base_turn_parts("02.150"),
            Some(("02".to_string(), 15))
        );
        assert_eq!(base_turn_parts("02.15"), Some(("02".to_string(), 15)));
        assert_eq!(
            base_turn_parts("02.110"),
            Some(("02".to_string(), 11))
        );
    }

    #[test]
    fn collect_base_turns_groups_interventions() {
        use crate::report_blocks::BlockTag;

        let blocks = vec![
            crate::report_blocks::ReportBlock {
                index: 0,
                tag: BlockTag::P,
                text: "02.15 Stefaan Van Hecke (Groen): speech".into(),
                inlines: Vec::new(),
                table_rows: None,
                lang: None,
                class: None,
                has_oraspr: false,
                content_hash: String::new(),
                word_count: 5,
            },
            crate::report_blocks::ReportBlock {
                index: 1,
                tag: BlockTag::P,
                text: "02.150 Steven Coenegrachts (Open Vld): reply".into(),
                inlines: Vec::new(),
                table_rows: None,
                lang: None,
                class: None,
                has_oraspr: false,
                content_hash: String::new(),
                word_count: 5,
            },
            crate::report_blocks::ReportBlock {
                index: 2,
                tag: BlockTag::P,
                text: "02.17 Annick Ponthier (VB): next".into(),
                inlines: Vec::new(),
                table_rows: None,
                lang: None,
                class: None,
                has_oraspr: false,
                content_hash: String::new(),
                word_count: 5,
            },
        ];
        let turns = collect_base_turns_by_agenda(&blocks);
        let agenda_02 = turns.get("02").expect("agenda 02 turns");
        assert!(agenda_02.contains(&15));
        assert!(agenda_02.contains(&17));
        assert_eq!(agenda_02.len(), 2);
    }
}
