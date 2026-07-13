use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::utils::relative_cache_path;
use parquet::arrow::ArrowWriter;
use regex::Regex;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, LazyLock};
use tokio::fs;

const LOBBY_PDF_URL: &str = "https://www.dekamer.be/kvvcr/pdf_sections/lobby/lobbyregister.pdf";
const COLUMN_CUTS: [usize; 5] = [0, 31, 62, 112, 500];

static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:https?://|www\.)[\w./\-]+").unwrap());

#[derive(Debug, Default, Clone)]
struct ScrapedLobby {
    name: String,
    contacts: String,
    interests: String,
    url: String,
    source_url: String,
    cache_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let client = ScrapingClient::new();
    let lobby_path = data_dir().join("lobby.parquet");
    let pdf_path = cache_dir().join("lobby/lobbyregister.pdf");

    fs::create_dir_all(lobby_path.parent().unwrap()).await?;
    fs::create_dir_all(pdf_path.parent().unwrap()).await?;

    if !pdf_path.exists() {
        if cache_only() {
            return Err(format!(
                "lobby PDF cache missing at {} (SCRAPER_CACHE_ONLY)",
                pdf_path.display()
            )
            .into());
        }
        let response = client.get(LOBBY_PDF_URL).await?;
        let bytes = response.bytes().await?;
        fs::write(&pdf_path, &bytes).await?;
    }

    let cache_path = relative_cache_path(&pdf_path, &cache_dir());
    let layout_text = pdftotext_layout(&pdf_path)?;
    let lobby = dedupe_lobby(extract_lobby_from_layout(&layout_text, LOBBY_PDF_URL, &cache_path)?);
    write_parquet(&lobby_path, &lobby)?;

    println!("Scraped {} lobby entries.", lobby.len());

    Ok(())
}

fn pdftotext_layout(pdf_path: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("pdftotext")
        .args(["-layout", pdf_path.to_str().unwrap(), "-"])
        .output()?;

    if !output.status.success() {
        return Err(format!(
            "pdftotext failed (is poppler installed?): {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn extract_lobby_from_layout(
    text: &str,
    source_url: &str,
    cache_path: &str,
) -> Result<Vec<ScrapedLobby>, Box<dyn Error>> {
    let mut entries = Vec::new();
    let mut current: Option<ScrapedLobby> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() || should_skip_line(line) {
            continue;
        }

        let cols = slice_columns(line);
        if !cols.iter().any(|value| !value.is_empty()) {
            continue;
        }

        let is_new_entry = !cols[0].is_empty();
        if is_new_entry {
            if let Some(entry) = current.take() {
                entries.push(finalize_entry(entry, &current_lines));
                current_lines.clear();
            }
            current = Some(ScrapedLobby {
                name: cols[0].clone(),
                contacts: cols[1].clone(),
                interests: cols[2].clone(),
                url: cols[3].clone(),
                source_url: source_url.to_string(),
                cache_path: cache_path.to_string(),
            });
            current_lines.push(line.to_string());
        } else if let Some(entry) = current.as_mut() {
            current_lines.push(line.to_string());
            merge_columns(entry, &cols);
        }
    }

    if let Some(entry) = current {
        entries.push(finalize_entry(entry, &current_lines));
    }

    entries.retain(|entry| {
        !entry.name.is_empty()
            && !Regex::new(r"^\d{2}-\d{2}-\d{2}$")
                .unwrap()
                .is_match(&entry.name)
    });

    Ok(entries)
}

fn dedupe_lobby(rows: Vec<ScrapedLobby>) -> Vec<ScrapedLobby> {
    let mut by_name: std::collections::HashMap<String, ScrapedLobby> = std::collections::HashMap::new();
    for row in rows {
        by_name
            .entry(row.name.clone())
            .and_modify(|existing| merge_lobby_row(existing, &row))
            .or_insert(row);
    }
    let mut out: Vec<_> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn merge_lobby_row(keep: &mut ScrapedLobby, other: &ScrapedLobby) {
    if other.url.len() > keep.url.len() {
        keep.url = other.url.clone();
    }
    if other.contacts.len() > keep.contacts.len() {
        keep.contacts = other.contacts.clone();
    }
    if other.interests.len() > keep.interests.len() {
        keep.interests = other.interests.clone();
    }
}

fn should_skip_line(line: &str) -> bool {
    const SKIP_PATTERNS: &[&str] = &[
        "organisme",
        "organisation",
        "contactpersonen",
        "personnes de contact",
        "behartigt belangen",
        "gère des intérêts",
        "LOBBYREGISTER",
        "laatst bijgewerkt",
        "dernière mise",
    ];

    SKIP_PATTERNS.iter().any(|pattern| line.contains(pattern))
}

fn slice_columns(line: &str) -> [String; 4] {
    let mut cols = [String::new(), String::new(), String::new(), String::new()];

    for (index, value) in cols.iter_mut().enumerate() {
        let start = byte_index_at_or_before(line, COLUMN_CUTS[index]);
        let end = byte_index_at_or_before(line, COLUMN_CUTS[index + 1].min(line.len()));
        if start < end {
            *value = line[start..end].trim().to_string();
        }
    }

    cols
}

fn byte_index_at_or_before(text: &str, byte_index: usize) -> usize {
    if byte_index >= text.len() {
        return text.len();
    }

    let mut index = byte_index;
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn merge_columns(entry: &mut ScrapedLobby, cols: &[String; 4]) {
    merge_field(&mut entry.contacts, &cols[1]);
    merge_field(&mut entry.interests, &cols[2]);
    merge_field(&mut entry.url, &cols[3]);
}

fn merge_field(target: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }

    if target.is_empty() {
        *target = value.to_string();
    } else {
        target.push(' ');
        target.push_str(value);
    }
}

fn finalize_entry(mut entry: ScrapedLobby, lines: &[String]) -> ScrapedLobby {
    let blob = lines.join(" ");
    if let Some(url_match) = URL_RE.find(&blob) {
        entry.url = url_match.as_str().trim_end_matches('.').to_string();
    }

    entry
}

fn write_parquet(path: &Path, rows: &[ScrapedLobby]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("name", DataType::Utf8, false),
        Field::new("contacts", DataType::Utf8, false),
        Field::new("interests", DataType::Utf8, false),
        Field::new("url", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            col!(|r| r.name.clone()),
            col!(|r| r.contacts.clone()),
            col!(|r| r.interests.clone()),
            col!(|r| r.url.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
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

    #[test]
    fn parses_layout_sample() {
        let sample = "\
       organisme                   contactpersonen                behartigt belangen voor                           WEB
11.11.11                        Naima Charkaoui          koepel van de Vlaamse Noord-               www.11.be
                                                         Zuidbeweging.
AB InBev                        Aron Wils                brouwen, verkoop en marketing van          www.ab-inbev.be
                                                         bieren.";

        let entries =
            extract_lobby_from_layout(sample, LOBBY_PDF_URL, "lobby/lobbyregister.pdf").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "11.11.11");
        assert_eq!(entries[0].url, "www.11.be");
        assert_eq!(entries[1].name, "AB InBev");
        assert_eq!(entries[1].url, "www.ab-inbev.be");
    }
}
