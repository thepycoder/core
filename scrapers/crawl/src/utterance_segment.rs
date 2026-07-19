use crate::agenda_timeline::{AgendaItem, MeetingKind, agenda_item_for_block};
use crate::report_blocks::{BlockTag, ReportBlock};
use crate::speaker_parse::{
    SpeakerRole, TurnStart, detect_turn_start, language_from_class, parse_turn_start,
};
use crate::speech_zones::{is_hard_boundary, is_stage_direction, is_vote_appendix_heading};
use crate::utils::agenda_item_id;

#[derive(Debug, Clone)]
pub struct UtteranceDraft {
    pub utterance_id: String,
    pub session_id: u32,
    pub meeting_id: u32,
    pub meeting_kind: MeetingKind,
    pub agenda_id: String,
    pub agenda_item_id: String,
    pub turn_number: String,
    pub seq: u32,
    pub item_kind: String,
    pub item_id: String,
    pub question_ids: String,
    pub dossier_id: String,
    pub document_id: String,
    pub motion_id: String,
    pub vote_id: String,
    pub raw_speaker: String,
    pub speaker_role: String,
    pub text: String,
    pub language: String,
    pub block_start: u32,
    pub block_end: u32,
    pub source_section: String,
    pub source_url: String,
    pub cache_path: String,
}

struct OpenTurn {
    turn_number: String,
    raw_speaker: String,
    speaker_role: SpeakerRole,
    text: String,
    language: String,
    block_start: u32,
    block_end: u32,
    agenda_id: String,
    agenda_item_id: String,
    item_kind: String,
    item_id: String,
    question_ids: String,
    dossier_id: String,
    document_id: String,
    source_section: String,
}

pub fn segment_utterances(
    blocks: &[ReportBlock],
    agenda: &[AgendaItem],
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
) -> Vec<UtteranceDraft> {
    let mut utterances = Vec::new();
    let mut open: Option<OpenTurn> = None;
    let mut seq = 0u32;
    let mut in_vote_appendix = false;

    for block in blocks {
        if block.tag == BlockTag::Table || block.tag == BlockTag::H1 || block.tag == BlockTag::H2 {
            if let Some(turn) = open.take() {
                push_turn(
                    &mut utterances,
                    turn,
                    meeting_kind,
                    session_id,
                    meeting_id,
                    source_url,
                    cache_path,
                    &mut seq,
                );
            }
            if block.tag == BlockTag::H1 && is_vote_appendix_heading(&block.text) {
                in_vote_appendix = true;
            }
            continue;
        }

        if block.tag != BlockTag::P {
            continue;
        }

        if in_vote_appendix || is_vote_appendix_heading(&block.text) {
            in_vote_appendix = true;
            if let Some(turn) = open.take() {
                push_turn(
                    &mut utterances,
                    turn,
                    meeting_kind,
                    session_id,
                    meeting_id,
                    source_url,
                    cache_path,
                    &mut seq,
                );
            }
            continue;
        }

        if is_hard_boundary(&block.text) {
            if let Some(turn) = open.take() {
                push_turn(
                    &mut utterances,
                    turn,
                    meeting_kind,
                    session_id,
                    meeting_id,
                    source_url,
                    cache_path,
                    &mut seq,
                );
            }
            continue;
        }

        if is_stage_direction(block.text.trim()) {
            continue;
        }

        let item = agenda_item_for_block(agenda, block.index);
        let item_kind = item
            .map(|i| i.item_kind.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        if let Some((start, content_start)) = detect_turn_start(&block.text) {
            if let Some(turn) = open.take() {
                push_turn(
                    &mut utterances,
                    turn,
                    meeting_kind,
                    session_id,
                    meeting_id,
                    source_url,
                    cache_path,
                    &mut seq,
                );
            }

            let (raw_speaker, speaker_role) = parse_turn_start(&start, &item_kind);
            let turn_number = match &start {
                TurnStart::Numbered { turn_number, .. } => turn_number.clone(),
                _ => String::new(),
            };
            let agenda_id = turn_agenda_id(&turn_number, item);
            let stamped_agenda_item_id = item
                .map(|i| {
                    agenda_item_id(session_id, meeting_kind.as_str(), meeting_id, i.start_block)
                })
                .unwrap_or_default();
            let text = block.text[content_start..].trim().to_string();

            open = Some(OpenTurn {
                turn_number,
                raw_speaker,
                speaker_role,
                text,
                language: language_from_class(block.class.as_deref()),
                block_start: block.index,
                block_end: block.index,
                agenda_id: agenda_id.clone(),
                agenda_item_id: stamped_agenda_item_id,
                item_kind: item_kind.clone(),
                item_id: item.map(|i| i.item_id.clone()).unwrap_or_default(),
                question_ids: item.map(|i| i.internal_ids.join(",")).unwrap_or_default(),
                dossier_id: item.map(|i| i.dossier_id.clone()).unwrap_or_default(),
                document_id: item.map(|i| i.document_id.clone()).unwrap_or_default(),
                source_section: item.map(|i| i.source_section.clone()).unwrap_or_default(),
            });
        } else if let Some(turn) = open.as_mut() {
            if !block.text.trim().is_empty() {
                if !turn.text.is_empty() {
                    turn.text.push(' ');
                }
                turn.text.push_str(block.text.trim());
                turn.block_end = block.index;
            }
        }
    }

    if let Some(turn) = open.take() {
        push_turn(
            &mut utterances,
            turn,
            meeting_kind,
            session_id,
            meeting_id,
            source_url,
            cache_path,
            &mut seq,
        );
    }

    dedupe_bilingual_turns(&mut utterances);
    utterances
}

fn turn_agenda_id(turn_number: &str, item: Option<&AgendaItem>) -> String {
    if let Some(item) = item {
        return item.agenda_id.clone();
    }
    turn_number.split('.').next().unwrap_or("00").to_string()
}

fn push_turn(
    utterances: &mut Vec<UtteranceDraft>,
    turn: OpenTurn,
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
    seq: &mut u32,
) {
    if turn.text.trim().is_empty() && turn.raw_speaker.is_empty() {
        return;
    }

    let turn_key = if turn.turn_number.is_empty() {
        format!("chair_{}", *seq)
    } else {
        turn.turn_number.clone()
    };

    let turn_slug = turn_key.replace('.', "_");
    let utterance_id = format!(
        "{session_id}_{kind}_{meeting_id}_{agenda_id}_{turn_slug}",
        kind = meeting_kind.as_str(),
        agenda_id = turn.agenda_id,
    );

    *seq += 1;

    utterances.push(UtteranceDraft {
        utterance_id,
        session_id,
        meeting_id,
        meeting_kind,
        agenda_id: turn.agenda_id,
        agenda_item_id: turn.agenda_item_id,
        turn_number: turn.turn_number,
        seq: *seq,
        item_kind: turn.item_kind,
        item_id: turn.item_id,
        question_ids: turn.question_ids,
        dossier_id: turn.dossier_id,
        document_id: turn.document_id,
        motion_id: String::new(),
        vote_id: String::new(),
        raw_speaker: turn.raw_speaker,
        speaker_role: turn.speaker_role.as_str().to_string(),
        text: turn.text.trim().to_string(),
        language: turn.language,
        block_start: turn.block_start,
        block_end: turn.block_end,
        source_section: turn.source_section,
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
    });
}

fn dedupe_bilingual_turns(utterances: &mut Vec<UtteranceDraft>) {
    use std::collections::HashMap;
    let mut by_key: HashMap<(String, String, String), usize> = HashMap::new();
    let mut result: Vec<UtteranceDraft> = Vec::new();
    for u in utterances.drain(..) {
        let key = (
            u.agenda_id.clone(),
            u.turn_number.clone(),
            u.raw_speaker.to_lowercase(),
        );
        if let Some(&idx) = by_key.get(&key) {
            if u.text.len() > result[idx].text.len() {
                result[idx] = u;
            }
        } else {
            by_key.insert(key, result.len());
            result.push(u);
        }
    }
    *utterances = result;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agenda_timeline::build_agenda_timeline;
    use crate::report_blocks::{parse_report_blocks, read_report_html};

    #[test]
    fn segments_plenary_questions_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/plenary/56-117.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = scraper::Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        let agenda = build_agenda_timeline(&blocks, MeetingKind::Plenary, 56, 117);
        let utterances = segment_utterances(
            &blocks,
            &agenda,
            MeetingKind::Plenary,
            56,
            117,
            "url",
            "cache",
        );
        assert!(utterances.len() > 50);
        assert!(utterances.iter().all(|u| u.seq > 0));
    }

    #[test]
    fn stamps_agenda_item_id_matching_timeline_start_block() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/plenary/56-42.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = scraper::Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        let agenda = build_agenda_timeline(&blocks, MeetingKind::Plenary, 56, 42);
        let utterances = segment_utterances(
            &blocks,
            &agenda,
            MeetingKind::Plenary,
            56,
            42,
            "url",
            "cache",
        );
        let with_dossier: Vec<_> = utterances
            .iter()
            .filter(|u| u.dossier_id == "56/318")
            .collect();
        assert!(
            !with_dossier.is_empty(),
            "expected utterances under dossier 56/318"
        );
        for u in &with_dossier {
            assert!(
                u.agenda_item_id.starts_with("56_plenary_42_agenda_"),
                "got {}",
                u.agenda_item_id
            );
            let start: u32 = u
                .agenda_item_id
                .rsplit('_')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            let item = agenda
                .iter()
                .find(|a| a.start_block == start)
                .expect("agenda item for stamped id");
            assert_eq!(item.dossier_id, "56/318");
        }
    }
}
