use crate::utils::clean_text;
use encoding_rs::WINDOWS_1252;
use scraper::{ElementRef, Html, Selector};
use std::error::Error;
use std::fs::read;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
pub struct ReportBlock {
    pub index: u32,
    pub tag: BlockTag,
    pub text: String,
    pub lang: Option<String>,
    pub class: Option<String>,
    pub has_oraspr: bool,
}

static SELECTOR_BLOCKS: OnceLock<Selector> = OnceLock::new();

fn selector_blocks() -> &'static Selector {
    SELECTOR_BLOCKS.get_or_init(|| Selector::parse("h1, h2, p, table").unwrap())
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

        let text = block_text(&element, tag);
        if text.is_empty() {
            continue;
        }

        let has_oraspr = element
            .select(&Selector::parse("span.oraspr").unwrap())
            .next()
            .is_some();
        blocks.push(ReportBlock {
            index,
            tag,
            text,
            lang: element.value().attr("lang").map(str::to_string),
            class: element.value().attr("class").map(str::to_string),
            has_oraspr,
        });
        index += 1;
    }

    blocks
}

fn block_text(element: &ElementRef<'_>, tag: BlockTag) -> String {
    let raw = element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\n', " ");
    let cleaned = clean_text(&raw);
    if tag == BlockTag::Table {
        compact_table_text(&cleaned)
    } else {
        cleaned
    }
}

fn compact_table_text(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 240 {
        let truncated: String = collapsed.chars().take(240).collect();
        format!("{truncated}…")
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn compact_table_text_truncates_on_char_boundary() {
        let text = "cafè ".repeat(50);
        let out = compact_table_text(&text);
        assert!(out.ends_with('…'));
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn parses_plenary_fixture_blocks() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../partijgedrag/core/cache/sessions/56/meetings/plenary/56-117.html");
        if !path.exists() {
            return;
        }
        let html = read_report_html(&path).unwrap();
        let document = Html::parse_document(&html);
        let blocks = parse_report_blocks(&document);
        assert!(blocks.iter().any(|b| b.tag == BlockTag::H1));
        assert!(blocks.iter().filter(|b| b.tag == BlockTag::P).count() > 100);
    }
}
