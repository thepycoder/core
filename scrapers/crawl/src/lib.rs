pub mod characters;
pub mod client;
pub mod paths;
pub mod utils;

pub mod agenda_timeline;
pub mod meeting_report;
pub mod report_blocks;
pub mod speaker_parse;
pub mod qa_markers;
pub mod utterance_io;
pub mod utterance_segment;

pub use agenda_timeline::{AgendaItem, ItemKind, MeetingKind};
pub use speaker_parse::{count_source_markers, detect_turn_start, parse_speaker_label, SpeakerRole, TurnStart};
pub use meeting_report::{extract_utterances_from_cache, extract_utterances_from_document};
pub use report_blocks::{parse_report_blocks, read_report_html, BlockTag, ReportBlock};
pub use qa_markers::{check_markers_vs_utterances, MarkerCheckResult};
pub use utterance_io::write_utterances_parquet;
pub use utterance_segment::{segment_utterances, UtteranceDraft};
