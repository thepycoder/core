mod parse;

use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, data_dir};
use parquet::arrow::ArrowWriter;
use parse::{ScrapedLobby, dedupe_lobby, parse_lobby_layout};
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tokio::fs;

const LOBBY_PDF_URL: &str = "https://www.dekamer.be/kvvcr/pdf_sections/lobby/lobbyregister.pdf";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let lobby_path = data_dir().join("lobby.parquet");
    fs::create_dir_all(lobby_path.parent().unwrap()).await?;

    let pdf_path = cache_dir().join("lobby/lobbyregister.pdf");

    if !pdf_path.exists() {
        let client = ScrapingClient::new();
        let response = client.get(LOBBY_PDF_URL).await?.error_for_status()?;
        let bytes = response.bytes().await?;
        if !bytes.starts_with(b"%PDF") {
            return Err(format!("{LOBBY_PDF_URL} did not return a PDF").into());
        }
        fs::create_dir_all(pdf_path.parent().unwrap()).await?;
        fs::write(&pdf_path, &bytes).await?;
    }

    let layout = pdftotext_layout(&pdf_path)?;
    let lobby = dedupe_lobby(parse_lobby_layout(&layout));
    if lobby.is_empty() {
        return Err(
            "No lobby entries parsed from the register PDF — aborting Parquet write.".into(),
        );
    }

    write_parquet(&lobby_path, &lobby)?;

    println!("Scraped {} lobby entries.", lobby.len());

    Ok(())
}

fn pdftotext_layout(pdf_path: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("pdftotext")
        .arg("-layout")
        .arg(pdf_path)
        .arg("-")
        .output()
        .map_err(|e| format!("could not run pdftotext (is poppler installed?): {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "pdftotext failed: {}",
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
        ],
    )?;

    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}
