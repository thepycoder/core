use crate::agenda_timeline::{MeetingKind, build_agenda_timeline};
use crate::report_blocks::{parse_report_blocks, read_report_html};
use crate::utterance_segment::{UtteranceDraft, segment_utterances};
use scraper::Html;
use std::error::Error;
use std::path::Path;

pub fn extract_utterances_from_document(
    document: &Html,
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
) -> Vec<UtteranceDraft> {
    let blocks = parse_report_blocks(document);
    let agenda = build_agenda_timeline(&blocks, meeting_kind, session_id, meeting_id);
    extract_utterances_from_blocks(
        &blocks,
        &agenda,
        meeting_kind,
        session_id,
        meeting_id,
        source_url,
        cache_path,
    )
}

pub fn extract_utterances_from_blocks(
    blocks: &[crate::report_blocks::ReportBlock],
    agenda: &[crate::agenda_timeline::AgendaItem],
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
) -> Vec<UtteranceDraft> {
    segment_utterances(
        blocks,
        agenda,
        meeting_kind,
        session_id,
        meeting_id,
        source_url,
        cache_path,
    )
}

pub fn extract_utterances_from_cache(
    cache_path: &Path,
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_rel_path: &str,
) -> Result<Vec<UtteranceDraft>, Box<dyn Error>> {
    let html = read_report_html(cache_path)?;
    let document = Html::parse_document(&html);
    Ok(extract_utterances_from_document(
        &document,
        meeting_kind,
        session_id,
        meeting_id,
        source_url,
        cache_rel_path,
    ))
}
