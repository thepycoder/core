pub mod characters;
pub mod client;
pub mod paths;
pub mod utils;

pub mod agenda_timeline;
pub mod question_boundaries;
pub mod meeting_report;
pub mod report_blocks;
pub mod speaker_parse;
pub mod qa_markers;
pub mod utterance_io;
pub mod utterance_segment;

pub mod vote_inventory;

pub use agenda_timeline::{
    count_agenda_questions_from_cache, AgendaItem, ItemKind, MeetingKind,
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
pub use utterance_io::write_utterances_parquet;
pub use utterance_segment::{segment_utterances, UtteranceDraft};
pub use vote_inventory::{
    inventory_vote_numbers, parse_vote_inventory, vote_number_gaps, VoteInventory,
};
