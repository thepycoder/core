use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

/// Staging row for written / oral-written minister replies (shared schema).
#[derive(Debug, Clone)]
pub struct AnswerDraft {
    pub answer_id: String,
    pub question_id: String,
    pub route_id: String,
    pub session_id: u32,
    pub meeting_id: String,
    pub meeting_kind: String,
    pub agenda_id: String,
    pub answer_slot: u8,
    pub kind: String,
    pub text_nl: String,
    pub text_fr: String,
    pub status: String,
    pub answer_num: String,
    pub publication_ref: String,
    pub casa: String,
    pub source_kind: String,
    pub confidence: String,
    pub source_url: String,
    pub cache_path: String,
}

macro_rules! col {
    ($rows:expr, $f:expr) => {
        Arc::new(StringArray::from($rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
    };
}

pub fn write_answers_parquet(path: &Path, rows: &[AnswerDraft]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let schema = Arc::new(Schema::new(vec![
        Field::new("answer_id", DataType::Utf8, false),
        Field::new("question_id", DataType::Utf8, false),
        Field::new("route_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("meeting_kind", DataType::Utf8, false),
        Field::new("agenda_id", DataType::Utf8, false),
        Field::new("answer_slot", DataType::Utf8, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("text_nl", DataType::Utf8, false),
        Field::new("text_fr", DataType::Utf8, false),
        Field::new("status", DataType::Utf8, false),
        Field::new("answer_num", DataType::Utf8, false),
        Field::new("publication_ref", DataType::Utf8, false),
        Field::new("casa", DataType::Utf8, false),
        Field::new("source_kind", DataType::Utf8, false),
        Field::new("confidence", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            col!(rows, |r| r.answer_id.clone()),
            col!(rows, |r| r.question_id.clone()),
            col!(rows, |r| r.route_id.clone()),
            col!(rows, |r| r.session_id.to_string()),
            col!(rows, |r| r.meeting_id.clone()),
            col!(rows, |r| r.meeting_kind.clone()),
            col!(rows, |r| r.agenda_id.clone()),
            col!(rows, |r| r.answer_slot.to_string()),
            col!(rows, |r| r.kind.clone()),
            col!(rows, |r| r.text_nl.clone()),
            col!(rows, |r| r.text_fr.clone()),
            col!(rows, |r| r.status.clone()),
            col!(rows, |r| r.answer_num.clone()),
            col!(rows, |r| r.publication_ref.clone()),
            col!(rows, |r| r.casa.clone()),
            col!(rows, |r| r.source_kind.clone()),
            col!(rows, |r| r.confidence.clone()),
            col!(rows, |r| r.source_url.clone()),
            col!(rows, |r| r.cache_path.clone()),
        ],
    )?;

    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}
