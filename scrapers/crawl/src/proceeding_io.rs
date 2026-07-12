use crate::proceeding_entities::{HearingDraft, InterpellationDraft};
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

pub fn write_hearings_parquet(path: &Path, rows: &[HearingDraft]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("hearing_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("meeting_kind", DataType::Utf8, false),
        Field::new("agenda_id", DataType::Utf8, false),
        Field::new("title_nl", DataType::Utf8, false),
        Field::new("title_fr", DataType::Utf8, false),
        Field::new("witnesses", DataType::Utf8, false),
        Field::new("dossier_id", DataType::Utf8, false),
        Field::new("internal_ids", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            col!(rows, |r| r.hearing_id.clone()),
            col!(rows, |r| r.session_id.to_string()),
            col!(rows, |r| r.meeting_id.to_string()),
            col!(rows, |r| r.meeting_kind.as_str().to_string()),
            col!(rows, |r| r.agenda_id.clone()),
            col!(rows, |r| r.title_nl.clone()),
            col!(rows, |r| r.title_fr.clone()),
            col!(rows, |r| r.witnesses.clone()),
            col!(rows, |r| r.dossier_id.clone()),
            col!(rows, |r| r.internal_ids.clone()),
            col!(rows, |r| r.source_url.clone()),
            col!(rows, |r| r.cache_path.clone()),
        ],
    )?;

    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

pub fn write_interpellations_parquet(
    path: &Path,
    rows: &[InterpellationDraft],
) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("interpellation_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("meeting_kind", DataType::Utf8, false),
        Field::new("agenda_id", DataType::Utf8, false),
        Field::new("interpellators", DataType::Utf8, false),
        Field::new("respondents", DataType::Utf8, false),
        Field::new("topics_nl", DataType::Utf8, false),
        Field::new("topics_fr", DataType::Utf8, false),
        Field::new("internal_ids", DataType::Utf8, false),
        Field::new("dossier_id", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            col!(rows, |r| r.interpellation_id.clone()),
            col!(rows, |r| r.session_id.to_string()),
            col!(rows, |r| r.meeting_id.to_string()),
            col!(rows, |r| r.meeting_kind.as_str().to_string()),
            col!(rows, |r| r.agenda_id.clone()),
            col!(rows, |r| r.interpellators.clone()),
            col!(rows, |r| r.respondents.clone()),
            col!(rows, |r| r.topics_nl.clone()),
            col!(rows, |r| r.topics_fr.clone()),
            col!(rows, |r| r.internal_ids.clone()),
            col!(rows, |r| r.dossier_id.clone()),
            col!(rows, |r| r.source_url.clone()),
            col!(rows, |r| r.cache_path.clone()),
        ],
    )?;

    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}
