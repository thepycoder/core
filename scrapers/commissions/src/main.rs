use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use commissions::{CommissionRole, extract_members};
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::utils::relative_cache_path;
use crawl::{
    BundlePublisher, MANIFEST_STATUS_PARSED, SourceManifestRow, content_hash_bytes, manifest_path,
    now_rfc3339, read_cache_metadata, require_cache_present, touch_checked_at,
    validate_manifest_rows, write_cache_artifact, write_source_manifest,
};
use parquet::arrow::ArrowWriter;
use scraper::{Html, Selector};
use std::collections::BTreeSet;
use std::error::Error;
use std::fs::{File, read_to_string};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

static SEL_INDEX: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("div.linklist_0 > a, h4").unwrap());

const SOURCE_NAME: &str = "commissions";
const INDEX_URL: &str = "https://www.dekamer.be/kvvcr/showpage.cfm?section=/none&language=nl&cfm=/site/wwwcfm/comm/LstCom.cfm";

#[derive(Debug)]
struct ScrapedCommission {
    name: String,
    ctype: String,
    chairs: String,
    subchairs: String,
    permanent_members: String,
    replacement_members: String,
    source_url: String,
    cache_path: String,
}

struct CommissionIndex {
    name: String,
    ctype: String,
    url: String,
    cache_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let detail_dir = cache_dir().join("commissions/details");
    let index_cache = cache_dir().join("commissions/commissions.html");
    let parquet_path = data_dir().join("commissions.parquet");

    std::fs::create_dir_all(&detail_dir)?;
    std::fs::create_dir_all(parquet_path.parent().unwrap())?;

    let run_mode = if cache_only() {
        SourceManifestRow::run_mode_cache_only()
    } else {
        SourceManifestRow::run_mode_live()
    };

    let client = if cache_only() {
        require_cache_present(&index_cache, "commissions index")?;
        None
    } else {
        let client = ScrapingClient::new();
        refresh_html_cache(&client, INDEX_URL, &index_cache).await?;
        Some(client)
    };

    let index_html = read_to_string(&index_cache)?;
    let index_doc = Html::parse_document(&index_html);
    let index = extract_index(&index_doc, &detail_dir);
    let index_names: BTreeSet<String> = index.iter().map(|e| e.name.clone()).collect();

    let index_rel = relative_cache_path(&index_cache, &cache_dir());
    let index_meta = read_cache_metadata(&index_cache)?;
    let index_hash = index_meta
        .as_ref()
        .map(|m| m.content_hash.clone())
        .unwrap_or_else(|| content_hash_bytes(index_html.as_bytes()));
    let index_fetched_at = index_meta
        .as_ref()
        .map(|m| m.fetched_at.clone())
        .unwrap_or_else(now_rfc3339);
    let index_checked_at = index_meta
        .as_ref()
        .map(|m| m.checked_at.clone())
        .unwrap_or_else(now_rfc3339);

    let mut manifest_rows = Vec::with_capacity(index.len() + 1);
    manifest_rows.push(SourceManifestRow {
        source: SOURCE_NAME.into(),
        session_id: String::new(),
        item_kind: "index".into(),
        native_item_id: "commissions_index".into(),
        source_url: INDEX_URL.into(),
        cache_path: index_rel,
        status: MANIFEST_STATUS_PARSED.into(),
        row_count: index.len() as u32,
        content_type: "text/html".into(),
        content_hash: index_hash,
        fetched_at: index_fetched_at,
        checked_at: index_checked_at,
        run_mode: run_mode.into(),
        detail: String::new(),
    });

    let mut all_commissions: Vec<ScrapedCommission> = Vec::new();

    for entry in &index {
        if cache_only() {
            require_cache_present(
                &entry.cache_path,
                &format!("commission detail '{}'", entry.name),
            )?;
        } else {
            let client = client.as_ref().expect("live mode requires ScrapingClient");
            refresh_html_cache(client, &entry.url, &entry.cache_path).await?;
        }

        let html = read_to_string(&entry.cache_path)?;
        let doc = Html::parse_document(&html);
        let rel_cache = relative_cache_path(&entry.cache_path, &cache_dir());

        let meta = read_cache_metadata(&entry.cache_path)?;
        let content_hash = meta
            .as_ref()
            .map(|m| m.content_hash.clone())
            .unwrap_or_else(|| content_hash_bytes(html.as_bytes()));
        let fetched_at = meta
            .as_ref()
            .map(|m| m.fetched_at.clone())
            .unwrap_or_else(now_rfc3339);
        let checked_at = meta
            .as_ref()
            .map(|m| m.checked_at.clone())
            .unwrap_or_else(now_rfc3339);

        all_commissions.push(ScrapedCommission {
            name: entry.name.clone(),
            ctype: entry.ctype.clone(),
            chairs: extract_members(&doc, CommissionRole::Chair),
            subchairs: extract_members(&doc, CommissionRole::Subchair),
            permanent_members: extract_members(&doc, CommissionRole::Permanent),
            replacement_members: extract_members(&doc, CommissionRole::Replacement),
            source_url: entry.url.clone(),
            cache_path: rel_cache.clone(),
        });

        manifest_rows.push(SourceManifestRow {
            source: SOURCE_NAME.into(),
            session_id: String::new(),
            item_kind: "commission".into(),
            native_item_id: entry.name.clone(),
            source_url: entry.url.clone(),
            cache_path: rel_cache,
            status: MANIFEST_STATUS_PARSED.into(),
            row_count: 1,
            content_type: "text/html".into(),
            content_hash,
            fetched_at,
            checked_at,
            run_mode: run_mode.into(),
            detail: String::new(),
        });
    }

    let parsed_names: BTreeSet<String> = all_commissions.iter().map(|c| c.name.clone()).collect();
    if parsed_names != index_names {
        let missing: Vec<_> = index_names.difference(&parsed_names).cloned().collect();
        let extra: Vec<_> = parsed_names.difference(&index_names).cloned().collect();
        return Err(format!(
            "commissions inventory mismatch: missing from parse {:?}, unexpected {:?} — aborting",
            missing, extra
        )
        .into());
    }

    if all_commissions.is_empty() {
        return Err(if index.is_empty() {
            "commissions index produced zero commission rows — aborting".into()
        } else {
            "commissions index is non-empty but produced zero commission rows — aborting".into()
        });
    }

    validate_manifest_rows(&manifest_rows)?;

    let manifest_final = manifest_path(SOURCE_NAME);
    let mut bundle = BundlePublisher::new("commissions", &data_dir())?;
    let stage_parquet = bundle.stage_path(&parquet_path)?;
    let stage_manifest = bundle.stage_path(&manifest_final)?;
    write_parquet(&stage_parquet, &all_commissions)?;
    write_source_manifest(&stage_manifest, &manifest_rows)?;
    bundle.commit()?;

    println!(
        "Scraped {} commissions (run_mode={}).",
        all_commissions.len(),
        run_mode
    );
    Ok(())
}

/// Live refresh: always fetch; unchanged bytes → touch_checked_at, else write_cache_artifact.
async fn refresh_html_cache(
    client: &ScrapingClient,
    url: &str,
    cache_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let html = client.get(url).await?.text().await?;
    let bytes = html.as_bytes();
    if cache_path.exists() {
        let existing = std::fs::read(cache_path)?;
        if existing.as_slice() == bytes {
            touch_checked_at(cache_path)?;
        } else {
            write_cache_artifact(cache_path, bytes, url, "text/html")?;
        }
    } else {
        write_cache_artifact(cache_path, bytes, url, "text/html")?;
    }
    Ok(())
}

fn write_parquet(path: &Path, rows: &[ScrapedCommission]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("name", DataType::Utf8, false),
        Field::new("type", DataType::Utf8, false),
        Field::new("chairs", DataType::Utf8, false),
        Field::new("subchairs", DataType::Utf8, false),
        Field::new("permanent_members", DataType::Utf8, false),
        Field::new("replacement_members", DataType::Utf8, false),
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
            col!(|r| r.ctype.clone()),
            col!(|r| r.chairs.clone()),
            col!(|r| r.subchairs.clone()),
            col!(|r| r.permanent_members.clone()),
            col!(|r| r.replacement_members.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
        ],
    )?;

    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

fn extract_index(document: &Html, detail_dir: &Path) -> Vec<CommissionIndex> {
    let mut entries = Vec::new();
    let mut current_type = String::from("unknown");

    for element in document.select(&SEL_INDEX) {
        match element.value().name() {
            "h4" => {
                let text = element.text().collect::<String>().trim().to_string();
                if !text.is_empty() {
                    current_type = text.to_lowercase();
                }
            }
            "a" => {
                let Some(href) = element.value().attr("href") else {
                    continue;
                };
                if !href.contains("/comm/com.cfm?com=") {
                    continue;
                }
                let name = element.text().collect::<String>().trim().to_lowercase();
                if name.is_empty() {
                    continue;
                }
                let url = if href.starts_with("http") {
                    href.to_string()
                } else {
                    format!("https://www.dekamer.be/kvvcr/{}", href)
                };
                let safe = name.replace(' ', "_").replace('/', "_");
                entries.push(CommissionIndex {
                    name,
                    ctype: current_type.clone(),
                    url,
                    cache_path: detail_dir.join(format!("{safe}.html")),
                });
            }
            _ => {}
        }
    }

    entries
}
