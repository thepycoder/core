use crate::common::{
    dedupe_unresolved, reason_label, UnresolvedRow, SESSION_ID,
};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use crawl::utils::ensure_question_id;
use identity::actor_resolver::{ActorResolution, ActorResolver};
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::Bucket;
use serde::Deserialize;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize)]
struct DiscussionEntry {
    speaker: String,
    text: String,
}

#[derive(Debug, Clone)]
pub struct UtteranceRow {
    pub utterance_id: String,
    pub question_id: String,
    pub session_id: String,
    pub meeting_id: String,
    pub meeting_kind: String,
    pub seq: String,
    pub raw_speaker: String,
    pub speaker_person_id: String,
    pub speaker_entity_type: String,
    pub speaker_entity_id: String,
    pub text: String,
    pub source_url: String,
    pub cache_path: String,
    pub confidence: String,
}

pub struct UtteranceOutput {
    pub rows: Vec<UtteranceRow>,
    pub unresolved: Vec<UnresolvedRow>,
}

fn skip_speaker(raw: &str) -> bool {
    let lower = raw.trim().to_lowercase();
    lower.is_empty() || lower == "onbekend" || lower == "n ."
}

pub fn normalize_utterances(
    data_dir: &Path,
    actor_resolver: &ActorResolver,
) -> Result<UtteranceOutput, Box<dyn Error>> {
    let mut rows = Vec::new();
    let mut unresolved = Vec::new();

    for (meeting_kind, rel_path) in [
        ("plenary", format!("sessions/{SESSION_ID}/plenary/questions.parquet")),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/questions.parquet"),
        ),
    ] {
        let path = data_dir.join(rel_path);
        for batch in read_all_rows(&path)? {
            let question_ids = read_string_column(&batch, "question_id")?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let discussions = read_string_column(&batch, "discussion")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                if discussions[i].trim().is_empty() {
                    continue;
                }
                let question_id = ensure_question_id(
                    &session_ids[i],
                    meeting_kind,
                    &question_ids[i],
                );
                let entries: Vec<DiscussionEntry> =
                    serde_json::from_str(&discussions[i]).unwrap_or_default();

                for (seq, entry) in entries.iter().enumerate() {
                    let utterance_id = format!("{question_id}_{seq}");
                    let speaker = entry.speaker.trim().to_string();
                    let (speaker_person_id, speaker_entity_type, speaker_entity_id, confidence) =
                        if skip_speaker(&speaker) {
                            (String::new(), String::new(), String::new(), String::new())
                        } else {
                            let detail = actor_resolver.resolve_actor_detail(&speaker, Bucket::Speaker);
                            match detail.resolution {
                                ActorResolution::Person(person_id) => (
                                    person_id.clone(),
                                    "Person".to_string(),
                                    person_id,
                                    "exact".to_string(),
                                ),
                                ActorResolution::ExternalPerson(ext_id) => (
                                    String::new(),
                                    "ExternalPerson".to_string(),
                                    ext_id,
                                    "exact".to_string(),
                                ),
                                ActorResolution::Unresolved(reason) => {
                                    unresolved.push(UnresolvedRow {
                                        raw_name: detail.raw_name,
                                        typo_corrected: detail.typo_corrected,
                                        norm_primary: detail.norm_primary,
                                        norm_reordered: detail.norm_reordered,
                                        reason: reason_label(&reason).to_string(),
                                        source_bucket: "speakers".to_string(),
                                        role: "speaker".to_string(),
                                        context_id: question_id.clone(),
                                        context_label: format!("utterance {utterance_id}"),
                                        raw_field: speaker.clone(),
                                        source_url: source_urls[i].clone(),
                                        cache_path: cache_paths[i].clone(),
                                    });
                                    (String::new(), String::new(), String::new(), String::new())
                                }
                            }
                        };

                    rows.push(UtteranceRow {
                        utterance_id,
                        question_id: question_id.clone(),
                        session_id: session_ids[i].clone(),
                        meeting_id: meeting_ids[i].clone(),
                        meeting_kind: meeting_kind.to_string(),
                        seq: seq.to_string(),
                        raw_speaker: speaker,
                        speaker_person_id,
                        speaker_entity_type,
                        speaker_entity_id,
                        text: entry.text.clone(),
                        source_url: source_urls[i].clone(),
                        cache_path: cache_paths[i].clone(),
                        confidence,
                    });
                }
            }
        }
    }

    rows.sort_by(|a, b| a.utterance_id.cmp(&b.utterance_id));
    dedupe_unresolved(&mut unresolved);
    Ok(UtteranceOutput { rows, unresolved })
}

pub fn write_utterances(path: &Path, rows: &[UtteranceRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("utterance_id", false),
        utf8_field("question_id", false),
        utf8_field("session_id", false),
        utf8_field("meeting_id", false),
        utf8_field("meeting_kind", false),
        utf8_field("seq", false),
        utf8_field("raw_speaker", false),
        utf8_field("speaker_person_id", false),
        utf8_field("speaker_entity_type", false),
        utf8_field("speaker_entity_id", false),
        utf8_field("text", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
        utf8_field("confidence", false),
    ]);

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    write_parquet(
        path,
        schema,
        vec![
            col!(|r| r.utterance_id.clone()),
            col!(|r| r.question_id.clone()),
            col!(|r| r.session_id.clone()),
            col!(|r| r.meeting_id.clone()),
            col!(|r| r.meeting_kind.clone()),
            col!(|r| r.seq.clone()),
            col!(|r| r.raw_speaker.clone()),
            col!(|r| r.speaker_person_id.clone()),
            col!(|r| r.speaker_entity_type.clone()),
            col!(|r| r.speaker_entity_id.clone()),
            col!(|r| r.text.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_discussion_json() {
        let raw = r#"[{"speaker":"Jan Jambon","text":"Hello"}]"#;
        let entries: Vec<DiscussionEntry> = serde_json::from_str(raw).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].speaker, "Jan Jambon");
    }
}
