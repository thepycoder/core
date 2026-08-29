use crate::artifact_id::content_hash;
use crate::utils::clean_text;
use encoding_rs::WINDOWS_1252;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::read;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockTag {
    H1,
    H2,
    P,
    Table,
}

impl BlockTag {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockTag::H1 => "h1",
            BlockTag::H2 => "h2",
            BlockTag::P => "p",
            BlockTag::Table => "table",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InlineSpan {
    pub lang: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableCell {
    pub text: String,
    pub colspan: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone)]
pub struct ReportBlock {
    pub index: u32,
    pub tag: BlockTag,
    /// Full normalized text for QA coverage and search.
    pub text: String,
    pub inlines: Vec<InlineSpan>,
    pub table_rows: Option<Vec<TableRow>>,
    pub lang: Option<String>,
    pub class: Option<String>,
    pub has_oraspr: bool,
    pub content_hash: String,
    pub word_count: u32,
}

static SELECTOR_BLOCKS: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TR: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TD: OnceLock<Selector> = OnceLock::new();
static SELECTOR_SPAN: OnceLock<Selector> = OnceLock::new();

fn selector_blocks() -> &'static Selector {
    SELECTOR_BLOCKS.get_or_init(|| Selector::parse("h1, h2, p, table").unwrap())
}

fn selector_tr() -> &'static Selector {
    SELECTOR_TR.get_or_init(|| Selector::parse("tr").unwrap())
}

fn selector_td() -> &'static Selector {
    SELECTOR_TD.get_or_init(|| Selector::parse("td, th").unwrap())
}

fn selector_span() -> &'static Selector {
    SELECTOR_SPAN.get_or_init(|| Selector::parse("span").unwrap())
}

pub fn read_report_html(path: &Path) -> Result<String, Box<dyn Error>> {
    let raw = read(path)?;
    if let Ok(text) = std::str::from_utf8(&raw) {
        return Ok(text.to_string());
    }
    let (decoded, _, _) = WINDOWS_1252.decode(&raw);
    Ok(decoded.into_owned())
}

pub fn parse_report_blocks(document: &Html) -> Vec<ReportBlock> {
    let mut blocks = Vec::new();
    let mut index = 0u32;

    for element in document.select(selector_blocks()) {
        let tag = match element.value().name() {
            "h1" => BlockTag::H1,
            "h2" => BlockTag::H2,
            "p" => BlockTag::P,
            "table" => BlockTag::Table,
            _ => continue,
        };

        if tag == BlockTag::P && is_inside_table(&element) {
            continue;
        }

        let inlines = parse_inlines(&element);
        let table_rows = if tag == BlockTag::Table {
            Some(parse_table_rows(&element))
        } else {
            None
        };
        let text = block_text(&element, tag, table_rows.as_ref());
        if text.is_empty() {
            continue;
        }

        let has_oraspr = element
            .select(&Selector::parse("span.oraspr").unwrap())
            .next()
            .is_some();
        let word_count = text.split_whitespace().count() as u32;
        let lang = element.value().attr("lang").map(str::to_string);
        let class = element.value().attr("class").map(str::to_string);
        let canonical = serde_json::json!({
            "block_type": tag.as_str(),
            "class_name": class,
            "has_oraspr": has_oraspr,
            "inlines": inlines,
            "language": lang,
            "table_rows": table_rows,
            "text": text,
        });
        blocks.push(ReportBlock {
            index,
            tag,
            text: text.clone(),
            inlines,
            table_rows,
            lang,
            class,
            has_oraspr,
            content_hash: content_hash(&canonical.to_string()),
            word_count,
        });
        index += 1;
    }

    blocks
}

fn is_inside_table(element: &ElementRef<'_>) -> bool {
    element
        .ancestors()
        .any(|a| a.value().as_element().is_some_and(|e| e.name() == "table"))
}

fn parse_inlines(element: &ElementRef<'_>) -> Vec<InlineSpan> {
    let mut spans = Vec::new();
    for span in element.select(selector_span()) {
        let text = clean_text(&span.text().collect::<Vec<_>>().join(" "));
        if text.is_empty() {
            continue;
        }
        spans.push(InlineSpan {
            lang: span.value().attr("lang").map(str::to_string),
            text,
        });
    }
    if spans.is_empty() {
        let text = clean_text(&element.text().collect::<Vec<_>>().join(" "));
        if !text.is_empty() {
            spans.push(InlineSpan {
                lang: element.value().attr("lang").map(str::to_string),
                text,
            });
        }
    }
    spans
}

fn parse_table_rows(table: &ElementRef<'_>) -> Vec<TableRow> {
    let mut rows = Vec::new();
    for tr in table.select(selector_tr()) {
        let cells: Vec<TableCell> = tr
            .select(selector_td())
            .map(|td| {
                let text = clean_text(&td.text().collect::<Vec<_>>().join(" "));
                let colspan = td
                    .value()
                    .attr("colspan")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                TableCell { text, colspan }
            })
            .collect();
        if !cells.is_empty() {
            rows.push(TableRow { cells });
        }
    }
    rows
}

fn block_text(
    element: &ElementRef<'_>,
    tag: BlockTag,
    table_rows: Option<&Vec<TableRow>>,
) -> String {
    if tag == BlockTag::Table
        && let Some(rows) = table_rows
    {
        return table_rows_text(rows);
    }
    let raw = element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\n', " ");
    clean_text(&raw)
}

pub fn table_rows_text(rows: &[TableRow]) -> String {
    rows.iter()
        .flat_map(|row| row.cells.iter().map(|c| c.text.as_str()))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn table_row_labels(row: &TableRow) -> Vec<String> {
    row.cells.iter().map(|c| c.text.clone()).collect()
}

pub fn table_row_numeric_cells(row: &TableRow) -> Vec<u32> {
    row.cells
        .iter()
        .filter_map(|c| c.text.trim().parse::<u32>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Html {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/votes")
            .join(name);
        let html = read_report_html(&path).expect("fixture html");
        Html::parse_document(&html)
    }

    #[test]
    fn read_report_html_accepts_utf8_and_windows_1252() {
        let dir = std::env::temp_dir().join("crawl_read_report_html");
        let _ = std::fs::create_dir_all(&dir);

        let utf8_path = dir.join("utf8.html");
        std::fs::write(&utf8_path, "<p>café</p>").unwrap();
        assert_eq!(read_report_html(&utf8_path).unwrap(), "<p>café</p>");

        let cp1252_path = dir.join("cp1252.html");
        std::fs::write(&cp1252_path, b"<p>caf\xe9</p>").unwrap();
        assert_eq!(read_report_html(&cp1252_path).unwrap(), "<p>café</p>");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_paragraphs_inside_tables() {
        let document = fixture("roll_call_compact.html");
        let blocks = parse_report_blocks(&document);
        let table_blocks: Vec<_> = blocks.iter().filter(|b| b.tag == BlockTag::Table).collect();
        assert!(!table_blocks.is_empty());
        for table in &table_blocks {
            assert!(table.table_rows.is_some());
        }
        assert!(
            !table_blocks.is_empty(),
            "appendix name paragraphs may exist as separate p blocks"
        );
    }

    #[test]
    fn roll_call_fixture_has_structured_table() {
        let document = fixture("roll_call_compact.html");
        let blocks = parse_report_blocks(&document);
        let vote_table = blocks
            .iter()
            .find(|b| b.tag == BlockTag::Table && b.text.contains("Stemming/vote"))
            .expect("vote table");
        let rows = vote_table.table_rows.as_ref().unwrap();
        assert!(rows.len() >= 4);
    }

    #[test]
    fn content_hash_covers_canonical_block_structure() {
        let first = Html::parse_document(
            r#"<p class="Normal" lang="NL"><span class="oraspr">Zelfde tekst</span></p>"#,
        );
        let reordered = Html::parse_document(
            r#"<p lang="NL" class="Normal"><span class="oraspr">Zelfde tekst</span></p>"#,
        );
        let without_oraspr =
            Html::parse_document(r#"<p lang="NL" class="Normal"><span>Zelfde tekst</span></p>"#);

        let first_block = &parse_report_blocks(&first)[0];
        let reordered_block = &parse_report_blocks(&reordered)[0];
        let plain_block = &parse_report_blocks(&without_oraspr)[0];

        assert_eq!(first_block.content_hash, reordered_block.content_hash);
        assert_ne!(first_block.content_hash, plain_block.content_hash);
        assert!(first_block.has_oraspr);
    }

    #[test]
    fn parses_plenary_fixture_blocks_when_cache_present() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../partijgedrag/core/cache/sessions/56/meetings/plenary/56-117.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        assert!(blocks.iter().any(|b| b.tag == BlockTag::H1));
        assert!(blocks.iter().filter(|b| b.tag == BlockTag::P).count() > 50);
    }
}
