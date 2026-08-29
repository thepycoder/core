//! Block-native vote assembly: decisions, results, tallies, members, and span evidence.

use crate::report_blocks::{BlockTag, ReportBlock};
use crate::vote_events::{
    RollCallTableShape, parse_appendix_from_blocks, parse_roll_call_table,
    parse_secret_ballot_table,
};
use crate::vote_patterns::{
    VoteSectionKind, candidate_tally_re, dossier_ref_re, formal_outcome, formal_vote_begin_re,
    is_numbered_agenda_heading, is_sitting_standing_proposal, paragraph_vote_title,
    parse_paragraph_vote_number, parse_participation, quorum_failure_re, reuse_result_re,
    scan_formal_outcome_after, scan_reuse_marker, sitting_standing_outcome_re,
    votes_section_heading,
};
use crate::vote_types::{
    SpanEvidence, UnresolvedVoteEventDraft, VoteAssemblyOutput, VoteDecisionDraft, VoteResultDraft,
    VoteResultMemberDraft, VoteTallyDraft, composite_result_id, composite_vote_id,
};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

static VOTE_REGEX_1: OnceLock<Regex> = OnceLock::new();
static VOTE_REGEX_2: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Default, Clone)]
struct TitleRefs {
    title_nl: String,
    title_fr: String,
    dossier_id: String,
    document_id: String,
    motion_id: String,
    source_blocks: Vec<u32>,
}

pub fn assemble_votes_from_blocks(
    blocks: &[ReportBlock],
    session_id: u32,
    meeting_id: u32,
    date: &str,
    source_url: &str,
    cache_path: &str,
) -> VoteAssemblyOutput {
    let mut out = VoteAssemblyOutput::default();
    let mut section = VoteSectionKind::None;
    let mut formal_zone = false;
    let mut pending = TitleRefs::default();
    let mut vote_seq = 0u32;
    let mut result_seq = 0u32;
    let mut results_by_source: HashMap<String, Vec<String>> = HashMap::new();
    let mut appendix_parsed: HashMap<String, Vec<(u32, Vec<crate::vote_events::AppendixBucket>)>> =
        HashMap::new();
    let mut appendix_cursor: HashMap<String, usize> = HashMap::new();
    for (i, b) in blocks.iter().enumerate() {
        if let Some(num) = crate::vote_patterns::parse_appendix_vote_number(&b.text) {
            let buckets = parse_appendix_from_blocks(blocks, i, &num);
            appendix_parsed
                .entry(num)
                .or_default()
                .push((b.index, buckets));
        }
    }

    let mut sitting_standing_context = false;

    for (idx, block) in blocks.iter().enumerate() {
        // Mirror vote_inventory section tracking: promote from any block text, reset
        // only on non-vote H1. Meeting 16 ip016x: FR continuation H1 + prose cues.
        let heading = votes_section_heading(&block.text);
        if heading != VoteSectionKind::None {
            section = heading;
            if heading == VoteSectionKind::RollCall {
                formal_zone = true;
            }
        } else if block.tag == BlockTag::H1 {
            section = VoteSectionKind::None;
            formal_zone = false;
        }
        if block.tag == BlockTag::P && formal_vote_begin_re().is_match(&block.text) {
            formal_zone = true;
        }

        if block.tag == BlockTag::H1 {
            pending = TitleRefs::default();
            if section != VoteSectionKind::SecretBallot {
                sitting_standing_context = false;
            }
            continue;
        }

        if block.tag == BlockTag::H2 {
            formal_zone = false;
            sitting_standing_context = false;
            if section == VoteSectionKind::SecretBallot {
                pending.source_blocks.push(block.index);
                if block.class.as_deref() == Some("NormalFR")
                    || block
                        .lang
                        .as_deref()
                        .is_some_and(|lang| lang.starts_with("FR"))
                {
                    pending.title_fr = block.text.clone();
                } else {
                    pending.title_nl = block.text.clone();
                }
                merge_refs(&mut pending, &block.text);
            } else if section == VoteSectionKind::RollCall
                && !is_numbered_agenda_heading(&block.text)
            {
                // Exit roll-call zone on debate headings; keep zone for numbered agenda
                // items before a compact table — meeting 129 ip129x.
                section = VoteSectionKind::None;
            }
            continue;
        }

        if block.tag == BlockTag::P {
            if is_sitting_standing_proposal(&block.text) {
                sitting_standing_context = true;
                if pending.title_nl.is_empty() && pending.title_fr.is_empty() {
                    pending.source_blocks.push(block.index);
                    if block.class.as_deref() == Some("NormalFR")
                        || block
                            .lang
                            .as_deref()
                            .is_some_and(|lang| lang.starts_with("FR"))
                    {
                        pending.title_fr = block.text.clone();
                    } else {
                        pending.title_nl = block.text.clone();
                    }
                }
            }
            if paragraph_vote_title(&block.text) {
                pending.source_blocks.push(block.index);
                if block.class.as_deref() == Some("NormalFR")
                    || block.lang.as_deref().is_some_and(|l| l.starts_with("FR"))
                {
                    pending.title_fr = block.text.clone();
                    merge_refs(&mut pending, &block.text);
                } else {
                    pending.title_nl = block.text.clone();
                    merge_refs(&mut pending, &block.text);
                }
                continue;
            }

            if reuse_result_re().is_match(&block.text) {
                let (source_num, marker_idx) = scan_reuse_marker(blocks, idx, 6);
                let title = merge_titles(&pending, recover_title_between(blocks, 0, idx));
                let result_id = results_by_source
                    .get(&source_num)
                    .and_then(|ids| ids.last().cloned());
                if let Some(result_id) = result_id {
                    vote_seq += 1;
                    let vote_id = composite_vote_id(session_id, meeting_id, vote_seq);
                    let outcome = marker_idx
                        .map(|marker_idx| scan_formal_outcome_after(blocks, marker_idx, 4))
                        .unwrap_or_default();
                    out.decisions.push(build_decision(
                        session_id,
                        meeting_id,
                        date,
                        vote_seq,
                        &vote_id,
                        &result_id,
                        &title,
                        "roll_call",
                        "complete",
                        &outcome,
                        &source_num,
                        true,
                        source_url,
                        cache_path,
                    ));
                    push_title_spans(&mut out, &vote_id, &title);
                    push_span(
                        &mut out,
                        "Vote",
                        &vote_id,
                        "result_reuse",
                        block.index,
                        block.index + 1,
                        "extraction",
                        "result_id,reuses_result",
                    );
                    push_span(
                        &mut out,
                        "VoteResult",
                        &result_id,
                        "result_reuse_reference",
                        block.index,
                        block.index + 1,
                        "extraction",
                        "result_id",
                    );
                    pending = TitleRefs::default();
                } else {
                    push_unresolved(
                        &mut out,
                        session_id,
                        meeting_id,
                        "result_reuse",
                        &source_num,
                        block,
                        "reuse_without_matching_result",
                        source_url,
                        cache_path,
                    );
                }
                continue;
            }

            if sitting_standing_context
                && (sitting_standing_outcome_re().is_match(&block.text)
                    || (formal_outcome(&block.text).is_some()
                        && block.text.to_lowercase().contains("zitten en opstaan")))
            {
                result_seq += 1;
                let result_id = composite_result_id(session_id, meeting_id, result_seq);
                let outcome = formal_outcome(&block.text).unwrap_or("unknown").to_string();
                out.results.push(VoteResultDraft {
                    result_id: result_id.clone(),
                    session_id,
                    meeting_id,
                    seq: result_seq,
                    method: "sitting_standing".to_string(),
                    named: false,
                    status: "complete".to_string(),
                    outcome: outcome.clone(),
                    source_roll_call_number: String::new(),
                    source_url: source_url.to_string(),
                    cache_path: cache_path.to_string(),
                });
                push_span(
                    &mut out,
                    "VoteResult",
                    &result_id,
                    "formal_outcome",
                    block.index,
                    block.index + 1,
                    "extraction",
                    "outcome,method",
                );
                vote_seq += 1;
                let vote_id = composite_vote_id(session_id, meeting_id, vote_seq);
                out.decisions.push(build_decision(
                    session_id,
                    meeting_id,
                    date,
                    vote_seq,
                    &vote_id,
                    &result_id,
                    &pending,
                    "sitting_standing",
                    "complete",
                    &outcome,
                    "",
                    false,
                    source_url,
                    cache_path,
                ));
                push_title_spans(&mut out, &vote_id, &pending);
                pending = TitleRefs::default();
                sitting_standing_context = false;
                continue;
            }

            let in_roll_call_zone = section == VoteSectionKind::RollCall || formal_zone;
            if in_roll_call_zone && parse_paragraph_vote_number(&block.text).is_some() {
                let next_text = blocks.get(idx + 1).map(|b| b.text.as_str()).unwrap_or("");
                if quorum_failure_re().is_match(next_text)
                    || quorum_failure_re().is_match(&block.text)
                {
                    result_seq += 1;
                    let result_id = composite_result_id(session_id, meeting_id, result_seq);
                    let source_num = parse_paragraph_vote_number(&block.text).unwrap_or_default();
                    out.results.push(VoteResultDraft {
                        result_id: result_id.clone(),
                        session_id,
                        meeting_id,
                        seq: result_seq,
                        method: "roll_call".to_string(),
                        named: false,
                        status: "no_quorum".to_string(),
                        outcome: "failed".to_string(),
                        source_roll_call_number: source_num.clone(),
                        source_url: source_url.to_string(),
                        cache_path: cache_path.to_string(),
                    });
                    let quorum_block = if quorum_failure_re().is_match(next_text) {
                        blocks.get(idx + 1).unwrap()
                    } else {
                        block
                    };
                    push_span(
                        &mut out,
                        "VoteResult",
                        &result_id,
                        "quorum_statement",
                        quorum_block.index,
                        quorum_block.index + 1,
                        "extraction",
                        "status,outcome",
                    );
                    push_span(
                        &mut out,
                        "VoteResult",
                        &result_id,
                        "result_reference",
                        block.index,
                        block.index + 1,
                        "extraction",
                        "source_roll_call_number",
                    );
                    if let Some((participated, required)) = parse_participation(&quorum_block.text)
                    {
                        out.tallies.push(VoteTallyDraft {
                            result_id: result_id.clone(),
                            tally_kind: "participation".to_string(),
                            option_key: "participated".to_string(),
                            label_nl: "deelgenomen".to_string(),
                            label_fr: "ont participé".to_string(),
                            dimension: "overall".to_string(),
                            count: participated,
                            selected: false,
                        });
                        push_span(
                            &mut out,
                            "VoteResult",
                            &result_id,
                            "quorum_participation",
                            quorum_block.index,
                            quorum_block.index + 1,
                            "extraction",
                            "participated,required",
                        );
                        if let Some(required) = required {
                            out.tallies.push(VoteTallyDraft {
                                result_id: result_id.clone(),
                                tally_kind: "participation".to_string(),
                                option_key: "required".to_string(),
                                label_nl: "vereist quorum".to_string(),
                                label_fr: "quorum requis".to_string(),
                                dimension: "overall".to_string(),
                                count: required,
                                selected: false,
                            });
                        }
                    }
                    vote_seq += 1;
                    let vote_id = composite_vote_id(session_id, meeting_id, vote_seq);
                    out.decisions.push(build_decision(
                        session_id,
                        meeting_id,
                        date,
                        vote_seq,
                        &vote_id,
                        &result_id,
                        &pending,
                        "roll_call",
                        "no_quorum",
                        "failed",
                        &source_num,
                        false,
                        source_url,
                        cache_path,
                    ));
                    push_title_spans(&mut out, &vote_id, &pending);
                    results_by_source
                        .entry(source_num)
                        .or_default()
                        .push(result_id);
                    pending = TitleRefs::default();
                    if section != VoteSectionKind::RollCall {
                        formal_zone = false;
                    }
                    continue;
                }
            }
        }

        if block.tag == BlockTag::Table {
            // Meeting 97 ip097x: compact roll-call table after a second secret-ballot block.
            let roll_call_in_secret_zone =
                section == VoteSectionKind::SecretBallot && parse_roll_call_table(block).is_some();
            if section == VoteSectionKind::SecretBallot && !roll_call_in_secret_zone {
                let secret = parse_secret_ballot_table(block);
                if secret.voters.is_some() || secret.valid.is_some() {
                    result_seq += 1;
                    let result_id = composite_result_id(session_id, meeting_id, result_seq);
                    let outcome_evidence = infer_secret_outcome(blocks, idx);
                    let outcome = outcome_evidence
                        .as_ref()
                        .map(|(outcome, _)| outcome.clone())
                        .unwrap_or_default();
                    out.results.push(VoteResultDraft {
                        result_id: result_id.clone(),
                        session_id,
                        meeting_id,
                        seq: result_seq,
                        method: "secret_ballot".to_string(),
                        named: false,
                        status: "complete".to_string(),
                        outcome: outcome.clone(),
                        source_roll_call_number: String::new(),
                        source_url: source_url.to_string(),
                        cache_path: cache_path.to_string(),
                    });
                    push_span(
                        &mut out,
                        "VoteResult",
                        &result_id,
                        "secret_statistics",
                        block.index,
                        block.index + 1,
                        "extraction",
                        "method,status",
                    );
                    push_secret_tallies(&mut out.tallies, &result_id, &secret);
                    attach_secret_candidates_after_table(blocks, idx, &result_id, &mut out);
                    if let Some((_, outcome_block)) = outcome_evidence {
                        push_span(
                            &mut out,
                            "VoteResult",
                            &result_id,
                            "formal_outcome",
                            outcome_block,
                            outcome_block + 1,
                            "extraction",
                            "outcome",
                        );
                    }
                    vote_seq += 1;
                    let vote_id = composite_vote_id(session_id, meeting_id, vote_seq);
                    out.decisions.push(build_decision(
                        session_id,
                        meeting_id,
                        date,
                        vote_seq,
                        &vote_id,
                        &result_id,
                        &pending,
                        "secret_ballot",
                        "complete",
                        &outcome,
                        "",
                        false,
                        source_url,
                        cache_path,
                    ));
                    push_title_spans(&mut out, &vote_id, &pending);
                    pending = TitleRefs::default();
                }
                continue;
            }

            let in_roll_call_zone =
                section == VoteSectionKind::RollCall || formal_zone || roll_call_in_secret_zone;
            if !in_roll_call_zone {
                continue;
            }

            if let Some(parsed) = parse_roll_call_table(block) {
                result_seq += 1;
                let result_id = composite_result_id(session_id, meeting_id, result_seq);
                let method = match parsed.shape {
                    RollCallTableShape::Standard => "roll_call",
                    RollCallTableShape::LanguageGroup => "language_group_roll_call",
                };
                let outcome_evidence = infer_roll_call_outcome(blocks, idx);
                let outcome = outcome_evidence
                    .as_ref()
                    .map(|(outcome, _)| outcome.clone())
                    .unwrap_or_default();
                out.results.push(VoteResultDraft {
                    result_id: result_id.clone(),
                    session_id,
                    meeting_id,
                    seq: result_seq,
                    method: method.to_string(),
                    named: true,
                    status: "complete".to_string(),
                    outcome: outcome.clone(),
                    source_roll_call_number: parsed.source_number.clone(),
                    source_url: source_url.to_string(),
                    cache_path: cache_path.to_string(),
                });
                push_span(
                    &mut out,
                    "VoteResult",
                    &result_id,
                    "result_table",
                    block.index,
                    block.index + 1,
                    "extraction",
                    "method,status,outcome",
                );
                push_span(
                    &mut out,
                    "VoteResult",
                    &result_id,
                    "result_reference",
                    block.index,
                    block.index + 1,
                    "extraction",
                    "source_roll_call_number",
                );
                let count_role = if parsed.shape == RollCallTableShape::LanguageGroup {
                    "language_group_counts"
                } else {
                    "overall_counts"
                };
                push_span(
                    &mut out,
                    "VoteResult",
                    &result_id,
                    count_role,
                    block.index,
                    block.index + 1,
                    "extraction",
                    "yes,no,abstain",
                );
                if let Some((_, outcome_block)) = outcome_evidence {
                    push_span(
                        &mut out,
                        "VoteResult",
                        &result_id,
                        "formal_outcome",
                        outcome_block,
                        outcome_block + 1,
                        "extraction",
                        "outcome",
                    );
                }
                for bucket in &parsed.invalid_language_group_buckets {
                    push_unresolved(
                        &mut out,
                        session_id,
                        meeting_id,
                        "language_group_counts",
                        &parsed.source_number,
                        block,
                        &format!("language_group_sum_mismatch:{bucket}"),
                        source_url,
                        cache_path,
                    );
                }
                push_roll_call_tallies(&mut out.tallies, &result_id, &parsed);
                let occurrence = appendix_cursor
                    .entry(parsed.source_number.clone())
                    .or_default();
                if let Some((header_block, buckets)) = appendix_parsed
                    .get(&parsed.source_number)
                    .and_then(|occurrences| occurrences.get(*occurrence))
                {
                    push_span(
                        &mut out,
                        "VoteResult",
                        &result_id,
                        "appendix_header",
                        *header_block,
                        *header_block + 1,
                        "extraction",
                        "source_roll_call_number",
                    );
                    for bucket in buckets {
                        push_span(
                            &mut out,
                            "VoteResult",
                            &result_id,
                            "appendix_bucket_count",
                            bucket.count_block,
                            bucket.count_block + 1,
                            "extraction",
                            "position,count",
                        );
                        for &name_block in &bucket.name_blocks {
                            push_span(
                                &mut out,
                                "VoteResult",
                                &result_id,
                                "appendix_voter_names",
                                name_block,
                                name_block + 1,
                                "extraction",
                                "position,raw_name",
                            );
                        }
                        for (seq, name) in bucket.names.iter().enumerate() {
                            out.members.push(VoteResultMemberDraft {
                                result_id: result_id.clone(),
                                position: bucket.position.clone(),
                                seq: seq as u32,
                                raw_name: name.clone(),
                            });
                        }
                    }
                }
                *occurrence += 1;
                results_by_source
                    .entry(parsed.source_number.clone())
                    .or_default()
                    .push(result_id.clone());
                vote_seq += 1;
                let vote_id = composite_vote_id(session_id, meeting_id, vote_seq);
                out.decisions.push(build_decision(
                    session_id,
                    meeting_id,
                    date,
                    vote_seq,
                    &vote_id,
                    &result_id,
                    &pending,
                    method,
                    "complete",
                    &outcome,
                    &parsed.source_number,
                    false,
                    source_url,
                    cache_path,
                ));
                push_title_spans(&mut out, &vote_id, &pending);
                pending = TitleRefs::default();
                if section != VoteSectionKind::RollCall {
                    formal_zone = false;
                }
            }
        }
    }

    discard_untitled_decisions(&mut out, session_id, meeting_id);
    out
}

fn discard_untitled_decisions(out: &mut VoteAssemblyOutput, session_id: u32, meeting_id: u32) {
    let mut retained_ids = HashMap::new();
    let mut retained = Vec::new();
    for mut decision in std::mem::take(&mut out.decisions) {
        if decision.title_nl.trim().is_empty() && decision.title_fr.trim().is_empty() {
            continue;
        }
        let previous_id = decision.vote_id.clone();
        decision.seq = retained.len() as u32 + 1;
        decision.vote_id = composite_vote_id(session_id, meeting_id, decision.seq);
        retained_ids.insert(previous_id, decision.vote_id.clone());
        retained.push(decision);
    }
    out.decisions = retained;

    out.span_evidence
        .retain(|span| span.entity_type != "Vote" || retained_ids.contains_key(&span.entity_id));
    for span in &mut out.span_evidence {
        if span.entity_type == "Vote" {
            span.entity_id = retained_ids[&span.entity_id].clone();
        }
    }
}

fn push_span(
    out: &mut VoteAssemblyOutput,
    entity_type: &str,
    entity_id: &str,
    role: &str,
    start: u32,
    end: u32,
    coverage_kind: &str,
    fields: &str,
) {
    out.span_evidence.push(SpanEvidence {
        entity_type: entity_type.to_string(),
        entity_id: entity_id.to_string(),
        span_role: role.to_string(),
        block_start: start,
        block_end: end,
        coverage_kind: coverage_kind.to_string(),
        field_names: fields.to_string(),
    });
}

#[allow(clippy::too_many_arguments)]
fn push_unresolved(
    out: &mut VoteAssemblyOutput,
    session_id: u32,
    meeting_id: u32,
    event_kind: &str,
    source_number: &str,
    block: &ReportBlock,
    reason: &str,
    source_url: &str,
    cache_path: &str,
) {
    out.unresolved_events.push(UnresolvedVoteEventDraft {
        session_id,
        meeting_id,
        event_kind: event_kind.to_string(),
        source_roll_call_number: source_number.to_string(),
        block_start: block.index,
        block_end: block.index + 1,
        reason: reason.to_string(),
        evidence_text: block.text.clone(),
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
    });
}

fn merge_titles(pending: &TitleRefs, recovered: TitleRefs) -> TitleRefs {
    let mut source_blocks = recovered.source_blocks;
    source_blocks.extend(pending.source_blocks.iter().copied());
    source_blocks.sort_unstable();
    source_blocks.dedup();
    TitleRefs {
        title_nl: if pending.title_nl.is_empty() {
            recovered.title_nl
        } else {
            pending.title_nl.clone()
        },
        title_fr: if pending.title_fr.is_empty() {
            recovered.title_fr
        } else {
            pending.title_fr.clone()
        },
        dossier_id: if pending.dossier_id.is_empty() {
            recovered.dossier_id
        } else {
            pending.dossier_id.clone()
        },
        document_id: if pending.document_id.is_empty() {
            recovered.document_id
        } else {
            pending.document_id.clone()
        },
        motion_id: if pending.motion_id.is_empty() {
            recovered.motion_id
        } else {
            pending.motion_id.clone()
        },
        source_blocks,
    }
}

fn recover_title_between(blocks: &[ReportBlock], from: usize, to: usize) -> TitleRefs {
    let mut refs = TitleRefs::default();
    for block in blocks.iter().skip(from).take(to.saturating_sub(from)) {
        if block.tag == BlockTag::P && paragraph_vote_title(&block.text) {
            refs.source_blocks.push(block.index);
            if block.class.as_deref() == Some("NormalFR")
                || block.lang.as_deref().is_some_and(|l| l.starts_with("FR"))
            {
                refs.title_fr = block.text.clone();
            } else {
                refs.title_nl = block.text.clone();
            }
            merge_refs(&mut refs, &block.text);
        }
    }
    refs
}

fn push_title_spans(out: &mut VoteAssemblyOutput, vote_id: &str, title: &TitleRefs) {
    for &block_index in &title.source_blocks {
        push_span(
            out,
            "Vote",
            vote_id,
            "decision_title",
            block_index,
            block_index + 1,
            "extraction",
            "title_nl,title_fr,dossier_id,document_id,motion_id",
        );
    }
}

fn attach_secret_candidates_after_table(
    blocks: &[ReportBlock],
    table_idx: usize,
    result_id: &str,
    out: &mut VoteAssemblyOutput,
) {
    for block in blocks.iter().skip(table_idx + 1).take(12) {
        if block.tag == BlockTag::Table || block.tag == BlockTag::H1 || block.tag == BlockTag::H2 {
            break;
        }
        for caps in candidate_tally_re().captures_iter(&block.text) {
            let name = caps[1].trim().to_string();
            let count: u32 = caps[2].parse().unwrap_or(0);
            let key = format!("candidate_{name}");
            if out
                .tallies
                .iter()
                .any(|t| t.result_id == result_id && t.option_key == key)
            {
                continue;
            }
            let selected = block.text.to_lowercase().contains("gekozen")
                || block.text.to_lowercase().contains("élu");
            out.tallies.push(VoteTallyDraft {
                result_id: result_id.to_string(),
                tally_kind: "candidate".to_string(),
                option_key: key,
                label_nl: name.clone(),
                label_fr: name,
                dimension: "overall".to_string(),
                count,
                selected,
            });
            push_span(
                out,
                "VoteResult",
                result_id,
                "candidate_tally",
                block.index,
                block.index + 1,
                "extraction",
                "count,selected",
            );
        }
        let lower = block.text.to_lowercase();
        let proclamation = lower.contains("gekozen")
            || lower.contains("verkozen")
            || lower.contains("est élu")
            || lower.contains("est élue")
            || lower.contains("wordt uitgeroepen");
        if proclamation {
            for tally in out
                .tallies
                .iter_mut()
                .filter(|t| t.result_id == result_id && t.tally_kind == "candidate")
            {
                if lower.contains(&tally.label_nl.to_lowercase()) {
                    tally.selected = true;
                }
            }
            push_span(
                out,
                "VoteResult",
                result_id,
                "proclamation",
                block.index,
                block.index + 1,
                "extraction",
                "selected,outcome",
            );
        }
    }
}

fn build_decision(
    session_id: u32,
    meeting_id: u32,
    date: &str,
    vote_seq: u32,
    vote_id: &str,
    result_id: &str,
    pending: &TitleRefs,
    method: &str,
    status: &str,
    outcome: &str,
    source_num: &str,
    reuses: bool,
    source_url: &str,
    cache_path: &str,
) -> VoteDecisionDraft {
    VoteDecisionDraft {
        vote_id: vote_id.to_string(),
        result_id: result_id.to_string(),
        session_id,
        meeting_id,
        date: date.to_string(),
        seq: vote_seq,
        title_nl: pending.title_nl.clone(),
        title_fr: pending.title_fr.clone(),
        method: method.to_string(),
        status: status.to_string(),
        outcome: outcome.to_string(),
        dossier_id: pending.dossier_id.clone(),
        document_id: pending.document_id.clone(),
        motion_id: pending.motion_id.clone(),
        source_roll_call_number: source_num.to_string(),
        reuses_result: reuses,
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
    }
}

fn merge_refs(pending: &mut TitleRefs, text: &str) {
    if let Some(caps) = vote_regex_1().captures(text) {
        pending.dossier_id = caps[2].trim().to_string();
        pending.document_id = caps[3].trim().to_string();
    } else if let Some(caps) = vote_regex_2().captures(text) {
        pending.motion_id = caps[2].trim().to_string();
    } else if let Some(caps) = dossier_ref_re().captures(text)
        && pending.document_id.is_empty()
    {
        pending.document_id = caps[1].trim().to_string();
    }
}

fn vote_regex_1() -> &'static Regex {
    VOTE_REGEX_1.get_or_init(|| Regex::new(r#"^(.*)\((\d+)/(\d+(?:-\d+)?)\)\s*$"#).unwrap())
}

fn vote_regex_2() -> &'static Regex {
    VOTE_REGEX_2.get_or_init(|| Regex::new(r#"^(.*)\s+\((?:nr\.|n°)\s*(\d+)\)\s*$"#).unwrap())
}

fn infer_roll_call_outcome(blocks: &[ReportBlock], table_idx: usize) -> Option<(String, u32)> {
    for block in blocks.iter().skip(table_idx + 1).take(5) {
        if block.tag == BlockTag::Table {
            break;
        }
        if let Some(outcome) = formal_outcome(&block.text) {
            return Some((outcome.to_string(), block.index));
        }
    }
    None
}

fn infer_secret_outcome(blocks: &[ReportBlock], table_idx: usize) -> Option<(String, u32)> {
    for block in blocks.iter().skip(table_idx + 1).take(8) {
        if block.tag == BlockTag::Table || block.tag == BlockTag::H1 {
            break;
        }
        if let Some(outcome) = formal_outcome(&block.text) {
            return Some((outcome.to_string(), block.index));
        }
        if block.text.to_lowercase().contains("meerderheid bekomen")
            || block.text.to_lowercase().contains("majorité absolue")
        {
            return Some(("adopted".to_string(), block.index));
        }
    }
    None
}

fn push_roll_call_tallies(
    tallies: &mut Vec<VoteTallyDraft>,
    result_id: &str,
    parsed: &crate::vote_events::ParsedRollCallTable,
) {
    let c = &parsed.counts;
    match parsed.shape {
        RollCallTableShape::Standard => {
            for (key, count) in [("yes", c.yes), ("no", c.no), ("abstain", c.abstain)] {
                if let Some(count) = count {
                    tallies.push(VoteTallyDraft {
                        result_id: result_id.to_string(),
                        tally_kind: "position".to_string(),
                        option_key: key.to_string(),
                        label_nl: key.to_string(),
                        label_fr: key.to_string(),
                        dimension: "overall".to_string(),
                        count,
                        selected: false,
                    });
                }
            }
        }
        RollCallTableShape::LanguageGroup => {
            for (key, overall, nl, fr) in [
                ("yes", c.yes, c.yes_nl, c.yes_fr),
                ("no", c.no, c.no_nl, c.no_fr),
                ("abstain", c.abstain, c.abstain_nl, c.abstain_fr),
            ] {
                if let Some(overall) = overall {
                    tallies.push(VoteTallyDraft {
                        result_id: result_id.to_string(),
                        tally_kind: "position".to_string(),
                        option_key: key.to_string(),
                        label_nl: key.to_string(),
                        label_fr: key.to_string(),
                        dimension: "overall".to_string(),
                        count: overall,
                        selected: false,
                    });
                }
                if let (Some(nl), Some(fr)) = (nl, fr) {
                    tallies.push(VoteTallyDraft {
                        result_id: result_id.to_string(),
                        tally_kind: "position".to_string(),
                        option_key: key.to_string(),
                        label_nl: key.to_string(),
                        label_fr: key.to_string(),
                        dimension: "nl_group".to_string(),
                        count: nl,
                        selected: false,
                    });
                    tallies.push(VoteTallyDraft {
                        result_id: result_id.to_string(),
                        tally_kind: "position".to_string(),
                        option_key: key.to_string(),
                        label_nl: key.to_string(),
                        label_fr: key.to_string(),
                        dimension: "fr_group".to_string(),
                        count: fr,
                        selected: false,
                    });
                }
            }
        }
    }
}

fn push_secret_tallies(
    tallies: &mut Vec<VoteTallyDraft>,
    result_id: &str,
    secret: &crate::vote_events::ParsedSecretTable,
) {
    for (key, value) in [
        ("voters", secret.voters),
        ("valid", secret.valid),
        ("blank_invalid", secret.blank_invalid),
        ("majority_threshold", secret.majority_threshold),
    ] {
        if let Some(count) = value {
            tallies.push(VoteTallyDraft {
                result_id: result_id.to_string(),
                tally_kind: "statistic".to_string(),
                option_key: key.to_string(),
                label_nl: key.to_string(),
                label_fr: key.to_string(),
                dimension: "overall".to_string(),
                count,
                selected: false,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report_blocks::{parse_report_blocks, read_report_html};
    use scraper::Html;
    use std::path::Path;

    fn assemble(name: &str) -> VoteAssemblyOutput {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/votes")
            .join(name);
        let html = read_report_html(&path).unwrap();
        let blocks = parse_report_blocks(&Html::parse_document(&html));
        assemble_votes_from_blocks(&blocks, 56, 60, "2020-01-01", "url", "cache")
    }

    #[test]
    fn fixture_secret_ballot_bilingual_h1() {
        let out = assemble("secret_ballot_bilingual_h1.html");
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].method, "secret_ballot");
        assert!(
            out.tallies
                .iter()
                .any(|t| t.option_key == "voters" && t.count == 111)
        );
    }

    #[test]
    fn fixture_roll_call_after_secret_section() {
        let out = assemble("roll_call_after_secret_section.html");
        assert_eq!(out.results.len(), 2);
        assert_eq!(out.results[0].method, "secret_ballot");
        assert_eq!(out.results[1].method, "roll_call");
        assert_eq!(out.results[1].source_roll_call_number, "12");
    }

    #[test]
    fn fixture_roll_call_after_agenda_h2() {
        let out = assemble("roll_call_after_agenda_h2.html");
        assert_eq!(out.results.len(), 2);
        assert_eq!(out.results[0].source_roll_call_number, "9");
        assert_eq!(out.results[1].source_roll_call_number, "10");
    }

    #[test]
    fn fixture_roll_call_compact() {
        let out = assemble("roll_call_compact.html");
        assert_eq!(out.decisions.len(), 1);
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].method, "roll_call");
        assert!(
            out.tallies
                .iter()
                .any(|t| t.option_key == "yes" && t.count == 74)
        );
        assert!(!out.members.is_empty());
        assert!(
            out.span_evidence
                .iter()
                .any(|s| s.span_role == "result_table")
        );
    }

    #[test]
    fn untitled_decisions_are_removed_without_removing_result_evidence() {
        let mut out = VoteAssemblyOutput {
            decisions: vec![
                VoteDecisionDraft {
                    vote_id: "56-60-v1".to_string(),
                    result_id: "56-60-r1".to_string(),
                    session_id: 56,
                    meeting_id: 60,
                    date: "2020-01-01".to_string(),
                    seq: 1,
                    title_nl: String::new(),
                    title_fr: String::new(),
                    method: "roll_call".to_string(),
                    status: "complete".to_string(),
                    outcome: "adopted".to_string(),
                    dossier_id: String::new(),
                    document_id: String::new(),
                    motion_id: String::new(),
                    source_roll_call_number: "1".to_string(),
                    reuses_result: false,
                    source_url: "url".to_string(),
                    cache_path: "cache".to_string(),
                },
                VoteDecisionDraft {
                    vote_id: "56-60-v2".to_string(),
                    result_id: "56-60-r2".to_string(),
                    session_id: 56,
                    meeting_id: 60,
                    date: "2020-01-01".to_string(),
                    seq: 2,
                    title_nl: "A titled decision".to_string(),
                    title_fr: String::new(),
                    method: "roll_call".to_string(),
                    status: "complete".to_string(),
                    outcome: "adopted".to_string(),
                    dossier_id: String::new(),
                    document_id: String::new(),
                    motion_id: String::new(),
                    source_roll_call_number: "2".to_string(),
                    reuses_result: false,
                    source_url: "url".to_string(),
                    cache_path: "cache".to_string(),
                },
            ],
            span_evidence: vec![
                SpanEvidence {
                    entity_type: "Vote".to_string(),
                    entity_id: "56-60-v1".to_string(),
                    span_role: "decision_title".to_string(),
                    block_start: 1,
                    block_end: 2,
                    coverage_kind: "extraction".to_string(),
                    field_names: "title_nl".to_string(),
                },
                SpanEvidence {
                    entity_type: "Vote".to_string(),
                    entity_id: "56-60-v2".to_string(),
                    span_role: "decision_title".to_string(),
                    block_start: 3,
                    block_end: 4,
                    coverage_kind: "extraction".to_string(),
                    field_names: "title_nl".to_string(),
                },
                SpanEvidence {
                    entity_type: "VoteResult".to_string(),
                    entity_id: "56-60-r1".to_string(),
                    span_role: "result_table".to_string(),
                    block_start: 1,
                    block_end: 2,
                    coverage_kind: "extraction".to_string(),
                    field_names: "yes,no".to_string(),
                },
            ],
            ..VoteAssemblyOutput::default()
        };

        discard_untitled_decisions(&mut out, 56, 60);

        assert_eq!(out.decisions.len(), 1);
        assert_eq!(out.decisions[0].vote_id, "56-60-v1");
        assert_eq!(out.decisions[0].seq, 1);
        assert!(
            out.span_evidence
                .iter()
                .all(|span| { span.entity_type != "Vote" || span.entity_id == "56-60-v1" })
        );
        assert!(
            out.span_evidence
                .iter()
                .any(|span| { span.entity_type == "VoteResult" && span.entity_id == "56-60-r1" })
        );
    }

    #[test]
    fn fixture_result_reuse() {
        let out = assemble("result_reuse.html");
        assert_eq!(out.decisions.len(), 2);
        assert_eq!(out.results.len(), 1);
        assert!(out.unresolved_events.is_empty());
        assert!(out.decisions[1].reuses_result);
        assert_eq!(out.decisions[0].result_id, out.decisions[1].result_id);
        assert_eq!(out.decisions[1].source_roll_call_number, "1");
        assert_eq!(out.decisions[1].outcome, "rejected");
        assert!(
            !out.decisions[1].title_nl.is_empty(),
            "reuse decision should retain title evidence"
        );
    }

    #[test]
    fn fixture_result_reuse_vote_stemming_marker() {
        let out = assemble("result_reuse_vote_stemming.html");
        assert_eq!(out.decisions.len(), 2);
        assert_eq!(out.results.len(), 1);
        assert!(out.unresolved_events.is_empty());
        assert!(out.decisions[1].reuses_result);
        assert_eq!(out.decisions[1].source_roll_call_number, "61");
        assert_eq!(out.decisions[1].outcome, "rejected");
    }

    #[test]
    fn fixture_language_group() {
        let out = assemble("language_group_roll_call.html");
        assert_eq!(out.results[0].method, "language_group_roll_call");
        assert!(out.tallies.iter().any(|t| t.dimension == "nl_group"));
    }

    #[test]
    fn fixture_secret_ballot() {
        let out = assemble("secret_ballot_aggregate.html");
        assert_eq!(out.results[0].method, "secret_ballot");
        assert!(out.tallies.iter().any(|t| t.option_key == "voters"));
    }

    #[test]
    fn fixture_secret_ballot_candidates() {
        let out = assemble("secret_ballot_candidates.html");
        assert_eq!(out.results.len(), 1);
        assert!(out.tallies.iter().any(|t| t.tally_kind == "candidate"));
    }

    #[test]
    fn fixture_quorum_failure() {
        let out = assemble("quorum_failure.html");
        assert_eq!(out.results[0].status, "no_quorum");
        assert!(out.tallies.iter().any(|t| t.option_key == "participated"));
        assert!(!out.tallies.iter().any(|t| t.option_key == "yes"));
    }

    #[test]
    fn fixture_sitting_standing() {
        let out = assemble("sitting_standing.html");
        assert_eq!(out.results[0].method, "sitting_standing");
        assert_eq!(out.results[0].outcome, "adopted");
        assert!(out.tallies.is_empty());
    }

    #[test]
    fn fixture_appendix_reverse_order() {
        let out = assemble("appendix_reverse_order.html");
        assert!(out.decisions.is_empty());
        assert_eq!(out.results.len(), 1);
        assert!(!out.members.is_empty());
    }

    #[test]
    fn fixture_formal_sequence_quorum() {
        let out = assemble("quorum_failure_formal_sequence.html");
        assert_eq!(out.results.len(), 2);
        assert!(out.results.iter().all(|r| r.status == "no_quorum"));
        assert!(out.decisions.iter().all(|decision| {
            out.span_evidence.iter().any(|span| {
                span.entity_id == decision.vote_id && span.span_role == "decision_title"
            })
        }));
    }

    #[test]
    fn fixture_debate_quoted_table_not_vote() {
        let out = assemble("debate_quoted_table.html");
        assert!(
            out.decisions.is_empty(),
            "quoted table in debate must not produce votes"
        );
    }

    #[test]
    fn fixture_meeting_81_compact_without_appendix_preserves_absence() {
        let out = assemble("meeting_81_compact_no_appendix.html");
        assert_eq!(out.decisions.len(), 1);
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].outcome, "adopted");
        assert!(out.members.is_empty());
        assert!(
            out.tallies
                .iter()
                .any(|t| t.option_key == "yes" && t.count == 88)
        );
        assert!(!out.tallies.iter().any(|t| t.option_key == "abstain"));
        for role in [
            "decision_title",
            "result_reference",
            "overall_counts",
            "formal_outcome",
        ] {
            assert!(
                out.span_evidence.iter().any(|span| span.span_role == role),
                "missing {role}"
            );
        }
    }

    #[test]
    fn fixture_meeting_135_secret_naturalization() {
        let out = assemble("meeting_135_secret_naturalization.html");
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].method, "secret_ballot");
        assert_eq!(out.results[0].outcome, "adopted");
        assert!(out.decisions[0].title_nl.contains("Naturalisaties"));
        assert!(
            out.tallies
                .iter()
                .any(|t| t.option_key == "blank_invalid" && t.count == 2)
        );
        for role in ["decision_title", "secret_statistics", "formal_outcome"] {
            assert!(
                out.span_evidence.iter().any(|span| span.span_role == role),
                "missing {role}"
            );
        }
    }

    #[test]
    fn fixture_rejected_sitting_standing_resets_context() {
        let out = assemble("sitting_standing_rejected.html");
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].method, "sitting_standing");
        assert_eq!(out.results[0].outcome, "rejected");
        assert_eq!(out.decisions.len(), 1);
        assert!(
            out.span_evidence
                .iter()
                .any(|span| span.span_role == "decision_title")
        );
        assert!(
            out.span_evidence
                .iter()
                .any(|span| span.span_role == "formal_outcome")
        );
    }

    #[test]
    fn fixture_repeated_source_numbers_use_ordered_appendices() {
        let out = assemble("repeated_source_occurrence.html");
        assert_eq!(out.results.len(), 2);
        assert!(
            out.results
                .iter()
                .all(|result| result.source_roll_call_number == "1")
        );
        let first: Vec<_> = out
            .members
            .iter()
            .filter(|member| member.result_id == out.results[0].result_id)
            .map(|member| member.raw_name.as_str())
            .collect();
        let second: Vec<_> = out
            .members
            .iter()
            .filter(|member| member.result_id == out.results[1].result_id)
            .map(|member| member.raw_name.as_str())
            .collect();
        assert_eq!(first, vec!["Alpha Alice", "Beta Bob", "Gamma Gina"]);
        assert_eq!(second, vec!["Delta Dirk", "Epsilon Els", "Zeta Zoe"]);
        assert_eq!(
            out.span_evidence
                .iter()
                .filter(|span| span.span_role == "appendix_header")
                .count(),
            2
        );
        assert!(
            out.span_evidence
                .iter()
                .any(|span| span.span_role == "appendix_bucket_count")
        );
        assert!(
            out.span_evidence
                .iter()
                .any(|span| span.span_role == "appendix_voter_names")
        );
    }

    #[test]
    fn fixture_multiple_secret_ballots_scope_candidates_and_proclamations() {
        let out = assemble("multiple_secret_ballots.html");
        assert_eq!(out.results.len(), 2);
        assert_eq!(out.decisions.len(), 2);
        for result in &out.results {
            let candidates: Vec<_> = out
                .tallies
                .iter()
                .filter(|tally| {
                    tally.result_id == result.result_id && tally.tally_kind == "candidate"
                })
                .collect();
            assert_eq!(candidates.len(), 2);
            assert_eq!(candidates.iter().filter(|tally| tally.selected).count(), 1);
        }
        assert_eq!(
            out.span_evidence
                .iter()
                .filter(|span| span.span_role == "proclamation")
                .count(),
            2
        );
        assert_eq!(
            out.span_evidence
                .iter()
                .filter(|span| span.span_role == "decision_title")
                .count(),
            2
        );
    }

    #[test]
    fn fixture_unanimity_and_no_objection_are_not_vote_events() {
        let out = assemble("unanimity_no_objection_negative.html");
        assert!(out.decisions.is_empty());
        assert!(out.results.is_empty());
    }

    #[test]
    fn fixture_vote_shaped_table_after_closed_zone_is_ignored() {
        let out = assemble("post_zone_quoted_table.html");
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].source_roll_call_number, "7");
    }

    #[test]
    fn invalid_language_group_sum_is_unresolved_and_not_fabricated() {
        let out = assemble("language_group_invalid.html");
        assert_eq!(out.results.len(), 1);
        assert!(out.unresolved_events.iter().any(|event| {
            event.reason == "language_group_sum_mismatch:yes" && event.block_start < event.block_end
        }));
        assert!(!out.tallies.iter().any(|tally| tally.option_key == "yes"));
        assert!(out.tallies.iter().any(|tally| {
            tally.option_key == "no" && tally.dimension == "overall" && tally.count == 15
        }));
    }

    #[test]
    fn quorum_failure_has_only_participation_semantics_and_spans() {
        let out = assemble("quorum_failure.html");
        assert!(out.members.is_empty());
        assert!(
            out.tallies
                .iter()
                .any(|t| t.option_key == "participated" && t.count == 65)
        );
        assert!(
            out.tallies
                .iter()
                .any(|t| t.option_key == "required" && t.count == 71)
        );
        assert!(!out.tallies.iter().any(|t| t.tally_kind == "position"));
        for role in ["decision_title", "result_reference", "quorum_participation"] {
            assert!(
                out.span_evidence.iter().any(|span| span.span_role == role),
                "missing {role}"
            );
        }
    }
}
