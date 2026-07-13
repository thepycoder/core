pub mod characters;
pub mod client;
pub mod paths;
pub mod utils;

pub mod agenda_timeline;
pub mod answer_io;
pub mod question_boundaries;
pub mod meeting_report;
pub mod proceeding_entities;
pub mod proceeding_io;
pub mod qrva_text;
pub mod report_blocks;
pub mod speaker_parse;
pub mod speech_zones;
pub mod qa_markers;
pub mod qa_coverage;
pub mod utterance_io;
pub mod utterance_segment;
pub mod written_oral_qa;

pub mod vote_inventory;

pub use agenda_timeline::{
    count_agenda_questions_from_cache, extract_agenda_number, looks_like_fr_heading, AgendaItem,
    ItemKind, MeetingKind,
};
pub use answer_io::{write_answers_parquet, AnswerDraft};
pub use proceeding_entities::{
    extract_proceedings_from_document, is_non_question_proceeding_heading, HearingDraft,
    InterpellationDraft,
};
pub use proceeding_io::{write_hearings_parquet, write_interpellations_parquet};
pub use qrva_text::{
    actr_id_to_person_id, department_external_id, docname_internal_id, flatten_qrva_text,
    inline_answer_id, parse_aut_actr_id, parse_oral_refs, qrva_answer_id, qrva_detail_url,
    route_id, written_question_id, QRVA_API_BASE,
};
pub use question_boundaries::{
    classify_question_heading_bilingual, classify_question_heading_text, extends_open_question,
    is_group_start_text, is_hearing_text, is_questions_section, is_single_text,
    is_subquestion_text, has_pending_question_text, starts_new_question_unit, QuestionHeadingRole,
};
pub use speaker_parse::{count_source_markers, detect_turn_start, parse_speaker_label, SpeakerRole, TurnStart};
pub use meeting_report::{extract_utterances_from_cache, extract_utterances_from_document};
pub use report_blocks::{parse_report_blocks, read_report_html, BlockTag, ReportBlock};
pub use qa_markers::{check_markers_vs_utterances, MarkerCheckResult};
pub use qa_coverage::{count_document_words, count_document_words_from_cache, word_count};
pub use utterance_io::write_utterances_parquet;
pub use utterance_segment::{segment_utterances, UtteranceDraft};
pub use written_oral_qa::{
    extract_written_oral_items, find_written_oral_zone_start, is_written_oral_section_heading,
    oral_written_answer_drafts, OralWrittenItem,
};
pub use vote_inventory::{
    appendix_marker_for_vote, inventory_vote_numbers, parse_appendix_vote_number,
    parse_compact_vote_number, parse_paragraph_vote_number, parse_vote_inventory,
    vote_number_gaps, VoteInventory,
};
