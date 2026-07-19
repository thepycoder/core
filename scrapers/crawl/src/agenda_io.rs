use crate::agenda_timeline::{AgendaItem, MeetingKind};
use crate::utils::agenda_item_id;
use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AgendaItemDraft {
    pub agenda_item_id: String,
    pub session_id: u32,
    pub meeting_id: u32,
    pub meeting_kind: MeetingKind,
    pub agenda_id: String,
    pub item_kind: String,
    pub item_id: String,
    pub title_nl: String,
    pub title_fr: String,
    pub dossier_id: String,
    pub document_id: String,
    pub internal_ids: String,
    pub start_block: u32,
    pub end_block: u32,
    pub title_blocks: String,
    pub source_section: String,
    pub source_url: String,
    pub cache_path: String,
}

pub fn materialize_agenda_items(
    agenda: &[AgendaItem],
    meeting_kind: MeetingKind,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
) -> Vec<AgendaItemDraft> {
    agenda
        .iter()
        .map(|item| AgendaItemDraft {
            agenda_item_id: agenda_item_id(
                session_id,
                meeting_kind.as_str(),
                meeting_id,
                item.start_block,
            ),
            session_id,
            meeting_id,
            meeting_kind,
            agenda_id: item.agenda_id.clone(),
            item_kind: item.item_kind.as_str().to_string(),
            item_id: item.item_id.clone(),
            title_nl: item.title_nl.clone(),
            title_fr: item.title_fr.clone(),
            dossier_id: item.dossier_id.clone(),
            document_id: item.document_id.clone(),
            internal_ids: item.internal_ids.join(","),
            start_block: item.start_block,
            end_block: item.end_block,
            title_blocks: item
                .title_blocks
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(","),
            source_section: item.source_section.clone(),
            source_url: source_url.to_string(),
            cache_path: cache_path.to_string(),
        })
        .collect()
}

macro_rules! col {
    ($rows:expr, $f:expr) => {
        Arc::new(StringArray::from($rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
    };
}

pub fn write_agenda_items_parquet(
    path: &Path,
    rows: &[AgendaItemDraft],
) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("agenda_item_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("meeting_kind", DataType::Utf8, false),
        Field::new("agenda_id", DataType::Utf8, false),
        Field::new("item_kind", DataType::Utf8, false),
        Field::new("item_id", DataType::Utf8, false),
        Field::new("title_nl", DataType::Utf8, false),
        Field::new("title_fr", DataType::Utf8, false),
        Field::new("dossier_id", DataType::Utf8, false),
        Field::new("document_id", DataType::Utf8, false),
        Field::new("internal_ids", DataType::Utf8, false),
        Field::new("start_block", DataType::Utf8, false),
        Field::new("end_block", DataType::Utf8, false),
        Field::new("title_blocks", DataType::Utf8, false),
        Field::new("source_section", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            col!(rows, |r| r.agenda_item_id.clone()),
            col!(rows, |r| r.session_id.to_string()),
            col!(rows, |r| r.meeting_id.to_string()),
            col!(rows, |r| r.meeting_kind.as_str().to_string()),
            col!(rows, |r| r.agenda_id.clone()),
            col!(rows, |r| r.item_kind.clone()),
            col!(rows, |r| r.item_id.clone()),
            col!(rows, |r| r.title_nl.clone()),
            col!(rows, |r| r.title_fr.clone()),
            col!(rows, |r| r.dossier_id.clone()),
            col!(rows, |r| r.document_id.clone()),
            col!(rows, |r| r.internal_ids.clone()),
            col!(rows, |r| r.start_block.to_string()),
            col!(rows, |r| r.end_block.to_string()),
            col!(rows, |r| r.title_blocks.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agenda_timeline::{ItemKind, build_agenda_timeline};
    use crate::report_blocks::{parse_report_blocks, read_report_html};

    #[test]
    fn materialize_assigns_agenda_item_id_from_start_block() {
        let item = AgendaItem {
            agenda_id: "15".into(),
            item_kind: ItemKind::Proposition,
            start_block: 596,
            end_block: 700,
            title_nl: "Wetsontwerp (56/318)".into(),
            title_fr: String::new(),
            dossier_id: "56/318".into(),
            document_id: "1-9".into(),
            internal_ids: vec![],
            item_id: String::new(),
            source_section: "propositions de loi".into(),
            title_blocks: vec![596],
        };
        let rows = materialize_agenda_items(
            &[item],
            MeetingKind::Plenary,
            56,
            42,
            "https://example.test",
            "cache/x.html",
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].agenda_item_id, "56_plenary_42_agenda_596");
        assert_eq!(rows[0].dossier_id, "56/318");
        assert_eq!(rows[0].item_kind, "proposition");
    }

    #[test]
    fn proposition_heading_fixture_gets_dossier_and_agenda_item_id() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../cache/sessions/56/meetings/plenary/56-42.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = scraper::Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        let agenda = build_agenda_timeline(&blocks, MeetingKind::Plenary, 56, 42);
        let rows = materialize_agenda_items(
            &agenda,
            MeetingKind::Plenary,
            56,
            42,
            "https://www.dekamer.be/doc/PCRI/html/56/ip042x.html",
            "sessions/56/meetings/plenary/56-42.html",
        );
        let with_dossier: Vec<_> = rows
            .iter()
            .filter(|r| r.dossier_id == "56/318")
            .collect();
        assert!(
            !with_dossier.is_empty(),
            "expected agenda item for dossier 56/318"
        );
        assert!(with_dossier[0]
            .agenda_item_id
            .starts_with("56_plenary_42_agenda_"));
    }
}
