use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::utils::relative_cache_path;
use crawl::{
    BundlePublisher, MANIFEST_STATUS_PARSED, SourceManifestRow, content_hash_bytes, looks_like_pdf,
    manifest_path, now_rfc3339, read_cache_metadata, require_cache_present, touch_checked_at,
    validate_manifest_rows, write_cache_artifact, write_source_manifest,
};
use lobby::{LOBBY_PDF_URL, ScrapedLobby, dedupe_lobby, extract_lobby_from_layout};
use parquet::arrow::ArrowWriter;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

const SOURCE_NAME: &str = "lobby";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let lobby_path = data_dir().join("lobby.parquet");
    let pdf_path = cache_dir().join("lobby/lobbyregister.pdf");
    std::fs::create_dir_all(lobby_path.parent().unwrap())?;
    std::fs::create_dir_all(pdf_path.parent().unwrap())?;

    let run_mode = if cache_only() {
        SourceManifestRow::run_mode_cache_only()
    } else {
        SourceManifestRow::run_mode_live()
    };

    if cache_only() {
        require_cache_present(&pdf_path, "lobby register PDF")?;
    } else {
        let client = ScrapingClient::new();
        let response = client.get(LOBBY_PDF_URL).await?;
        if !response.status().is_success() {
            return Err(
                format!("lobby PDF download failed with HTTP {}", response.status()).into(),
            );
        }
        let bytes = response.bytes().await?;
        if !looks_like_pdf(&bytes) {
            return Err("lobby response is not a PDF (missing %PDF magic)".into());
        }

        // Parse candidate from temp file before promoting into the cache.
        let candidate = pdf_path.with_extension("pdf.candidate");
        std::fs::write(&candidate, &bytes)?;
        let layout = pdftotext_layout(&candidate).map_err(|e| {
            let _ = std::fs::remove_file(&candidate);
            e
        })?;
        let preview = extract_lobby_from_layout(&layout, LOBBY_PDF_URL, "candidate")?;
        if preview.is_empty() {
            let _ = std::fs::remove_file(&candidate);
            return Err("lobby PDF parsed to zero organisations — aborting".into());
        }
        let _ = std::fs::remove_file(&candidate);

        if pdf_path.exists() {
            let existing = std::fs::read(&pdf_path)?;
            if existing.as_slice() == bytes.as_ref() {
                touch_checked_at(&pdf_path)?;
            } else {
                write_cache_artifact(&pdf_path, &bytes, LOBBY_PDF_URL, "application/pdf")?;
            }
        } else {
            write_cache_artifact(&pdf_path, &bytes, LOBBY_PDF_URL, "application/pdf")?;
        }
    }

    let cache_path = relative_cache_path(&pdf_path, &cache_dir());
    let layout_text = pdftotext_layout(&pdf_path)?;
    let lobby = dedupe_lobby(extract_lobby_from_layout(
        &layout_text,
        LOBBY_PDF_URL,
        &cache_path,
    )?);
    if lobby.is_empty() {
        return Err("lobby parquet would be empty — aborting".into());
    }

    let meta = read_cache_metadata(&pdf_path)?;
    let content_hash = meta
        .as_ref()
        .map(|m| m.content_hash.clone())
        .unwrap_or_else(|| {
            std::fs::read(&pdf_path)
                .map(|b| content_hash_bytes(&b))
                .unwrap_or_default()
        });
    let fetched_at = meta
        .as_ref()
        .map(|m| m.fetched_at.clone())
        .unwrap_or_else(now_rfc3339);
    let checked_at = meta
        .as_ref()
        .map(|m| m.checked_at.clone())
        .unwrap_or_else(now_rfc3339);

    let manifest_rows = vec![SourceManifestRow {
        source: SOURCE_NAME.into(),
        session_id: String::new(),
        item_kind: "register_pdf".into(),
        native_item_id: "lobbyregister".into(),
        source_url: LOBBY_PDF_URL.into(),
        cache_path: cache_path.clone(),
        status: MANIFEST_STATUS_PARSED.into(),
        row_count: lobby.len() as u32,
        content_type: "application/pdf".into(),
        content_hash,
        fetched_at,
        checked_at,
        run_mode: run_mode.into(),
        detail: String::new(),
    }];
    validate_manifest_rows(&manifest_rows)?;

    let manifest_final = manifest_path(SOURCE_NAME);
    let mut bundle = BundlePublisher::new("lobby", &data_dir())?;
    let stage_parquet = bundle.stage_path(&lobby_path)?;
    let stage_manifest = bundle.stage_path(&manifest_final)?;
    write_parquet(&stage_parquet, &lobby)?;
    write_source_manifest(&stage_manifest, &manifest_rows)?;
    bundle.commit()?;

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
