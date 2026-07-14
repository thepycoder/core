use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::utils::relative_cache_path;
use lobby::{dedupe_lobby, extract_lobby_from_layout, LOBBY_PDF_URL, ScrapedLobby};
use parquet::arrow::ArrowWriter;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tokio::fs;

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
    let lobby = dedupe_lobby(extract_lobby_from_layout(
        &layout_text,
        LOBBY_PDF_URL,
        &cache_path,
    )?);
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
