//! Canonical vote decision/result staging types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteDecisionDraft {
    pub vote_id: String,
    pub result_id: String,
    pub session_id: u32,
    pub meeting_id: u32,
    pub date: String,
    pub seq: u32,
    pub title_nl: String,
    pub title_fr: String,
    pub method: String,
    pub status: String,
    pub outcome: String,
    pub dossier_id: String,
    pub document_id: String,
    pub motion_id: String,
    pub source_roll_call_number: String,
    pub reuses_result: bool,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteResultDraft {
    pub result_id: String,
    pub session_id: u32,
    pub meeting_id: u32,
    pub seq: u32,
    pub method: String,
    pub named: bool,
    pub status: String,
    pub outcome: String,
    pub source_roll_call_number: String,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteTallyDraft {
    pub result_id: String,
    pub tally_kind: String,
    pub option_key: String,
    pub label_nl: String,
    pub label_fr: String,
    pub dimension: String,
    pub count: u32,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteResultMemberDraft {
    pub result_id: String,
    pub position: String,
    pub seq: u32,
    pub raw_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct VoteAssemblyOutput {
    pub decisions: Vec<VoteDecisionDraft>,
    pub results: Vec<VoteResultDraft>,
    pub tallies: Vec<VoteTallyDraft>,
    pub members: Vec<VoteResultMemberDraft>,
    pub span_evidence: Vec<SpanEvidence>,
    pub unresolved_events: Vec<UnresolvedVoteEventDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnresolvedVoteEventDraft {
    pub session_id: u32,
    pub meeting_id: u32,
    pub event_kind: String,
    pub source_roll_call_number: String,
    pub block_start: u32,
    pub block_end: u32,
    pub reason: String,
    pub evidence_text: String,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanEvidence {
    pub entity_type: String,
    pub entity_id: String,
    pub span_role: String,
    pub block_start: u32,
    pub block_end: u32,
    pub coverage_kind: String,
    pub field_names: String,
}

pub fn composite_vote_id(session_id: u32, meeting_id: u32, seq: u32) -> String {
    format!("{session_id}-{meeting_id}-v{seq}")
}

pub fn composite_result_id(session_id: u32, meeting_id: u32, seq: u32) -> String {
    format!("{session_id}-{meeting_id}-r{seq}")
}
