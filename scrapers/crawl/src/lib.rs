// Parser entry points take many explicit context parameters by design; extracting
// a context struct is planned separately (maintainability review, T3).
#![allow(clippy::too_many_arguments)]

pub mod characters;
pub mod client;
pub mod paths;
pub mod utils;

pub mod agenda_io;
pub mod agenda_timeline;
pub mod answer_io;
pub mod artifact_id;
pub mod atomic_io;
pub mod cache_meta;
pub mod corpus_policy;
pub mod freshness;
pub mod meeting_gaps;
pub mod meeting_parse;
pub mod meeting_report;
pub mod oral_questions;
pub mod proceeding_entities;
pub mod proceeding_io;
pub mod qa_coverage;
pub mod qa_markers;
pub mod qrva_text;
pub mod question_boundaries;
pub mod report_blocks;
pub mod report_blocks_io;
pub mod source_manifest;
pub mod source_spans;
pub mod speaker_parse;
pub mod speech_zones;
pub mod utterance_io;
pub mod utterance_segment;
pub mod vote_assembly;
pub mod vote_events;
pub mod vote_io;
pub mod vote_patterns;
pub mod vote_types;
pub mod written_oral_qa;

pub mod vote_inventory;

pub use agenda_io::{AgendaItemDraft, materialize_agenda_items, write_agenda_items_parquet};
pub use agenda_timeline::{
    AgendaItem, ItemKind, MeetingKind, count_agenda_questions_from_cache, extract_agenda_number,
    looks_like_fr_heading,
};
pub use answer_io::{AnswerDraft, write_answers_parquet};
pub use artifact_id::{
    BLOCK_PARSER_VERSION, REPORT_BLOCK_EXTRACTOR_VERSION, VOTE_EXTRACTOR_VERSION, artifact_id,
    content_hash, content_hash_bytes,
};
pub use atomic_io::{BundlePublisher, write_bytes_atomic, write_text_atomic};
pub use cache_meta::{
    CacheMetadata, looks_like_pdf, meta_path_for, now_rfc3339, read_cache_metadata,
    require_cache_present, touch_checked_at, write_cache_artifact, write_cache_metadata,
};
pub use corpus_policy::{
    CorpusClass, CorpusClassification, POLICY_DOC, classify_meeting,
    is_procedural_credentials_heading, whole_report_constitutive_from_blocks,
};
pub use freshness::{
    FRESHNESS_POLICIES, FreshnessPolicy, days_between_rfc3339, freshness_policy,
    rfc3339_to_unix_days,
};
pub use meeting_gaps::{
    ACCEPTED_GAP_REASONS, DiscoveryResult, GAP_REASON_NO_RESULT, GAP_REASON_NOT_FOUND,
    GAP_REASON_UNSUPPORTED_FORMAT, MeetingGapRow, MeetingHttpOutcome, classify_meeting_http_status,
    discover_last_from_probes, gap_reason_to_manifest_status, load_prior_gaps,
    manifest_status_for_parsed, reconcile_meeting_coverage, record_gap, upsert_gap,
    write_meeting_gaps_parquet,
};
pub use meeting_parse::{
    MeetingParseOutput, materialize_commission_source_spans, parse_commission_meeting_report,
    parse_plenary_meeting_report,
};
pub use meeting_report::{
    extract_utterances_from_blocks, extract_utterances_from_cache, extract_utterances_from_document,
};
pub use oral_questions::{
    OralQuestionDraft, extract_questions_from_agenda, normalize_questioner_name,
};
pub use proceeding_entities::{
    HearingDraft, InterpellationDraft, extract_proceedings_from_document,
    is_non_question_proceeding_heading,
};
pub use proceeding_io::{write_hearings_parquet, write_interpellations_parquet};
pub use qa_coverage::{count_document_words, count_document_words_from_cache, word_count};
pub use qa_markers::{MarkerCheckResult, check_markers_vs_utterances};
pub use qrva_text::{
    QRVA_API_BASE, actr_id_to_person_id, department_external_id, docname_internal_id,
    flatten_qrva_text, inline_answer_id, parse_aut_actr_id, parse_oral_refs, qrva_answer_id,
    qrva_detail_url, route_id, written_question_id,
};
pub use question_boundaries::{
    QuestionHeadingRole, classify_question_heading_bilingual, classify_question_heading_text,
    extends_open_question, has_pending_question_text, is_group_start_text, is_hearing_text,
    is_questions_section, is_single_text, is_subquestion_text, starts_new_question_unit,
};
pub use report_blocks::{
    BlockTag, InlineSpan, ReportBlock, TableCell, TableRow, parse_report_blocks, read_report_html,
    table_row_labels, table_row_numeric_cells, table_rows_text,
};
pub use report_blocks_io::{
    ReportBlockRow, materialize_report_blocks, write_report_blocks_parquet,
};
pub use source_manifest::{
    MANIFEST_STATUS_NO_RESULT, MANIFEST_STATUS_NOT_FOUND, MANIFEST_STATUS_PARSED,
    MANIFEST_STATUS_UNSUPPORTED_FORMAT, MANIFEST_STATUSES, SourceManifestRow,
    is_known_manifest_status, manifest_path, stage_source_manifest, validate_manifest_rows,
    write_source_manifest,
};
pub use source_spans::{
    SourceSpanDraft, SpanValidationOutput, make_artifact_id, span_id, validate_source_spans,
    write_source_spans_parquet,
};
pub use speaker_parse::{
    SpeakerRole, TurnStart, count_source_markers, detect_turn_start, parse_speaker_label,
};
pub use utils::agenda_item_id;
pub use utterance_io::write_utterances_parquet;
pub use utterance_segment::{UtteranceDraft, segment_utterances};
pub use vote_assembly::assemble_votes_from_blocks;
pub use vote_io::{
    write_unresolved_vote_events_parquet, write_vote_bundle, write_vote_result_members_parquet,
    write_vote_results_parquet, write_vote_tallies_parquet, write_votes_parquet,
};
pub use vote_patterns::{
    VoteBucket, VoteSectionKind, appendix_marker_for_vote, appendix_vote_re, compact_vote_re,
    paragraph_vote_re, parse_appendix_vote_number, parse_compact_vote_number,
    parse_electronic_count_number, parse_paragraph_vote_number, vote_bucket_label,
};
pub use vote_types::{
    SpanEvidence, UnresolvedVoteEventDraft, VoteAssemblyOutput, VoteDecisionDraft, VoteResultDraft,
    VoteResultMemberDraft, VoteTallyDraft, composite_result_id, composite_vote_id,
};
pub use written_oral_qa::{
    OralWrittenItem, extract_written_oral_items, find_written_oral_zone_start,
    is_written_oral_section_heading, oral_written_answer_drafts,
};
