use crate::utterance_segment::UtteranceDraft;
use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

macro_rules! col {
    ($rows:expr, $f:expr) => {
        Arc::new(StringArray::from($rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
    };
}

pub fn write_utterances_parquet(
    path: &Path,
    rows: &[UtteranceDraft],
) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("utterance_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("meeting_kind", DataType::Utf8, false),
        Field::new("agenda_id", DataType::Utf8, false),
        Field::new("agenda_item_id", DataType::Utf8, false),
        Field::new("turn_number", DataType::Utf8, false),
        Field::new("seq", DataType::Utf8, false),
        Field::new("item_kind", DataType::Utf8, false),
        Field::new("item_id", DataType::Utf8, false),
        Field::new("question_ids", DataType::Utf8, false),
        Field::new("dossier_id", DataType::Utf8, false),
        Field::new("document_id", DataType::Utf8, false),
        Field::new("motion_id", DataType::Utf8, false),
        Field::new("vote_id", DataType::Utf8, false),
        Field::new("raw_speaker", DataType::Utf8, false),
        Field::new("speaker_role", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new("language", DataType::Utf8, false),
        Field::new("block_start", DataType::Utf8, false),
        Field::new("block_end", DataType::Utf8, false),
        Field::new("source_section", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            col!(rows, |r| r.utterance_id.clone()),
            col!(rows, |r| r.session_id.to_string()),
            col!(rows, |r| r.meeting_id.to_string()),
            col!(rows, |r| r.meeting_kind.as_str().to_string()),
            col!(rows, |r| r.agenda_id.clone()),
            col!(rows, |r| r.agenda_item_id.clone()),
            col!(rows, |r| r.turn_number.clone()),
            col!(rows, |r| r.seq.to_string()),
            col!(rows, |r| r.item_kind.clone()),
            col!(rows, |r| r.item_id.clone()),
            col!(rows, |r| r.question_ids.clone()),
            col!(rows, |r| r.dossier_id.clone()),
            col!(rows, |r| r.document_id.clone()),
            col!(rows, |r| r.motion_id.clone()),
            col!(rows, |r| r.vote_id.clone()),
            col!(rows, |r| r.raw_speaker.clone()),
            col!(rows, |r| r.speaker_role.clone()),
            col!(rows, |r| r.text.clone()),
            col!(rows, |r| r.language.clone()),
            col!(rows, |r| r.block_start.to_string()),
            col!(rows, |r| r.block_end.to_string()),
            col!(rows, |r| r.source_section.clone()),
            col!(rows, |r| r.source_url.clone()),
            col!(rows, |r| r.cache_path.clone()),
        ],
    )?;

    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}
