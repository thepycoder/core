mod parse;

use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use chrono::{Datelike, Utc};
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::utils::relative_cache_path;
use crawl::{
    BundlePublisher, MANIFEST_STATUS_NO_RESULT, MANIFEST_STATUS_PARSED, SourceManifestRow,
    content_hash_bytes, manifest_path, now_rfc3339, read_cache_metadata, require_cache_present,
    touch_checked_at, validate_manifest_rows, write_cache_artifact, write_source_manifest,
};
use headless_chrome::Browser;
use indicatif::{ProgressBar, ProgressStyle};
use parquet::arrow::ArrowWriter;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::RowAccessor;
use scraper::{Html, Selector};
use std::collections::{BTreeSet, HashSet};
use std::error::Error;
use std::fs::{File, read_to_string};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

static SEL_ROW: LazyLock<Selector> = LazyLock::new(|| Selector::parse("tbody tr").unwrap());
static SEL_MANDATE: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("tbody td[aria-colindex='3']").unwrap());
static SEL_INSTITUTE: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("tbody td[aria-colindex='4'] button").unwrap());
static SEL_REMUNERATION: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("tbody td[aria-colindex='6']").unwrap());
// Begin / Einde columns — date segments for the same mandate (e.g. Clarinval-David-2018.html).
static SEL_PERIOD_START: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("tbody td[aria-colindex='7']").unwrap());
static SEL_PERIOD_END: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("tbody td[aria-colindex='8']").unwrap());
static SEL_NO_RESULT: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("tbody tr.k-grid-norecords td").unwrap());

const SOURCE_NAME: &str = "remunerations";

/// Floor year for the member×year query matrix. Regimand coverage before 2018 is
/// out of scope for this scraper; the upper bound is [`latest_completed_year`].
const YEAR_FLOOR: i32 = 2018;

#[derive(Debug)]
struct ScrapedRemuneration {
    first_name: String,
    last_name: String,
    year: u32,
    mandate: String,
    institute: String,
    remuneration_min: String,
    remuneration_max: String,
    /// Raw Begin cell from the regimand grid (period segment identity).
    period_start: String,
    /// Raw Einde cell from the regimand grid (period segment identity).
    period_end: String,
    source_url: String,
    cache_path: String,
}

/// Latest calendar year whose remunerations are expected to be complete
/// (current UTC year minus one).
fn latest_completed_year() -> i32 {
    Utc::now().year() - 1
}

/// Live runs always cover `YEAR_FLOOR..=latest_completed_year()`.
/// Cache-only runs only require years already evidenced by existing cache files
/// (so extending the upper bound does not break `just reparse` until a live fetch).
fn remuneration_years() -> Vec<i32> {
    let latest = latest_completed_year();
    if !cache_only() {
        return (YEAR_FLOOR..=latest).collect();
    }
    let upper = max_year_in_remuneration_cache()
        .unwrap_or(YEAR_FLOOR)
        .min(latest);
    (YEAR_FLOOR..=upper.max(YEAR_FLOOR)).collect()
}

fn max_year_in_remuneration_cache() -> Option<i32> {
    let dir = cache_dir().join("remunerations");
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut max_year: Option<i32> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // `{Last}-{First}-{year}.html`
        if let Some(stem) = name.strip_suffix(".html") {
            if let Some(year_str) = stem.rsplit('-').next() {
                if let Ok(y) = year_str.parse::<i32>() {
                    max_year = Some(max_year.map_or(y, |m| m.max(y)));
                }
            }
        }
    }
    max_year
}

fn native_item_id(last_name: &str, first_name: &str, year: i32) -> String {
    format!("{last_name}-{first_name}-{year}")
}

fn source_url_for(first_name: &str, last_name: &str, year: i32) -> String {
    format!("https://public.regimand.be/?mandatary={first_name} {last_name}&year={year}")
}

fn cache_path_for(first_name: &str, last_name: &str, year: i32) -> PathBuf {
    cache_dir().join(format!(
        "remunerations/{}-{}-{}.html",
        last_name, first_name, year
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let browser = if cache_only() {
        None
    } else {
        Some(Browser::default()?)
    };
    let tab = match browser.as_ref() {
        Some(browser) => Some(browser.new_tab()?),
        None => None,
    };

    let members_path = data_dir().join("sessions/56/members.parquet");
    let remunerations_path = data_dir().join("remunerations.parquet");
    std::fs::create_dir_all(remunerations_path.parent().unwrap())?;

    let run_mode = if cache_only() {
        SourceManifestRow::run_mode_cache_only()
    } else {
        SourceManifestRow::run_mode_live()
    };

    let mut seen = HashSet::new();
    let mut members = Vec::new();
    let mut all_remunerations = Vec::new();
    let mut manifest_rows = Vec::new();
    let mut web_requests = 0u32;

    let members_file = File::open(&members_path)?;
    let reader = SerializedFileReader::new(members_file)?;
    let mut iter = reader.get_row_iter(None)?;

    while let Some(row_result) = iter.next() {
        let row = row_result?;
        let first_name = row.get_string(2)?.to_string();
        let last_name = row.get_string(3)?.to_string();
        let full_name = format!("{first_name} {last_name}");

        if seen.insert(full_name) {
            members.push((first_name, last_name));
        }
    }

    if members.is_empty() {
        return Err("members parquet produced zero unique members — aborting".into());
    }

    let years = remuneration_years();
    let year_count = years.len();
    if year_count == 0 {
        return Err(format!(
            "remuneration year range is empty (floor={YEAR_FLOOR}, latest_completed={}) — aborting",
            latest_completed_year()
        )
        .into());
    }

    let expected: BTreeSet<String> = members
        .iter()
        .flat_map(|(first_name, last_name)| {
            years
                .iter()
                .map(move |&year| native_item_id(last_name, first_name, year))
        })
        .collect();

    let total_steps = (members.len() * year_count) as u64;
    let pb = ProgressBar::new(total_steps);
    pb.set_style(
        ProgressStyle::with_template(
            "[remunerations] [{elapsed_precise}] {spinner:.blue} {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}",
        )?
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    pb.set_message(web_requests.to_string());

    for (first_name, last_name) in &members {
        for &year in &years {
            pb.set_message(format!(
                "reqs={} {} {} ({})",
                web_requests, first_name, last_name, year
            ));

            let cache_path = cache_path_for(first_name, last_name, year);
            let url = source_url_for(first_name, last_name, year);
            let item_id = native_item_id(last_name, first_name, year);

            fetch_remuneration_html(tab.as_deref(), &url, &cache_path, &mut web_requests)?;

            let (mut rows, status) =
                parse_remuneration_html(first_name, last_name, year, &cache_path, &url)?;

            let meta = read_cache_metadata(&cache_path)?;
            let content_hash = meta
                .as_ref()
                .map(|m| m.content_hash.clone())
                .unwrap_or_else(|| {
                    std::fs::read(&cache_path)
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
            let rel_cache = relative_cache_path(&cache_path, &cache_dir());

            let detail = if status == MANIFEST_STATUS_NO_RESULT {
                "Geen resultaat gevonden".to_string()
            } else {
                String::new()
            };

            manifest_rows.push(SourceManifestRow {
                source: SOURCE_NAME.into(),
                session_id: String::new(),
                item_kind: "member_year".into(),
                native_item_id: item_id,
                source_url: url,
                cache_path: rel_cache,
                status: status.into(),
                row_count: rows.len() as u32,
                content_type: "text/html".into(),
                content_hash,
                fetched_at,
                checked_at,
                run_mode: run_mode.into(),
                detail,
            });

            all_remunerations.append(&mut rows);
            pb.inc(1);
        }
    }

    pb.finish_with_message("done");

    let seen_ids: BTreeSet<String> = manifest_rows
        .iter()
        .map(|r| r.native_item_id.clone())
        .collect();
    if seen_ids != expected {
        let missing: Vec<_> = expected.difference(&seen_ids).cloned().collect();
        let extra: Vec<_> = seen_ids.difference(&expected).cloned().collect();
        return Err(format!(
            "remunerations member×year matrix incomplete: missing {:?}, unexpected {:?} — aborting",
            missing, extra
        )
        .into());
    }

    dedupe_remunerations(&mut all_remunerations);
    validate_manifest_rows(&manifest_rows)?;

    let manifest_final = manifest_path(SOURCE_NAME);
    let mut bundle = BundlePublisher::new("remunerations", &data_dir())?;
    let stage_parquet = bundle.stage_path(&remunerations_path)?;
    let stage_manifest = bundle.stage_path(&manifest_final)?;
    write_parquet(&stage_parquet, &all_remunerations)?;
    write_source_manifest(&stage_manifest, &manifest_rows)?;
    bundle.commit()?;

    println!(
        "Scraped {} remuneration rows using {} web request(s) ({} member×year cells, run_mode={}).",
        all_remunerations.len(),
        web_requests,
        expected.len(),
        run_mode
    );
    Ok(())
}

/// Live: always navigate (even when cache exists), then write_cache_artifact or
/// touch_checked_at. Cache-only: require_cache_present — never soft-skip missing files.
fn fetch_remuneration_html(
    tab: Option<&headless_chrome::Tab>,
    source_url: &str,
    cache_path: &Path,
    web_requests: &mut u32,
) -> Result<(), Box<dyn Error>> {
    if cache_only() {
        require_cache_present(
            cache_path,
            &format!("remuneration page {}", cache_path.display()),
        )?;
        return Ok(());
    }

    let tab = tab.ok_or("browser tab required for live remuneration scrape")?;
    tab.navigate_to(source_url)?;
    *web_requests += 1;
    tab.wait_for_element("kendo-autocomplete")?;
    let html = tab.get_content()?;
    let bytes = html.as_bytes();

    if cache_path.exists() {
        let existing = std::fs::read(cache_path)?;
        if existing.as_slice() == bytes {
            touch_checked_at(cache_path)?;
        } else {
            write_cache_artifact(cache_path, bytes, source_url, "text/html")?;
        }
    } else {
        write_cache_artifact(cache_path, bytes, source_url, "text/html")?;
    }
    Ok(())
}

/// Parse a cached remuneration HTML page into staging rows plus manifest status.
fn parse_remuneration_html(
    first_name: &str,
    last_name: &str,
    year: i32,
    cache_path: &Path,
    source_url: &str,
) -> Result<(Vec<ScrapedRemuneration>, &'static str), Box<dyn Error>> {
    let content = read_to_string(cache_path)?;
    let document = Html::parse_document(&content);
    let cache_path_rel = relative_cache_path(cache_path, &cache_dir());

    if document
        .select(&SEL_NO_RESULT)
        .any(|el| el.text().any(|t| t.contains("Geen resultaat gevonden")))
    {
        return Ok((Vec::new(), MANIFEST_STATUS_NO_RESULT));
    }

    let mut rows = Vec::new();
    for row in document.select(&SEL_ROW) {
        let mandate = row
            .select(&SEL_MANDATE)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join(" "))
            .unwrap_or_else(|| "Unknown".to_string());

        let institute = row
            .select(&SEL_INSTITUTE)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join(" "))
            .unwrap_or_else(|| "Unknown".to_string());

        let (remuneration_min, remuneration_max) = row
            .select(&SEL_REMUNERATION)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join(" "))
            .and_then(|raw| parse::parse_remuneration_text(&raw))
            .unwrap_or_else(|| (String::new(), String::new()));

        let period_start = row
            .select(&SEL_PERIOD_START)
            .next()
            .map(|el| clean_cell_text(&el.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();
        let period_end = row
            .select(&SEL_PERIOD_END)
            .next()
            .map(|el| clean_cell_text(&el.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();

        rows.push(ScrapedRemuneration {
            first_name: first_name.to_string(),
            last_name: last_name.to_string(),
            year: year as u32,
            mandate,
            institute,
            remuneration_min,
            remuneration_max,
            period_start,
            period_end,
            source_url: source_url.to_string(),
            cache_path: cache_path_rel.clone(),
        });
    }

    Ok((rows, MANIFEST_STATUS_PARSED))
}

fn clean_cell_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn write_parquet(path: &Path, rows: &[ScrapedRemuneration]) -> Result<(), Box<dyn Error>> {
    // Amounts stay Utf8 decimal EUR strings (not Arrow DECIMAL): staging is string-oriented
    // and parse.rs already normalizes European locale into canonical decimal text.
    let schema = Arc::new(Schema::new(vec![
        Field::new("first_name", DataType::Utf8, false),
        Field::new("last_name", DataType::Utf8, false),
        Field::new("year", DataType::Utf8, false),
        Field::new("mandate", DataType::Utf8, false),
        Field::new("institute", DataType::Utf8, false),
        Field::new("remuneration_min", DataType::Utf8, false),
        Field::new("remuneration_max", DataType::Utf8, false),
        Field::new("period_start", DataType::Utf8, false),
        Field::new("period_end", DataType::Utf8, false),
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
            col!(|r| r.first_name.clone()),
            col!(|r| r.last_name.clone()),
            col!(|r| r.year.to_string()),
            col!(|r| r.mandate.clone()),
            col!(|r| r.institute.clone()),
            col!(|r| r.remuneration_min.clone()),
            col!(|r| r.remuneration_max.clone()),
            col!(|r| r.period_start.clone()),
            col!(|r| r.period_end.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
        ],
    )?;

    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

/// Exact-row dedupe only. Same mandate with distinct Begin/Einde periods is kept —
/// those are source date segments (Clarinval-David-2018.html Burgemeester Bièvre).
fn dedupe_remunerations(rows: &mut Vec<ScrapedRemuneration>) {
    let mut seen = HashSet::new();
    rows.retain(|row| {
        seen.insert((
            row.first_name.clone(),
            row.last_name.clone(),
            row.year,
            row.mandate.clone(),
            row.institute.clone(),
            row.period_start.clone(),
            row.period_end.clone(),
            row.remuneration_min.clone(),
            row.remuneration_max.clone(),
        ))
    });
}
