//! Derived report block materialization for QA and the coverage viewer.

use crate::artifact_id::{BLOCK_PARSER_VERSION, REPORT_BLOCK_EXTRACTOR_VERSION};
use crate::report_blocks::ReportBlock;
use arrow::array::{ArrayRef, BooleanArray, RecordBatch, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use serde_json;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ReportBlockRow {
    pub artifact_id: String,
    pub source_content_hash: String,
    pub block_index: u32,
    pub block_type: String,
    pub text: String,
    pub structured_json: String,
    pub language: String,
    pub class_name: String,
    pub word_count: u32,
    pub content_hash: String,
    pub has_oraspr: bool,
    pub block_parser_version: String,
    pub extractor_version: String,
    pub source_url: String,
    pub cache_path: String,
}

pub fn materialize_report_blocks(
    artifact_id: &str,
    source_content_hash: &str,
    blocks: &[ReportBlock],
    source_url: &str,
    cache_path: &str,
) -> Vec<ReportBlockRow> {
    blocks
        .iter()
        .map(|block| {
            let structured = serde_json::json!({
                "inlines": block.inlines,
                "table_rows": block.table_rows,
                "has_oraspr": block.has_oraspr,
            });
            ReportBlockRow {
                artifact_id: artifact_id.to_string(),
                source_content_hash: source_content_hash.to_string(),
                block_index: block.index,
                block_type: block.tag.as_str().to_string(),
                text: block.text.clone(),
                structured_json: structured.to_string(),
                language: block.lang.clone().unwrap_or_default(),
                class_name: block.class.clone().unwrap_or_default(),
                word_count: block.word_count,
                content_hash: block.content_hash.clone(),
                has_oraspr: block.has_oraspr,
                block_parser_version: BLOCK_PARSER_VERSION.to_string(),
                extractor_version: REPORT_BLOCK_EXTRACTOR_VERSION.to_string(),
                source_url: source_url.to_string(),
                cache_path: cache_path.to_string(),
            }
        })
        .collect()
}

pub fn write_report_blocks_parquet(
    path: &Path,
    rows: &[ReportBlockRow],
) -> Result<(), Box<dyn Error>> {
    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }
    let schema = Arc::new(Schema::new(vec![
        Field::new("artifact_id", DataType::Utf8, false),
        Field::new("source_content_hash", DataType::Utf8, false),
        Field::new("block_index", DataType::UInt32, false),
        Field::new("block_type", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new("structured_json", DataType::Utf8, false),
        Field::new("language", DataType::Utf8, false),
        Field::new("class_name", DataType::Utf8, false),
        Field::new("word_count", DataType::UInt32, false),
        Field::new("content_hash", DataType::Utf8, false),
        Field::new("has_oraspr", DataType::Boolean, false),
        Field::new("block_parser_version", DataType::Utf8, false),
        Field::new("extractor_version", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            col!(|r| r.artifact_id.clone()),
            col!(|r| r.source_content_hash.clone()),
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.block_index).collect::<Vec<_>>(),
            )) as ArrayRef,
            col!(|r| r.block_type.clone()),
            col!(|r| r.text.clone()),
            col!(|r| r.structured_json.clone()),
            col!(|r| r.language.clone()),
            col!(|r| r.class_name.clone()),
            Arc::new(UInt32Array::from(
                rows.iter().map(|r| r.word_count).collect::<Vec<_>>(),
            )) as ArrayRef,
            col!(|r| r.content_hash.clone()),
            Arc::new(BooleanArray::from(
                rows.iter().map(|r| r.has_oraspr).collect::<Vec<_>>(),
            )) as ArrayRef,
            col!(|r| r.block_parser_version.clone()),
            col!(|r| r.extractor_version.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
        ],
    )?;
    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}
