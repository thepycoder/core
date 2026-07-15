use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use chrono::NaiveDate;
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::utils::{dutch_language_to_language_code, dutch_month_to_number, relative_cache_path};
use crawl::{
    BundlePublisher, MANIFEST_STATUS_PARSED, SourceManifestRow, content_hash_bytes, manifest_path,
    now_rfc3339, read_cache_metadata, require_cache_present, touch_checked_at,
    validate_manifest_rows, write_cache_artifact, write_source_manifest,
};
use indicatif::{ProgressBar, ProgressStyle};
use parquet::arrow::ArrowWriter;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use std::collections::{BTreeSet, HashSet};
use std::error::Error;
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, LazyLock, OnceLock};

const SOURCE_NAME: &str = "members";

/// REGEXES
static REGEX_FRACTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Fractie:\s*([^|(]+)").unwrap());

static REGEX_BIRTH_DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Geboren(?:\s+te\s+(?P<place>[^|]+?))?\s+op\s+(?P<date>\d{1,2}/\d{1,2}/\d{4})")
        .unwrap()
});

static REGEX_BIRTH_DATE_SLASH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Geboren(?:\s+te\s+(?P<place>[^|]+?))?\s+op\s+(?P<date>\d{1,2}/\d{1,2}/\d{4})")
        .unwrap()
});

static REGEX_START_DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{1,2})/(\d{1,2})/(\d{4})").unwrap());

static REGEX_CONSTITUENCY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:voor de kieskring|voor het arrondissement)\s+([A-Za-z0-9\s-]+)").unwrap()
});

static REGEX_MEMBER_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"cvview54\.cfm\?key=([O0]\d{4})").unwrap());

/// SELECTORS
static SELECTOR_A: OnceLock<Selector> = OnceLock::new();
static SELECTOR_P: OnceLock<Selector> = OnceLock::new();
static SELECTOR_P_I: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TR: OnceLock<Selector> = OnceLock::new();
static SELECTOR_NAME: OnceLock<Selector> = OnceLock::new();
static SELECTOR_FRACTION: OnceLock<Selector> = OnceLock::new();
static SELECTOR_DETAIL_PAGE_LINK: OnceLock<Selector> = OnceLock::new();
static SELECTOR_EMAIL: OnceLock<Selector> = OnceLock::new();

fn selector_a() -> &'static Selector {
    SELECTOR_A.get_or_init(|| Selector::parse("a").unwrap())
}

fn selector_p() -> &'static Selector {
    SELECTOR_P.get_or_init(|| Selector::parse("p").unwrap())
}

fn selector_p_i() -> &'static Selector {
    SELECTOR_P_I.get_or_init(|| Selector::parse("p i").unwrap())
}

fn selector_tr() -> &'static Selector {
    SELECTOR_TR.get_or_init(|| Selector::parse("tr").unwrap())
}

fn selector_name() -> &'static Selector {
    SELECTOR_NAME.get_or_init(|| Selector::parse("tr a[href*='cvview54.cfm'] > b").unwrap())
}

fn selector_fraction() -> &'static Selector {
    SELECTOR_FRACTION.get_or_init(|| Selector::parse("tr a[href*='cvlist54.cfm']").unwrap())
}

fn selector_detail_page_link() -> &'static Selector {
    SELECTOR_DETAIL_PAGE_LINK.get_or_init(|| Selector::parse("tr a[href*='cvview54.cfm']").unwrap())
}

fn selector_email() -> &'static Selector {
    SELECTOR_EMAIL.get_or_init(|| Selector::parse("tr a[href*='mailto:']").unwrap())
}

/// A scraped member.
#[derive(Debug)]
struct ScrapedMember {
    member_id: String,
    session_id: i32,
    first_name: String,
    last_name: String,
    date_of_birth: String,
    place_of_birth: String,
    language: String,
    constituency: String,
    fraction: String,
    email: String,
    active: bool,
    start: Option<String>,
    source_url: String,
    cache_path: String,
}

#[derive(Hash)]
struct MemberKey {
    session_id: i32,
    first_name: String,
    last_name: String,
}

/// One unique index row after dedup (inventory key for reconciliation).
struct IndexEntry {
    /// Stable inventory key: cvview key from index href when present, else reordered name.
    expected_key: String,
    session_id: i32,
    active: bool,
    name: String,
    first_name: String,
    last_name: String,
    detail_href: String,
    fraction: String,
    email: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let run_mode = if cache_only() {
        SourceManifestRow::run_mode_cache_only()
    } else {
        SourceManifestRow::run_mode_live()
    };

    // NOTE: Page 56 here is not the same as 'today', so for session 56 we also add today.
    // This is needed to determine the active/inactive state of the members.
    let sessions: Vec<((i32, bool), String)> = {
        let mut v = vec![(
            (56, true),
            "https://www.dekamer.be/kvvcr/showpage.cfm?section=/depute&language=nl&cfm=/site/wwwcfm/depute/cvlist54.cfm"
                .to_string(),
        )];
        v.extend([(56i32, false)].into_iter().map(|i| {
            (
                i,
                format!(
                    "https://www.dekamer.be/kvvcr/showpage.cfm?section=/depute&language=nl&cfm=cvlist54.cfm?legis={:02}&today=n",
                    i.0
                ),
            )
        }));
        v
    };

    let members_path = data_dir().join("sessions/56/members.parquet");
    std::fs::create_dir_all(members_path.parent().unwrap())?;

    let client = if cache_only() {
        None
    } else {
        Some(ScrapingClient::new())
    };

    let mut seen: HashSet<(i32, u64)> = HashSet::new();
    let mut expected_keys: BTreeSet<String> = BTreeSet::new();
    let mut output_keys: BTreeSet<String> = BTreeSet::new();
    let mut all_members: Vec<ScrapedMember> = Vec::new();
    let mut manifest_rows: Vec<SourceManifestRow> = Vec::new();
    let mut web_request_count = 0u32;

    for ((session_id, active), url) in &sessions {
        let index_label = if *active { "active" } else { "all" };
        let index_path = cache_dir().join(format!(
            "sessions/{}/members/{}.html",
            session_id, index_label
        ));

        fetch_or_verify_html(
            client.as_ref(),
            &index_path,
            url,
            &format!("members {index_label} index"),
            &mut web_request_count,
        )
        .await?;

        let content = std::fs::read_to_string(&index_path)?;
        let document = Html::parse_document(&content);
        let rel_index = relative_cache_path(&index_path, &cache_dir());

        let index_meta = read_cache_metadata(&index_path)?;
        let index_hash = index_meta
            .as_ref()
            .map(|m| m.content_hash.clone())
            .unwrap_or_else(|| content_hash_bytes(content.as_bytes()));
        let index_fetched = index_meta
            .as_ref()
            .map(|m| m.fetched_at.clone())
            .unwrap_or_else(now_rfc3339);
        let index_checked = index_meta
            .as_ref()
            .map(|m| m.checked_at.clone())
            .unwrap_or_else(now_rfc3339);

        let entries = collect_index_entries(&document, *session_id, *active, &mut seen);
        for entry in &entries {
            expected_keys.insert(entry.expected_key.clone());
        }

        manifest_rows.push(SourceManifestRow {
            source: SOURCE_NAME.into(),
            session_id: session_id.to_string(),
            item_kind: "index".into(),
            native_item_id: index_label.into(),
            source_url: url.clone(),
            cache_path: rel_index,
            status: MANIFEST_STATUS_PARSED.into(),
            row_count: entries.len() as u32,
            content_type: "text/html".into(),
            content_hash: index_hash,
            fetched_at: index_fetched,
            checked_at: index_checked,
            run_mode: run_mode.into(),
            detail: String::new(),
        });

        let mut members = scrape_member_details(
            client.as_ref(),
            &entries,
            &mut web_request_count,
            &mut manifest_rows,
            &mut output_keys,
            run_mode,
        )
        .await?;
        all_members.append(&mut members);
    }

    if output_keys != expected_keys {
        let missing: Vec<_> = expected_keys.difference(&output_keys).cloned().collect();
        let extra: Vec<_> = output_keys.difference(&expected_keys).cloned().collect();
        return Err(format!(
            "members inventory mismatch: expected {} keys, got {}; missing={:?} extra={:?}",
            expected_keys.len(),
            output_keys.len(),
            missing,
            extra
        )
        .into());
    }

    // append_hardcoded_members(&mut all_members, &mut seen);
    validate_manifest_rows(&manifest_rows)?;

    let manifest_final = manifest_path(SOURCE_NAME);
    let mut bundle = BundlePublisher::new("members", &data_dir())?;
    let stage_parquet = bundle.stage_path(&members_path)?;
    let stage_manifest = bundle.stage_path(&manifest_final)?;
    write_parquet(&stage_parquet, &all_members)?;
    write_source_manifest(&stage_manifest, &manifest_rows)?;
    bundle.commit()?;

    println!(
        "[members] scraped {} members using {} web requests",
        all_members.len(),
        web_request_count
    );
    Ok(())
}

async fn fetch_or_verify_html(
    client: Option<&ScrapingClient>,
    cache_path: &Path,
    url: &str,
    label: &str,
    web_request_count: &mut u32,
) -> Result<(), Box<dyn Error>> {
    if cache_only() {
        require_cache_present(cache_path, label)?;
        return Ok(());
    }
    let client = client.ok_or("ScrapingClient required in live mode")?;
    let html = client.get(url).await?.text().await?;
    *web_request_count += 1;
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

fn collect_index_entries(
    index_document: &Html,
    session_id: i32,
    active: bool,
    seen: &mut HashSet<(i32, u64)>,
) -> Vec<IndexEntry> {
    let mut entries = Vec::new();

    for row in index_document.select(&selector_tr()) {
        let raw_name = match extract_from_row(&row, &selector_name(), None) {
            Some(n) => n,
            None => continue,
        };

        let name = reorder_name(raw_name);
        let (first_name, last_name) = split_name(&name);

        let dedup_id = calculate_hash(&MemberKey {
            session_id,
            first_name: first_name.clone(),
            last_name: last_name.clone(),
        });
        if !seen.insert((session_id, dedup_id)) {
            continue;
        }

        let detail_href = extract_from_row(&row, &selector_detail_page_link(), Some("href"))
            .unwrap_or_else(|| "unknown".to_string());
        let fraction = extract_from_row(&row, &selector_fraction(), None)
            .unwrap_or_default()
            .to_lowercase();
        let email = extract_from_row(&row, &selector_email(), None)
            .map(|e| e.chars().rev().collect::<String>())
            .unwrap_or_default();

        let expected_key = native_key_from_href(&detail_href).unwrap_or_else(|| name.clone());

        entries.push(IndexEntry {
            expected_key,
            session_id,
            active,
            name,
            first_name,
            last_name,
            detail_href,
            fraction,
            email,
        });
    }

    entries
}

async fn scrape_member_details(
    client: Option<&ScrapingClient>,
    entries: &[IndexEntry],
    web_request_count: &mut u32,
    manifest_rows: &mut Vec<SourceManifestRow>,
    output_keys: &mut BTreeSet<String>,
    run_mode: &str,
) -> Result<Vec<ScrapedMember>, Box<dyn Error>> {
    let mut members = Vec::new();

    let pb = ProgressBar::new(entries.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "[members] [{elapsed_precise}] {spinner:.blue} {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}",
        )?
        .tick_chars("⠋⠙⠹⠼⠴⠦⠧⠇⠏"),
    );

    for entry in entries {
        pb.set_message(format!(
            "reqs={} session={} member={}",
            web_request_count, entry.session_id, entry.name
        ));

        let detail_path = cache_dir().join(format!(
            "sessions/{}/members/{}/details.html",
            entry.session_id, entry.name
        ));
        let detail_url = format!("https://www.dekamer.be/kvvcr/{}", entry.detail_href);

        fetch_or_verify_html(
            client,
            &detail_path,
            &detail_url,
            &format!("member detail {}", entry.name),
            web_request_count,
        )
        .await?;

        let detail_cache_path = relative_cache_path(&detail_path, &cache_dir());
        let content = std::fs::read_to_string(&detail_path)?;
        let detail = Html::parse_document(&content);

        let member_id = extract_member_key(&detail).unwrap_or_default();
        let native_item_id = if !member_id.is_empty() {
            member_id.clone()
        } else {
            entry.name.clone()
        };
        output_keys.insert(entry.expected_key.clone());

        // Extract the representative paragraph
        let paragraph: Option<String> = detail
            .select(&selector_p())
            .find(|el| {
                el.text().any(|t| {
                    t.contains("olksvertegenwoordiger")
                        && (t.contains("arrondissement") || t.contains("kieskring"))
                })
            })
            .map(|el| el.text().collect());

        let language = extract_sibling_text(&detail, "Taal")
            .and_then(|l| dutch_language_to_language_code(l.as_str()).map(str::to_string))
            .map(|c| c.to_ascii_lowercase())
            .unwrap_or_default();

        let fraction = if entry.fraction.is_empty() {
            extract_fraction(paragraph.as_deref())
        } else {
            entry.fraction.clone()
        };

        let meta = read_cache_metadata(&detail_path)?;
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
        let content_type = meta
            .as_ref()
            .map(|m| m.content_type.clone())
            .unwrap_or_else(|| "text/html".to_string());

        manifest_rows.push(SourceManifestRow {
            source: SOURCE_NAME.into(),
            session_id: entry.session_id.to_string(),
            item_kind: "member".into(),
            native_item_id,
            source_url: detail_url.clone(),
            cache_path: detail_cache_path.clone(),
            status: MANIFEST_STATUS_PARSED.into(),
            row_count: 1,
            content_type,
            content_hash,
            fetched_at,
            checked_at,
            run_mode: run_mode.into(),
            detail: String::new(),
        });

        members.push(ScrapedMember {
            member_id,
            session_id: entry.session_id,
            first_name: entry.first_name.clone(),
            last_name: entry.last_name.clone(),
            date_of_birth: extract_birth_date(&detail),
            place_of_birth: extract_birth_place(&detail),
            language,
            constituency: extract_constituency(paragraph.as_deref()),
            fraction,
            email: entry.email.clone(),
            active: entry.active,
            start: extract_start_date(&detail),
            source_url: detail_url,
            cache_path: detail_cache_path,
        });

        pb.inc(1);
    }

    pb.finish_with_message("done");
    Ok(members)
}

fn native_key_from_href(href: &str) -> Option<String> {
    REGEX_MEMBER_KEY
        .captures(href)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

fn write_parquet(path: &Path, members: &[ScrapedMember]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("member_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("first_name", DataType::Utf8, false),
        Field::new("last_name", DataType::Utf8, false),
        Field::new("date_of_birth", DataType::Utf8, false),
        Field::new("place_of_birth", DataType::Utf8, false),
        Field::new("language", DataType::Utf8, false),
        Field::new("constituency", DataType::Utf8, false),
        Field::new("fraction", DataType::Utf8, false),
        Field::new("email", DataType::Utf8, false),
        Field::new("active", DataType::Utf8, false),
        Field::new("start", DataType::Utf8, true),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));

    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(
                members.iter().map($f).collect::<Vec<_>>(),
            )) as ArrayRef
        };
    }

    macro_rules! col_opt {
        ($f:expr) => {
            Arc::new(StringArray::from(
                members.iter().map($f).collect::<Vec<Option<String>>>(),
            )) as ArrayRef
        };
    }

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            col!(|m| m.member_id.clone()),
            col!(|m| m.session_id.to_string()),
            col!(|m| m.first_name.clone()),
            col!(|m| m.last_name.clone()),
            col!(|m| m.date_of_birth.clone()),
            col!(|m| m.place_of_birth.clone()),
            col!(|m| m.language.clone()),
            col!(|m| m.constituency.clone()),
            col!(|m| m.fraction.clone()),
            col!(|m| m.email.clone()),
            col!(|m| m.active.to_string()),
            col_opt!(|m| m.start.clone()),
            col!(|m| m.source_url.clone()),
            col!(|m| m.cache_path.clone()),
        ],
    )?;

    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

fn extract_fraction(paragraph: Option<&str>) -> String {
    let text = paragraph.unwrap_or_default();
    if text.contains("Geen lid van een erkende fractie") {
        return "independent".to_string();
    }

    if let Some(cap) = REGEX_FRACTION.captures(text) {
        if let Some(m) = cap.get(1) {
            return m.as_str().trim().to_lowercase();
        }
    }

    "".to_string()
}

fn extract_birth_place(document: &Html) -> String {
    document
        .select(&selector_p())
        .find(|el| el.text().any(|t| t.contains("Geboren te")))
        .and_then(|el| {
            let text = el.text().collect::<String>();
            let after = text.split("Geboren te").nth(1)?.to_string();
            let place = if after.contains("op") {
                after.split("op").next()?.trim().to_string()
            } else {
                after
                    .split(|c| c == '.' || c == '|' || c == '\n')
                    .next()?
                    .trim()
                    .to_string()
            };
            Some(place)
        })
        .unwrap_or_default()
}

fn extract_birth_date(document: &Html) -> String {
    document
        .select(&selector_p())
        .find(|el| el.text().any(|t| t.contains("Geboren")))
        .and_then(|el| {
            let text = el.text().collect::<String>();
            let segment = text
                .split('|')
                .map(str::trim)
                .find(|s| s.to_lowercase().contains("geboren"))
                .unwrap_or("")
                .to_string();

            if let Some(caps) = REGEX_BIRTH_DATE_SLASH.captures(&segment) {
                let date_str = caps.name("date")?.as_str();
                let parts: Vec<&str> = date_str.split('/').collect();
                let day: u32 = parts[0].parse().ok()?;
                let month: u32 = parts[1].parse().ok()?;
                let year: i32 = parts[2].parse().ok()?;
                return NaiveDate::from_ymd_opt(year, month, day)
                    .map(|d| d.format("%Y-%m-%d").to_string());
            }
            // Old dutch-month format: "12 januari 1980"
            let raw = REGEX_BIRTH_DATE
                .captures(&segment)
                .and_then(|c| c.name("date"))
                .map(|m| m.as_str().trim().trim_end_matches('.').to_string())
                .unwrap_or_default();
            Some(parse_dutch_date(&raw))
        })
        .unwrap_or_default()
}

fn extract_start_date(document: &Html) -> Option<String> {
    // Try to find a paragraph containing "sedert" or "sinds"
    let text = document
        .select(&selector_p())
        .find(|el| {
            el.text()
                .any(|t| t.contains("sedert") || t.contains("sinds"))
        })
        .map(|el| el.text().collect::<String>());

    let text = match text {
        Some(t) => t,
        None => {
            // Fallback: search ALL text in the document for sedert/sinds
            let full = document.root_element().text().collect::<String>();
            if full.contains("sedert") || full.contains("sinds") {
                full
            } else {
                return Some("2024-06-09".to_string()); // session 56 default
            }
        }
    };

    let keyword_pos = text
        .find("sedert")
        .map(|pos| (pos, 6))
        .or_else(|| text.find("sinds").map(|pos| (pos, 5)))?;

    let (pos, keyword_len) = keyword_pos;
    let after = text[pos + keyword_len..].trim_start();

    // Try numeric format: dd/mm/yyyy
    if let Some(caps) = REGEX_START_DATE.captures(after) {
        let day: u32 = caps.get(1)?.as_str().parse().ok()?;
        let month: u32 = caps.get(2)?.as_str().parse().ok()?;
        let year: i32 = caps.get(3)?.as_str().parse().ok()?;
        if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
            return Some(date.format("%Y-%m-%d").to_string());
        }
    }

    // Try Dutch month format: "9 juni 2024"
    let dutch_date = parse_dutch_date(after.split('|').next().unwrap_or(after).trim());
    if !dutch_date.is_empty() {
        return Some(dutch_date);
    }

    Some("2024-06-09".to_string())
}

fn extract_constituency(rep_para: Option<&str>) -> String {
    let text = rep_para.unwrap_or_default();

    let result = REGEX_CONSTITUENCY
        .captures(text)
        .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
        .map(|c| {
            c.split_whitespace()
                .take_while(|&w| {
                    w != "sedert" && w != "sinds" && w != "van" && w != "tot" && !w.ends_with('.')
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    // Replace "Waals Brabant" with "Waals-Brabant"
    if result.eq_ignore_ascii_case("waals brabant") {
        return "Waals-Brabant".to_string();
    }

    result
}

/// The list page stores names as "Last First"; reorder to "First Last".
fn reorder_name(raw: String) -> String {
    let mut parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() > 1 {
        let first = parts.pop().unwrap();
        format!("{} {}", first, parts.join(" "))
    } else {
        raw
    }
}

fn split_name(name: &str) -> (String, String) {
    match name.splitn(2, ' ').collect::<Vec<_>>().as_slice() {
        [first, rest] => (first.to_string(), rest.to_string()),
        [first] => (first.to_string(), String::new()),
        _ => (String::new(), String::new()),
    }
}

fn parse_dutch_date(raw: &str) -> String {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() != 3 {
        return String::new();
    }
    match (
        parts[0].parse::<u32>(),
        dutch_month_to_number(parts[1]),
        parts[2].parse::<i32>(),
    ) {
        (Ok(day), Some(month), Ok(year)) => NaiveDate::from_ymd_opt(year, month, day)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

fn extract_from_row(row: &ElementRef, selector: &Selector, attr: Option<&str>) -> Option<String> {
    row.select(selector).next().map(|el| match attr {
        Some(a) => el.value().attr(a).unwrap_or_default().to_string(),
        None => el.text().collect::<String>().trim().to_string(),
    })
}

fn extract_sibling_text(document: &Html, label: &str) -> Option<String> {
    document
        .select(&selector_p_i())
        .find(|el| el.text().any(|t| t.contains(label)))
        .and_then(|el| {
            el.next_sibling()
                .and_then(|sib| sib.value().as_text().map(|t| t.trim().to_string()))
        })
}

fn extract_member_key(document: &Html) -> Option<String> {
    document.select(selector_a()).find_map(|el| {
        let href = el.value().attr("href")?;
        REGEX_MEMBER_KEY
            .captures(href)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
    })
}

// fn append_hardcoded_members(members: &mut Vec<ScrapedMember>, seen: &mut HashSet<(i32, u64)>) {
//     #[rustfmt::skip]
//     let hardcoded: &[(i32, &str, &str, &str, &str, &str, &str, &str)] = &[
//         (56, "Rob",      "Beenders", "1979-04-26", "Bree",         "nl", "Limburg",          "vooruit"),
//         (56, "Jan",      "Jambon",    "1960-04-26", "Genk",         "nl", "Antwerpen",         "n-va"),
//         (56, "Bernard",  "Quintin",   "1971-01-01", "Brussel",      "fr", "",                  "mr"),
//         (56, "Eléonore", "Simonet",   "1997-11-26", "Luik",         "fr", "",                  "mr"),
//         (56, "Nicole",   "de Moor",   "1984-01-18", "Sint-Niklaas", "nl", "Brussel-Hoofdstad", "cd&v"),
//     ];

//     for &(session_id, first, last, dob, pob, lang, constituency, party) in hardcoded {
//         let member_id = calculate_hash(&MemberKey {
//             session_id,
//             first_name: first.to_string(),
//             last_name: last.to_string(),
//         });
//         if !seen.insert((session_id, member_id)) {
//             continue;
//         }
//         members.push(ScrapedMember {
//             member_id: String::new(),
//             session_id,
//             first_name: first.to_string(),
//             last_name: last.to_string(),
//             date_of_birth: dob.to_string(),
//             place_of_birth: pob.to_string(),
//             language: lang.to_string(),
//             constituency: constituency.to_string(),
//             fraction: String::new(),
//             email: String::new(),
//             active: false,
//             start: Some("".to_string()),
//         });
//     }
// }
