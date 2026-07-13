//! Unified meeting report parse: blocks, votes, derived rows, and provenance spans.

use crate::agenda_timeline::{AgendaItem, MeetingKind, build_agenda_timeline};
use crate::answer_io::AnswerDraft;
use crate::artifact_id::{
    BLOCK_PARSER_VERSION, MEETING_SCOPE_EXTRACTOR_VERSION, VOTE_EXTRACTOR_VERSION, artifact_id,
};
use crate::meeting_report::extract_utterances_from_blocks;
use crate::proceeding_entities::{
    HearingDraft, InterpellationDraft, extract_proceedings_from_agenda,
};
use crate::report_blocks::{ReportBlock, parse_report_blocks};
use crate::report_blocks_io::{ReportBlockRow, materialize_report_blocks};
use crate::source_spans::{SourceSpanDraft, span_id, validate_source_spans};
use crate::utterance_segment::UtteranceDraft;
use crate::vote_assembly::assemble_votes_from_blocks;
use crate::vote_types::{SpanEvidence, VoteAssemblyOutput};
use crate::written_oral_qa::{
    OralWrittenItem, extract_written_oral_items, oral_written_answer_drafts,
};
use scraper::Html;

pub struct MeetingParseOutput {
    pub blocks: Vec<ReportBlock>,
    pub agenda: Vec<AgendaItem>,
    pub utterances: Vec<UtteranceDraft>,
    pub hearings: Vec<HearingDraft>,
    pub interpellations: Vec<InterpellationDraft>,
    pub oral_written_items: Vec<OralWrittenItem>,
    pub answers: Vec<AnswerDraft>,
    pub votes: VoteAssemblyOutput,
    pub report_block_rows: Vec<ReportBlockRow>,
    /// Contains both valid and unresolved rows; inspect `validation_status`.
    pub source_spans: Vec<SourceSpanDraft>,
}

pub fn parse_plenary_meeting_report(
    document: &Html,
    session_id: u32,
    meeting_id: u32,
    date: &str,
    source_url: &str,
    cache_path: &str,
    source_content_hash: &str,
) -> MeetingParseOutput {
    let blocks = parse_report_blocks(document);
    let artifact = artifact_id(source_url, cache_path);
    let agenda = build_agenda_timeline(&blocks, MeetingKind::Plenary, session_id, meeting_id);
    let utterances = extract_utterances_from_blocks(
        &blocks,
        &agenda,
        MeetingKind::Plenary,
        session_id,
        meeting_id,
        source_url,
        cache_path,
    );
    let (hearings, interpellations) = extract_proceedings_from_agenda(
        &agenda,
        MeetingKind::Plenary,
        session_id,
        meeting_id,
        source_url,
        cache_path,
    );
    let oral_written_items =
        extract_written_oral_items(&blocks, MeetingKind::Plenary, session_id, meeting_id);
    let answers = oral_written_answer_drafts(
        &oral_written_items,
        MeetingKind::Plenary,
        session_id,
        meeting_id,
        source_url,
        cache_path,
    );
    let votes = assemble_votes_from_blocks(
        &blocks, session_id, meeting_id, date, source_url, cache_path,
    );
    let report_block_rows = materialize_report_blocks(
        &artifact,
        source_content_hash,
        &blocks,
        source_url,
        cache_path,
    );
    let mut span_candidates = assembly_spans(
        &artifact,
        source_content_hash,
        session_id,
        meeting_id,
        &votes,
        source_url,
        cache_path,
    );
    span_candidates.extend(semantic_spans(
        &artifact,
        source_content_hash,
        session_id,
        meeting_id,
        &blocks,
        &agenda,
        &utterances,
        &hearings,
        &interpellations,
        &oral_written_items,
        &answers,
        source_url,
        cache_path,
    ));
    let validated = validate_source_spans(
        span_candidates,
        &artifact,
        source_content_hash,
        BLOCK_PARSER_VERSION,
        blocks.len() as u32,
    );
    MeetingParseOutput {
        blocks,
        agenda,
        utterances,
        hearings,
        interpellations,
        oral_written_items,
        answers,
        votes,
        report_block_rows,
        source_spans: validated.rows,
    }
}

fn assembly_spans(
    artifact: &str,
    source_content_hash: &str,
    session_id: u32,
    meeting_id: u32,
    votes: &VoteAssemblyOutput,
    source_url: &str,
    cache_path: &str,
) -> Vec<SourceSpanDraft> {
    let mut spans = Vec::new();
    for evidence in &votes.span_evidence {
        spans.push(span_from_evidence(
            artifact,
            source_content_hash,
            session_id,
            meeting_id,
            evidence,
            &evidence.entity_id,
            source_url,
            cache_path,
        ));
    }
    spans
}

fn span_from_evidence(
    artifact: &str,
    source_content_hash: &str,
    session_id: u32,
    meeting_id: u32,
    evidence: &SpanEvidence,
    entity_id: &str,
    source_url: &str,
    cache_path: &str,
) -> SourceSpanDraft {
    SourceSpanDraft {
        span_id: span_id(
            artifact,
            &evidence.entity_type,
            entity_id,
            &evidence.span_role,
            evidence.block_start,
            evidence.block_end,
        ),
        artifact_id: artifact.to_string(),
        source_content_hash: source_content_hash.to_string(),
        session_id,
        meeting_id,
        entity_type: evidence.entity_type.clone(),
        entity_id: entity_id.to_string(),
        span_role: evidence.span_role.clone(),
        block_start: evidence.block_start,
        block_end: evidence.block_end,
        coverage_kind: evidence.coverage_kind.clone(),
        field_names: evidence.field_names.clone(),
        confidence: 1.0,
        extractor: "vote_assembly".to_string(),
        block_parser_version: BLOCK_PARSER_VERSION.to_string(),
        extractor_version: VOTE_EXTRACTOR_VERSION.to_string(),
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
        validation_status: String::new(),
        unresolved_reason: String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn semantic_spans(
    artifact: &str,
    source_content_hash: &str,
    session_id: u32,
    meeting_id: u32,
    blocks: &[ReportBlock],
    agenda: &[AgendaItem],
    utterances: &[UtteranceDraft],
    hearings: &[HearingDraft],
    interpellations: &[InterpellationDraft],
    oral_written_items: &[OralWrittenItem],
    answers: &[AnswerDraft],
    source_url: &str,
    cache_path: &str,
) -> Vec<SourceSpanDraft> {
    let mut spans = vec![scope_span(
        artifact,
        source_content_hash,
        session_id,
        meeting_id,
        "Meeting",
        &format!("plenary_{session_id}_{meeting_id}"),
        "meeting_scope",
        0,
        blocks.len() as u32,
        source_url,
        cache_path,
    )];
    for item in agenda {
        if matches!(
            item.item_kind,
            crate::agenda_timeline::ItemKind::Proposition
                | crate::agenda_timeline::ItemKind::Notice
        ) {
            continue;
        }
        let entity_type = match item.item_kind {
            crate::agenda_timeline::ItemKind::Question => "Question",
            crate::agenda_timeline::ItemKind::Hearing => "Hearing",
            crate::agenda_timeline::ItemKind::Interpellation => "Interpellation",
            crate::agenda_timeline::ItemKind::Proposition => "Proposition",
            crate::agenda_timeline::ItemKind::Notice => "Notice",
            _ => "AgendaItem",
        };
        let entity_id = if item.item_id.is_empty() {
            format!(
                "{session_id}_plenary_{meeting_id}_agenda_{}",
                item.start_block
            )
        } else {
            item.item_id.clone()
        };
        spans.push(scope_span(
            artifact,
            source_content_hash,
            session_id,
            meeting_id,
            entity_type,
            &entity_id,
            "agenda_item_scope",
            item.start_block,
            item.end_block,
            source_url,
            cache_path,
        ));
        for &block_index in &item.title_blocks {
            spans.push(extraction_span(
                artifact,
                source_content_hash,
                session_id,
                meeting_id,
                entity_type,
                &entity_id,
                "entity_title",
                block_index,
                block_index + 1,
                "title_nl,title_fr,agenda_id,internal_ids,dossier_id,document_id",
                source_url,
                cache_path,
            ));
            if item.item_kind == crate::agenda_timeline::ItemKind::Question
                && question_heading_has_respondent(item, blocks, block_index)
            {
                spans.push(extraction_span(
                    artifact,
                    source_content_hash,
                    session_id,
                    meeting_id,
                    "Question",
                    &entity_id,
                    "question_participants",
                    block_index,
                    block_index + 1,
                    "questioners,respondents",
                    source_url,
                    cache_path,
                ));
            }
        }
    }
    for hearing in hearings {
        if let Some(item) = agenda
            .iter()
            .find(|item| item.item_id == hearing.hearing_id)
        {
            if !hearing.witnesses.is_empty() {
                emit_title_field_spans(
                    &mut spans,
                    item,
                    artifact,
                    source_content_hash,
                    session_id,
                    meeting_id,
                    "Hearing",
                    &hearing.hearing_id,
                    "hearing_participants",
                    "witnesses",
                    source_url,
                    cache_path,
                );
            }
            spans.push(scope_span(
                artifact,
                source_content_hash,
                session_id,
                meeting_id,
                "Hearing",
                &hearing.hearing_id,
                "hearing_body",
                item.start_block,
                item.end_block,
                source_url,
                cache_path,
            ));
        }
    }
    for interpellation in interpellations {
        if let Some(item) = agenda
            .iter()
            .find(|item| item.item_id == interpellation.interpellation_id)
        {
            if !interpellation.interpellators.is_empty() || !interpellation.respondents.is_empty() {
                emit_title_field_spans(
                    &mut spans,
                    item,
                    artifact,
                    source_content_hash,
                    session_id,
                    meeting_id,
                    "Interpellation",
                    &interpellation.interpellation_id,
                    "interpellation_participants",
                    "interpellators,respondents",
                    source_url,
                    cache_path,
                );
            }
            spans.push(scope_span(
                artifact,
                source_content_hash,
                session_id,
                meeting_id,
                "Interpellation",
                &interpellation.interpellation_id,
                "interpellation_body",
                item.start_block,
                item.end_block,
                source_url,
                cache_path,
            ));
        }
    }
    for utterance in utterances {
        spans.push(extraction_span(
            artifact,
            source_content_hash,
            session_id,
            meeting_id,
            "Utterance",
            &utterance.utterance_id,
            "utterance_text",
            utterance.block_start,
            utterance.block_end.saturating_add(1),
            "text,raw_speaker,speaker_role,language",
            source_url,
            cache_path,
        ));
    }
    for item in oral_written_items {
        for &block_index in &item.question_blocks {
            spans.push(extraction_span(
                artifact,
                source_content_hash,
                session_id,
                meeting_id,
                "Question",
                &item.question_id,
                "question_body",
                block_index,
                block_index + 1,
                "question_body_nl,question_body_fr,treatment_mode",
                source_url,
                cache_path,
            ));
        }
        let answer_id = answers
            .iter()
            .find(|answer| answer.question_id == item.question_id)
            .map(|answer| answer.answer_id.as_str())
            .unwrap_or("");
        for &block_index in &item.answer_blocks {
            spans.push(extraction_span(
                artifact,
                source_content_hash,
                session_id,
                meeting_id,
                "Answer",
                answer_id,
                "answer_text",
                block_index,
                block_index + 1,
                "text_nl,text_fr",
                source_url,
                cache_path,
            ));
        }
    }
    spans.extend(plenary_heading_entity_spans(
        artifact,
        source_content_hash,
        session_id,
        meeting_id,
        blocks,
        source_url,
        cache_path,
    ));
    spans
}

fn question_heading_has_respondent(
    item: &AgendaItem,
    blocks: &[ReportBlock],
    block_index: u32,
) -> bool {
    blocks
        .get(block_index as usize)
        .is_some_and(|block| block.text.contains(" aan ") || block.text.contains(" à "))
        || item.title_nl.contains(" aan ")
        || item.title_fr.contains(" à ")
}

#[allow(clippy::too_many_arguments)]
fn emit_title_field_spans(
    spans: &mut Vec<SourceSpanDraft>,
    item: &AgendaItem,
    artifact: &str,
    source_content_hash: &str,
    session_id: u32,
    meeting_id: u32,
    entity_type: &str,
    entity_id: &str,
    role: &str,
    fields: &str,
    source_url: &str,
    cache_path: &str,
) {
    for &block_index in &item.title_blocks {
        spans.push(extraction_span(
            artifact,
            source_content_hash,
            session_id,
            meeting_id,
            entity_type,
            entity_id,
            role,
            block_index,
            block_index + 1,
            fields,
            source_url,
            cache_path,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn plenary_heading_entity_spans(
    artifact: &str,
    source_content_hash: &str,
    session_id: u32,
    meeting_id: u32,
    blocks: &[ReportBlock],
    source_url: &str,
    cache_path: &str,
) -> Vec<SourceSpanDraft> {
    use crate::report_blocks::BlockTag;
    use crate::utils::composite_id;

    let mut spans = Vec::new();
    for (entity_type, section_terms) in [
        ("Proposition", &["voorstel", "wetsvoorstel"][..]),
        ("Notice", &["mededeling", "mededelingen"][..]),
    ] {
        let body_role = if entity_type == "Proposition" {
            "proposition_body"
        } else {
            "notice_body"
        };
        let mut in_section = false;
        let mut seen_section = false;
        let mut groups: Vec<Vec<u32>> = Vec::new();
        let mut current_number: Option<String> = None;
        for block in blocks {
            if block.tag == BlockTag::H1 {
                let lower = block.text.to_lowercase();
                let matches = section_terms.iter().any(|term| lower.contains(term));
                if matches {
                    in_section = true;
                    seen_section = true;
                } else if seen_section
                    && !(entity_type == "Proposition" && lower.contains("proposition"))
                    && !(entity_type == "Notice" && lower.contains("communication"))
                {
                    break;
                }
                continue;
            }
            if !in_section || block.tag != BlockTag::H2 {
                continue;
            }
            let number = crate::agenda_timeline::extract_agenda_number(&block.text);
            if number.is_some() && number != current_number {
                current_number = number;
                groups.push(vec![block.index]);
            } else if let Some(group) = groups.last_mut() {
                group.push(block.index);
            }
        }
        let mut seq = 0i32;
        for group in groups {
            let half = group.len() / 2;
            for (&nl_block, &fr_block) in group[..half].iter().zip(group[half..].iter()) {
                let entity_id = composite_id(session_id, meeting_id, seq);
                seq += 1;
                for block_index in [nl_block, fr_block] {
                    spans.push(extraction_span(
                        artifact,
                        source_content_hash,
                        session_id,
                        meeting_id,
                        entity_type,
                        &entity_id,
                        "entity_title",
                        block_index,
                        block_index + 1,
                        "title_nl,title_fr,dossier_id,document_id",
                        source_url,
                        cache_path,
                    ));
                }
                spans.push(scope_span(
                    artifact,
                    source_content_hash,
                    session_id,
                    meeting_id,
                    entity_type,
                    &entity_id,
                    body_role,
                    nl_block.min(fr_block),
                    nl_block.max(fr_block) + 1,
                    source_url,
                    cache_path,
                ));
            }
        }
    }
    spans
}

fn scope_span(
    artifact: &str,
    source_content_hash: &str,
    session_id: u32,
    meeting_id: u32,
    entity_type: &str,
    entity_id: &str,
    role: &str,
    start: u32,
    end: u32,
    source_url: &str,
    cache_path: &str,
) -> SourceSpanDraft {
    SourceSpanDraft {
        span_id: span_id(artifact, entity_type, entity_id, role, start, end),
        artifact_id: artifact.to_string(),
        source_content_hash: source_content_hash.to_string(),
        session_id,
        meeting_id,
        entity_type: entity_type.to_string(),
        entity_id: entity_id.to_string(),
        span_role: role.to_string(),
        block_start: start,
        block_end: end,
        coverage_kind: "scope".to_string(),
        field_names: String::new(),
        confidence: 0.5,
        extractor: "meeting_parse".to_string(),
        block_parser_version: BLOCK_PARSER_VERSION.to_string(),
        extractor_version: MEETING_SCOPE_EXTRACTOR_VERSION.to_string(),
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
        validation_status: String::new(),
        unresolved_reason: String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn extraction_span(
    artifact: &str,
    source_content_hash: &str,
    session_id: u32,
    meeting_id: u32,
    entity_type: &str,
    entity_id: &str,
    role: &str,
    start: u32,
    end: u32,
    fields: &str,
    source_url: &str,
    cache_path: &str,
) -> SourceSpanDraft {
    let mut span = scope_span(
        artifact,
        source_content_hash,
        session_id,
        meeting_id,
        entity_type,
        entity_id,
        role,
        start,
        end,
        source_url,
        cache_path,
    );
    span.coverage_kind = "extraction".to_string();
    span.field_names = fields.to_string();
    span.confidence = 1.0;
    span
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report_blocks::read_report_html;
    use scraper::Html;
    use std::path::Path;

    fn parse_fixture(name: &str) -> MeetingParseOutput {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/votes")
            .join(name);
        let html = read_report_html(&path).unwrap();
        parse_plenary_meeting_report(
            &Html::parse_document(&html),
            56,
            60,
            "2020-01-01",
            "https://example.test",
            "sessions/56/meetings/plenary/56-60.html",
            &crate::content_hash(&html),
        )
    }

    #[test]
    fn emits_vote_spans_from_assembly_evidence() {
        let parsed = parse_fixture("roll_call_compact.html");
        assert!(
            parsed
                .source_spans
                .iter()
                .any(|s| s.entity_type == "VoteResult")
        );
        assert!(
            parsed
                .source_spans
                .iter()
                .any(|s| s.coverage_kind == "extraction")
        );
    }

    #[test]
    fn bilingual_disjoint_titles_emit_exact_one_block_spans() {
        let html = r#"
            <h1>Mondelinge vragen</h1>
            <h2>01 Vraag van Jan aan de minister (56000001P) over hetzelfde</h2>
            <p>01.01 Jan: vraag.</p>
            <h2>01 Question de Jan au ministre (56000001P) sur le même sujet</h2>
        "#;
        let parsed = parse_plenary_meeting_report(
            &Html::parse_document(html),
            56,
            1,
            "2026-01-01",
            "url",
            "cache",
            &crate::content_hash(html),
        );
        let question = parsed
            .agenda
            .iter()
            .find(|item| item.item_kind == crate::agenda_timeline::ItemKind::Question)
            .unwrap();
        assert_eq!(question.title_blocks, vec![1, 3]);
        let spans: Vec<_> = parsed
            .source_spans
            .iter()
            .filter(|span| {
                span.entity_type == "Question"
                    && span.entity_id == question.item_id
                    && span.span_role == "entity_title"
            })
            .collect();
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].block_start, spans[0].block_end), (1, 2));
        assert_eq!((spans[1].block_start, spans[1].block_end), (3, 4));
        assert!(parsed.source_spans.iter().any(|span| {
            span.entity_type == "Question"
                && span.entity_id == question.item_id
                && span.span_role == "question_participants"
                && span.field_names == "questioners,respondents"
        }));
        assert!(
            parsed
                .source_spans
                .iter()
                .all(|span| span.validation_status == "valid")
        );
    }

    #[test]
    fn proceedings_emit_staged_participant_and_body_roles() {
        let html = r#"
            <h1>Hoorzittingen</h1>
            <h2>01 Hoorzitting met: Alice Expert</h2>
            <p>Alice Expert geeft een toelichting.</p>
            <h1>Interpellaties</h1>
            <h2>02 Interpellatie van Jan Jansen aan Minister X over "Onderwerp" (56000001I)</h2>
            <p>02.01 Jan Jansen: Mijn interpellatie.</p>
        "#;
        let parsed = parse_plenary_meeting_report(
            &Html::parse_document(html),
            56,
            2,
            "2026-01-01",
            "url",
            "cache",
            &crate::content_hash(html),
        );
        let hearing = parsed.hearings.first().expect("hearing");
        let interpellation = parsed.interpellations.first().expect("interpellation");
        for (entity_type, entity_id, participant_role, body_role) in [
            (
                "Hearing",
                hearing.hearing_id.as_str(),
                "hearing_participants",
                "hearing_body",
            ),
            (
                "Interpellation",
                interpellation.interpellation_id.as_str(),
                "interpellation_participants",
                "interpellation_body",
            ),
        ] {
            assert!(parsed.source_spans.iter().any(|span| {
                span.entity_type == entity_type
                    && span.entity_id == entity_id
                    && span.span_role == participant_role
                    && span.coverage_kind == "extraction"
            }));
            assert!(parsed.source_spans.iter().any(|span| {
                span.entity_type == entity_type
                    && span.entity_id == entity_id
                    && span.span_role == body_role
                    && span.coverage_kind == "scope"
            }));
        }
    }

    #[test]
    fn repeated_title_text_keeps_distinct_source_occurrences() {
        let html = r#"
            <h1>Voorstellen</h1>
            <h2><span>01</span><span>Zelfde titel (56/1)</span></h2>
            <h2><span>Zelfde titel (56/1)</span></h2>
        "#;
        let parsed = parse_plenary_meeting_report(
            &Html::parse_document(html),
            56,
            1,
            "2026-01-01",
            "url",
            "cache",
            &crate::content_hash(html),
        );
        let title_spans: Vec<_> = parsed
            .source_spans
            .iter()
            .filter(|span| span.entity_type == "Proposition" && span.span_role == "entity_title")
            .map(|span| (span.block_start, span.block_end))
            .collect();
        assert_eq!(title_spans, vec![(1, 2), (2, 3)]);
    }

    #[test]
    fn reuse_fixture_emits_result_reuse_span() {
        let parsed = parse_fixture("result_reuse.html");
        assert!(
            parsed
                .source_spans
                .iter()
                .any(|s| s.span_role == "result_reuse")
        );
    }

    #[test]
    fn artifact_id_is_stable_for_fixture() {
        let a = artifact_id(
            "https://example.test",
            "sessions/56/meetings/plenary/56-60.html",
        );
        let b = artifact_id(
            "https://example.test",
            "sessions/56/meetings/plenary/56-60.html",
        );
        assert_eq!(a, b);
        let parsed = parse_fixture("roll_call_compact.html");
        assert!(parsed.source_spans.iter().all(|s| s.artifact_id == a));
        assert!(
            parsed
                .source_spans
                .iter()
                .all(|s| !s.source_content_hash.is_empty())
        );
        assert!(
            parsed.report_block_rows.iter().all(|row| {
                row.source_content_hash == parsed.source_spans[0].source_content_hash
            })
        );
    }
}
