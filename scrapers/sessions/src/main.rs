use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::utils::relative_cache_path;
use crawl::{
    BundlePublisher, MANIFEST_STATUS_PARSED, SourceManifestRow, content_hash_bytes, manifest_path,
    now_rfc3339, read_cache_metadata, require_cache_present, touch_checked_at,
    validate_manifest_rows, write_cache_artifact, write_source_manifest,
};
use parquet::arrow::ArrowWriter;
use scraper::{Html, Selector};
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, LazyLock};

static SEL_SESSION: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("div a[href*='showpage.cfm']").unwrap());

const SOURCE_NAME: &str = "sessions";
const INDEX_URL: &str = "https://www.dekamer.be/kvvcr/showpage.cfm?section=/depute&language=nl&cfm=/site/wwwcfm/depute/cvlist54.cfm";

#[derive(Debug)]
struct ScrapedSession {
    session_id: String,
    start_date: String,
    end_date: String,
    source_url: String,
    cache_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let index_cache = cache_dir().join("sessions/index.html");
    std::fs::create_dir_all(index_cache.parent().unwrap())?;

    let parquet_path = data_dir().join("sessions.parquet");
    let run_mode = if cache_only() {
        SourceManifestRow::run_mode_cache_only()
    } else {
        SourceManifestRow::run_mode_live()
    };

    if cache_only() {
        require_cache_present(&index_cache, "sessions index")?;
    } else {
        let client = crawl::client::ScrapingClient::new();
        let html = client.get(INDEX_URL).await?.text().await?;
        let bytes = html.as_bytes();
        if index_cache.exists() {
            let existing = std::fs::read(&index_cache)?;
            if existing.as_slice() == bytes {
                touch_checked_at(&index_cache)?;
            } else {
                write_cache_artifact(&index_cache, bytes, INDEX_URL, "text/html")?;
            }
        } else {
            write_cache_artifact(&index_cache, bytes, INDEX_URL, "text/html")?;
        }
    }

    let content = std::fs::read_to_string(&index_cache)?;
    let document = Html::parse_document(&content);
    let rel_cache = relative_cache_path(&index_cache, &cache_dir());
    let sessions = extract_sessions(&document, INDEX_URL, &rel_cache);
    if sessions.is_empty() {
        return Err("sessions index produced zero session rows — aborting".into());
    }

    let meta = read_cache_metadata(&index_cache)?;
    let content_hash = meta
        .as_ref()
        .map(|m| m.content_hash.clone())
        .unwrap_or_else(|| content_hash_bytes(content.as_bytes()));
    let fetched_at = meta
        .as_ref()
        .map(|m| m.fetched_at.clone())
        .unwrap_or_else(now_rfc3339);
    let checked_at = meta
        .as_ref()
        .map(|m| m.checked_at.clone())
        .unwrap_or_else(now_rfc3339);

    let mut manifest_rows = Vec::with_capacity(sessions.len() + 1);
    manifest_rows.push(SourceManifestRow {
        source: SOURCE_NAME.into(),
        session_id: String::new(),
        item_kind: "index".into(),
        native_item_id: "sessions_index".into(),
        source_url: INDEX_URL.into(),
        cache_path: rel_cache.clone(),
        status: MANIFEST_STATUS_PARSED.into(),
        row_count: sessions.len() as u32,
        content_type: "text/html".into(),
        content_hash: content_hash.clone(),
        fetched_at: fetched_at.clone(),
        checked_at: checked_at.clone(),
        run_mode: run_mode.into(),
        detail: String::new(),
    });
    for s in &sessions {
        manifest_rows.push(SourceManifestRow {
            source: SOURCE_NAME.into(),
            session_id: s.session_id.clone(),
            item_kind: "session".into(),
            native_item_id: s.session_id.clone(),
            source_url: s.source_url.clone(),
            cache_path: s.cache_path.clone(),
            status: MANIFEST_STATUS_PARSED.into(),
            row_count: 1,
            content_type: "text/html".into(),
            content_hash: content_hash.clone(),
            fetched_at: fetched_at.clone(),
            checked_at: checked_at.clone(),
            run_mode: run_mode.into(),
            detail: String::new(),
        });
    }
    validate_manifest_rows(&manifest_rows)?;

    let manifest_final = manifest_path(SOURCE_NAME);
    let mut bundle = BundlePublisher::new("sessions", &data_dir())?;
    let stage_parquet = bundle.stage_path(&parquet_path)?;
    let stage_manifest = bundle.stage_path(&manifest_final)?;
    write_parquet(&stage_parquet, &sessions)?;
    write_source_manifest(&stage_manifest, &manifest_rows)?;
    bundle.commit()?;

    println!(
        "Written {} sessions to {}.",
        sessions.len(),
        parquet_path.display()
    );
    Ok(())
}

fn write_parquet(path: &Path, rows: &[ScrapedSession]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("session_id", DataType::Utf8, false),
        Field::new("start_date", DataType::Utf8, false),
        Field::new("end_date", DataType::Utf8, false),
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
            col!(|r| r.session_id.clone()),
            col!(|r| r.start_date.clone()),
            col!(|r| r.end_date.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
        ],
    )?;

    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

fn extract_sessions(document: &Html, source_url: &str, cache_path: &str) -> Vec<ScrapedSession> {
    let mut sessions = Vec::new();

    for element in document.select(&SEL_SESSION) {
        let href = match element.value().attr("href") {
            Some(h) => h,
            None => continue,
        };
        let session_id = match href
            .split("legis=")
            .nth(1)
            .and_then(|s| s.split('&').next())
        {
            Some(id) => id.to_string(),
            None => continue,
        };
        let text = element.text().collect::<String>();
        let date_str = text
            .split('(')
            .nth(1)
            .and_then(|s| s.split(')').next())
            .unwrap_or("");

        let parts: Vec<&str> = date_str.split('-').collect();
        if parts.len() != 2 {
            continue;
        }

        sessions.push(ScrapedSession {
            session_id,
            start_date: parts[0].trim().to_string(),
            end_date: parts[1].trim().to_string(),
            source_url: source_url.to_string(),
            cache_path: cache_path.to_string(),
        });
    }

    sessions
}
