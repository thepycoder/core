use crate::parse::{WrittenQuestionDraft, WrittenRouteDraft};
use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use crawl::answer_io::{write_answers_parquet, AnswerDraft};
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

fn write_string_table(
    path: &Path,
    fields: &[&str],
    rows: &[Vec<String>],
) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let schema = Arc::new(Schema::new(
        fields
            .iter()
            .map(|name| Field::new(*name, DataType::Utf8, false))
            .collect::<Vec<Field>>(),
    ));
    let columns: Vec<ArrayRef> = (0..fields.len())
        .map(|i| {
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|r| r.get(i).cloned().unwrap_or_default())
                    .collect::<Vec<_>>(),
            )) as ArrayRef
        })
        .collect();
    let batch = RecordBatch::try_new(schema.clone(), columns)?;
    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

pub fn write_written_questions(
    path: &Path,
    rows: &[WrittenQuestionDraft],
) -> Result<(), Box<dyn Error>> {
    let fields = [
        "question_id",
        "session_id",
        "docname",
        "kind",
        "author_actr_id",
        "author_raw",
        "depot_date",
        "deadline_date",
        "lang",
        "title_nl",
        "title_fr",
        "text_nl",
        "text_fr",
        "main_thesa_nl",
        "main_thesa_fr",
        "oral_refs",
        "qrva_route_ids",
        "internal_ids",
        "source_url",
        "cache_path",
    ];
    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.question_id.clone(),
                r.session_id.to_string(),
                r.docname.clone(),
                r.kind.clone(),
                r.author_actr_id.clone(),
                r.author_raw.clone(),
                r.depot_date.clone(),
                r.deadline_date.clone(),
                r.lang.clone(),
                r.title_nl.clone(),
                r.title_fr.clone(),
                r.text_nl.clone(),
                r.text_fr.clone(),
                r.main_thesa_nl.clone(),
                r.main_thesa_fr.clone(),
                r.oral_refs.clone(),
                r.qrva_route_ids.clone(),
                r.internal_ids.clone(),
                r.source_url.clone(),
                r.cache_path.clone(),
            ]
        })
        .collect();
    write_string_table(path, &fields, &data)
}

pub fn write_written_routes(path: &Path, rows: &[WrittenRouteDraft]) -> Result<(), Box<dyn Error>> {
    let fields = [
        "route_id",
        "question_id",
        "session_id",
        "qrva_id",
        "sdocname",
        "docname",
        "deptnum",
        "deptpres",
        "dept_title_nl",
        "dept_title_fr",
        "subdept_nl",
        "subdept_fr",
        "questnum",
        "statusq",
        "source_url",
        "cache_path",
    ];
    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.route_id.clone(),
                r.question_id.clone(),
                r.session_id.to_string(),
                r.qrva_id.to_string(),
                r.sdocname.clone(),
                r.docname.clone(),
                r.deptnum.clone(),
                r.deptpres.clone(),
                r.dept_title_nl.clone(),
                r.dept_title_fr.clone(),
                r.subdept_nl.clone(),
                r.subdept_fr.clone(),
                r.questnum.clone(),
                r.statusq.clone(),
                r.source_url.clone(),
                r.cache_path.clone(),
            ]
        })
        .collect();
    write_string_table(path, &fields, &data)
}

pub fn write_written_answers(path: &Path, rows: &[AnswerDraft]) -> Result<(), Box<dyn Error>> {
    write_answers_parquet(path, rows)
}
