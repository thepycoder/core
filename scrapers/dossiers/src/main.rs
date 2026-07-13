use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use chrono::{Local, NaiveDate};
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::utils::relative_cache_path;
use encoding_rs::WINDOWS_1252;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parquet::arrow::ArrowWriter;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs::{File, read_to_string};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::SystemTime;
use tokio::fs::read_dir;
use tokio::fs::{self, remove_file};

const DEKAMER_BASE: &str = "https://www.dekamer.be";
/// Re-check open dossiers at most once per week unless linked to a recent plenary meeting.
const DEFAULT_RECHECK_DAYS: i64 = 7;
/// Dossiers tied to a plenary meeting in the last week are re-checked daily.
const ACTIVE_RECHECK_DAYS: i64 = 1;

static SELECTOR_TR: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TD: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TABLE: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TBODY: OnceLock<Selector> = OnceLock::new();
static SELECTOR_A: OnceLock<Selector> = OnceLock::new();
static SELECTOR_FONT: OnceLock<Selector> = OnceLock::new();
static LIST_FROM_TO_REGEX: OnceLock<Regex> = OnceLock::new();
static FLWB_DOSSIER_ID_REGEX: OnceLock<Regex> = OnceLock::new();

fn selector_tr() -> &'static Selector {
    SELECTOR_TR.get_or_init(|| Selector::parse("tr").unwrap())
}
fn selector_td() -> &'static Selector {
    SELECTOR_TD.get_or_init(|| Selector::parse("td").unwrap())
}
fn selector_table() -> &'static Selector {
    SELECTOR_TABLE.get_or_init(|| Selector::parse("table").unwrap())
}
fn selector_tbody() -> &'static Selector {
    SELECTOR_TBODY.get_or_init(|| Selector::parse("tbody").unwrap())
}
fn selector_a() -> &'static Selector {
    SELECTOR_A.get_or_init(|| Selector::parse("a").unwrap())
}
fn selector_font() -> &'static Selector {
    SELECTOR_FONT.get_or_init(|| Selector::parse("font").unwrap())
}

fn list_from_to_regex() -> &'static Regex {
    LIST_FROM_TO_REGEX.get_or_init(|| {
        Regex::new(r"(?i)ListFromTo\.cfm\?legislat=(\d+)&from=(\d+)&to=(\d+)").unwrap()
    })
}

fn flwb_dossier_id_regex() -> &'static Regex {
    FLWB_DOSSIER_ID_REGEX.get_or_init(|| {
        Regex::new(r#"(?i)flwbn\.cfm[^"'<>]*dossierID=(\d+)"#).unwrap()
    })
}

/// The output of this scraper.
struct DossierScrapeOutput {
    dossiers: Vec<ScrapedDossier>,
    subdocuments: Vec<ScrapedSubdocument>,
}

/// A scraped dossier.
struct ScrapedDossier {
    session_id: u32,
    dossier_id: String,
    last_updated: String,
    title: String,
    authors: String,
    submission_date: String,
    end_date: String,
    vote_date: String,
    document_type: String,
    status: String,
    latest_adopted_text_url: Option<String>,
    latest_report_url: Option<String>,
    eurovoc_main_descriptor: String,
    eurovoc_descriptors: String,
    source_url: String,
    cache_path: String,
}

/// A scraped subdocument.
struct ScrapedSubdocument {
    dossier_id: String,
    id: String,
    date: String,
    document_type: String,
    authors: String,
    file_url: Option<String>,
    source_url: String,
    cache_path: String,
}

/// A dossier.
struct Dossier {
    title: String,
    authors: Vec<String>,
    submission_date: String,
    end_date: String,
    vote_date: String,
    document_type: DocumentType,
    status: DocumentStatus,
    subdocuments: Vec<Subdocument>,
    eurovoc_main_descriptor: String,
    eurovoc_descriptors: String,
}

/// A subdocument.
struct Subdocument {
    dossier_id: String,
    id: String,
    document_type: DocumentType,
    date: String,
    authors: Vec<String>,
    file_url: Option<String>,
}

/// The status of a document which is linked to a dossier.
#[derive(Debug, Clone, Copy)]
enum DocumentStatus {
    Aangenomen,
    HangendKamer,
    Verworpen,
    ZonderVoorwerp,
    Onbekend,
}

impl fmt::Display for DocumentStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// The type of a document which is linked to a dossier.
#[derive(Debug, Clone, Copy)]
enum DocumentType {
    AangenomenMotie,
    AangenomenTekst,
    AanvullendVerslag,
    Advies,
    AdviesVanDeRaadVanState,
    Amendement,
    ArtikelenAangenomenInPlenum,
    ArtikelenBijEersteStemmingAangenomen,
    ArtikelenInTweedeLezingAangenomen,
    Begroting,
    Bijlage,
    Errata,
    Kaft,
    NietGeevoceerdOntwerp,
    OpmerkingenVanHetRekenhof,
    OvergezondenOntwerp,
    TabellenOfLijsten,
    Verantwoording,
    Verslag,
    VerslagVerwijzend,
    VoorstelOnderzoekscommissie,
    VoorstelReglement,
    VoorstelTotHerziening,
    VoorstelVanResolutie,
    VoorstelVanMotie,
    WetsOntwerp,
    WetsVoorstel,
    Onbekend,
}

impl fmt::Display for DocumentType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

macro_rules! col {
    ($rows:expr, $f:expr) => {
        Arc::new(StringArray::from($rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
    };
}

macro_rules! col_opt {
    ($rows:expr, $f:expr) => {
        Arc::new(StringArray::from(
            $rows.iter().map($f).collect::<Vec<Option<String>>>(),
        )) as ArrayRef
    };
}

/// A helper function to write a Parquet file.
fn write_parquet(
    path: &Path,
    schema: Arc<Schema>,
    columns: Vec<ArrayRef>,
) -> Result<(), Box<dyn Error>> {
    let batch = RecordBatch::try_new(schema.clone(), columns)?;
    let mut writer = ArrowWriter::try_new(File::create(path)?, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let client = ScrapingClient::new();
    let session_id: u32 = 56;

    let session_dir = data_dir().join("sessions").join(session_id.to_string());
    fs::create_dir_all(&session_dir).await?;

    let mp = MultiProgress::new();
    let mut web_request_count = 0u32;

    if cache_only() {
        println!("[dossiers] cache-only: skipping downloads, parsing cached HTML only");
    } else {
        download_dossiers(session_id, &client, &mut web_request_count, &mp).await?;
    }

    let DossierScrapeOutput {
        dossiers,
        subdocuments,
    } = scrape_all_dossiers(session_id, &mp).await?;

    write_dossiers(&session_dir.join("dossiers.parquet"), &dossiers)?;
    write_subdocuments(&session_dir.join("subdocuments.parquet"), &subdocuments)?;

    println!(
        "[dossiers] scraped {} dossiers using {} web requests",
        dossiers.len(),
        web_request_count
    );
    Ok(())
}

async fn download_dossiers(
    session_id: u32,
    client: &ScrapingClient,
    web_request_count: &mut u32,
    mp: &MultiProgress,
) -> Result<(), Box<dyn Error>> {
    let mut id_dates = load_plenary_dossier_ids(session_id);
    let flwb_ids = discover_all_dossier_ids(session_id, client, web_request_count).await?;
    let plenary_only = id_dates.len();
    for id in flwb_ids {
        id_dates.entry(id).or_insert_with(String::new);
    }
    let flwb_only = id_dates.len().saturating_sub(plenary_only);
    println!(
        "[dossiers] {} dossier ids to download ({} plenary, {} FLWB-only)",
        id_dates.len(),
        plenary_only,
        flwb_only
    );

    let pb = mp.add(ProgressBar::new(id_dates.len() as u64));
    pb.set_style(
        ProgressStyle::with_template(
            "[dossiers-download] [{elapsed_precise}] {spinner:.blue} {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}",
        )?
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );

    let mut stats = DownloadStats::default();
    for (id, latest_meeting_date) in &id_dates {
        pb.set_message(format!(
            "reqs={} id={} skip={}",
            web_request_count,
            id,
            stats.skipped_total()
        ));
        let action = check_and_download_dossier_file(
            id,
            latest_meeting_date,
            session_id,
            client,
            web_request_count,
        )
        .await?;
        stats.record(action);
        pb.inc(1);
    }

    pb.finish_with_message("done");
    println!(
        "[dossiers] download cache: {} settled, {} fresh, {} unchanged, {} updated ({} web requests)",
        stats.skipped_settled,
        stats.skipped_fresh,
        stats.skipped_unchanged,
        stats.downloaded,
        web_request_count
    );
    Ok(())
}

fn load_plenary_dossier_ids(session_id: u32) -> HashMap<String, String> {
    let ids_path = cache_dir().join(format!("sessions/{}/dossier_ids.txt", session_id));
    let content = match std::fs::read_to_string(&ids_path) {
        Ok(content) => content,
        Err(_) => {
            eprintln!(
                "[dossiers] no dossier_ids.txt from plenary scrape; using FLWB discovery only"
            );
            return HashMap::new();
        }
    };

    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let mut parts = l.splitn(2, '\t');
            let id = parts.next()?.trim().to_string();
            let date = parts.next().unwrap_or("").trim().to_string();
            Some((id, date))
        })
        .collect()
}

async fn discover_all_dossier_ids(
    session_id: u32,
    client: &ScrapingClient,
    web_request_count: &mut u32,
) -> Result<Vec<String>, Box<dyn Error>> {
    let list_url = format!(
        "{}/kvvcr/showpage.cfm?section=/flwb&language=nl&cfm=/site/wwwcfm/flwb/ListDocument.cfm?legislat={}",
        DEKAMER_BASE, session_id
    );
    let list_html = fetch_html(client, &list_url, web_request_count).await?;
    let range_urls = extract_list_from_to_urls(&list_html, session_id);

    if range_urls.is_empty() {
        return Err(format!(
            "FLWB discovery found no ListFromTo ranges for session {session_id}"
        )
        .into());
    }

    let range_count = range_urls.len();
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for range_url in range_urls {
        let range_html = fetch_html(client, &range_url, web_request_count).await?;
        for id in extract_flwb_dossier_ids(&range_html) {
            if seen.insert(id.clone()) {
                ids.push(id);
            }
        }
    }

    ids.sort_by(|a, b| {
        a.parse::<u32>()
            .unwrap_or(0)
            .cmp(&b.parse::<u32>().unwrap_or(0))
    });

    println!(
        "[dossiers] FLWB browse discovered {} dossier ids across {} ranges",
        ids.len(),
        range_count
    );
    Ok(ids)
}

async fn fetch_html(
    client: &ScrapingClient,
    url: &str,
    web_request_count: &mut u32,
) -> Result<String, Box<dyn Error>> {
    let response = client.get(url).await?;
    *web_request_count += 1;
    let raw_bytes = response.bytes().await?;
    let (decoded_str, _, _) = WINDOWS_1252.decode(&raw_bytes);
    Ok(decoded_str.into_owned())
}

fn decode_html_entities(raw: &str) -> String {
    raw.replace("&amp;", "&")
}

fn normalize_dossier_id(raw: &str) -> String {
    let trimmed = raw.trim().trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn extract_list_from_to_urls(html: &str, session_id: u32) -> Vec<String> {
    let decoded = decode_html_entities(html);
    let mut urls = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for caps in list_from_to_regex().captures_iter(&decoded) {
        let legislat: u32 = caps[1].parse().unwrap_or(0);
        if legislat != session_id {
            continue;
        }
        let from = caps[2].to_string();
        let to = caps[3].to_string();
        let url = format!(
            "{}/kvvcr/showpage.cfm?section=/flwb&language=nl&cfm=ListFromTo.cfm?legislat={}&from={}&to={}",
            DEKAMER_BASE, session_id, from, to
        );
        if seen.insert(url.clone()) {
            urls.push(url);
        }
    }

    urls
}

fn extract_flwb_dossier_ids(html: &str) -> Vec<String> {
    let decoded = decode_html_entities(html);
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for caps in flwb_dossier_id_regex().captures_iter(&decoded) {
        let id = normalize_dossier_id(&caps[1]);
        if seen.insert(id.clone()) {
            ids.push(id);
        }
    }

    ids
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadAction {
    SkippedSettled,
    SkippedFresh,
    SkippedUnchanged,
    Downloaded,
}

#[derive(Debug, Default)]
struct DownloadStats {
    skipped_settled: u32,
    skipped_fresh: u32,
    skipped_unchanged: u32,
    downloaded: u32,
}

impl DownloadStats {
    fn record(&mut self, action: DownloadAction) {
        match action {
            DownloadAction::SkippedSettled => self.skipped_settled += 1,
            DownloadAction::SkippedFresh => self.skipped_fresh += 1,
            DownloadAction::SkippedUnchanged => self.skipped_unchanged += 1,
            DownloadAction::Downloaded => self.downloaded += 1,
        }
    }

    fn skipped_total(&self) -> u32 {
        self.skipped_settled + self.skipped_fresh + self.skipped_unchanged
    }
}

#[derive(Serialize)]
struct SubdocumentFingerprint {
    id: String,
    date: String,
    document_type: String,
}

#[derive(Serialize)]
struct DossierContentFingerprint {
    status: String,
    submission_date: String,
    end_date: String,
    vote_date: String,
    subdocuments: Vec<SubdocumentFingerprint>,
}

/// Fingerprint of the dossier metadata table (status, dates, subdocuments).
/// Changes when dekamer.be adds or updates parliamentary documents on the dossier page.
fn dossier_content_fingerprint(html: &str, dossier_id: &str) -> Result<String, Box<dyn Error>> {
    let document = Html::parse_document(html);
    let dossier = scrape_dossier(dossier_id, &document)?;
    let mut subdocuments = dossier
        .subdocuments
        .iter()
        .map(|subdocument| SubdocumentFingerprint {
            id: subdocument.id.clone(),
            date: subdocument.date.clone(),
            document_type: subdocument.document_type.to_string(),
        })
        .collect::<Vec<_>>();
    subdocuments.sort_by(|left, right| left.id.cmp(&right.id));

    let fingerprint = DossierContentFingerprint {
        status: dossier.status.to_string(),
        submission_date: dossier.submission_date,
        end_date: dossier.end_date,
        vote_date: dossier.vote_date,
        subdocuments,
    };
    Ok(serde_json::to_string(&fingerprint)?)
}

fn dossier_reference_date(dossier: &Dossier) -> Option<NaiveDate> {
    for value in [&dossier.end_date, &dossier.vote_date] {
        if value.is_empty() {
            continue;
        }
        if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
            return Some(date);
        }
    }
    None
}

/// Terminal dossiers with an end/vote date more than a week ago are unlikely to change.
fn is_settled_dossier(dossier: &Dossier) -> bool {
    let terminal = matches!(
        dossier.status,
        DocumentStatus::Aangenomen | DocumentStatus::Verworpen | DocumentStatus::ZonderVoorwerp
    );
    if !terminal {
        return false;
    }
    let Some(reference_date) = dossier_reference_date(dossier) else {
        return false;
    };
    let today = Local::now().naive_local().date();
    (today - reference_date).num_days() >= DEFAULT_RECHECK_DAYS
}

fn recheck_max_age_days(latest_meeting_date: &str) -> i64 {
    if latest_meeting_date.is_empty() {
        return DEFAULT_RECHECK_DAYS;
    }
    let Ok(meeting_date) = NaiveDate::parse_from_str(latest_meeting_date, "%Y-%m-%d") else {
        return DEFAULT_RECHECK_DAYS;
    };
    let today = Local::now().naive_local().date();
    if (today - meeting_date).num_days().abs() < DEFAULT_RECHECK_DAYS {
        ACTIVE_RECHECK_DAYS
    } else {
        DEFAULT_RECHECK_DAYS
    }
}

fn cache_age_days(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let modified = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    let modified = chrono::DateTime::<chrono::Utc>::from_timestamp(modified.as_secs() as i64, 0)?;
    let today = Local::now().date_naive();
    Some((today - modified.date_naive()).num_days())
}

fn touch_cache_file(path: &Path) -> Result<(), Box<dyn Error>> {
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.set_modified(SystemTime::now())?;
    Ok(())
}

async fn find_cached_dossier_path(
    dossier_dir: &Path,
    session_id: u32,
    dossier_id: &str,
) -> Option<PathBuf> {
    let filename_prefix = format!("{}_{}_", session_id, dossier_id);
    let mut entries = read_dir(dossier_dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.starts_with(&filename_prefix) && file_name.ends_with(".html") {
            return Some(entry.path());
        }
    }
    None
}

fn dossier_url(session_id: u32, dossier_id: &str) -> String {
    format!(
        "https://www.dekamer.be/kvvcr/showpage.cfm?section=/flwb&language=nl&cfm=/site/wwwcfm/flwb/flwbn.cfm?lang=N&legislat={}&dossierID={}",
        session_id, dossier_id
    )
}

/// Checks whether a cached dossier HTML is still current.
///
/// Uses the metadata table fingerprint (status, dates, subdocuments). Settled dossiers
/// skip network entirely; open dossiers are re-checked weekly (daily when linked to a
/// recent plenary meeting).
async fn check_and_download_dossier_file(
    dossier_id: &str,
    latest_meeting_date: &str,
    session_id: u32,
    client: &ScrapingClient,
    web_request_count: &mut u32,
) -> Result<DownloadAction, Box<dyn Error>> {
    let dossier_dir = cache_dir().join(format!("sessions/{}/dossiers", session_id));
    fs::create_dir_all(&dossier_dir).await?;

    let url = dossier_url(session_id, dossier_id);
    let existing_file = find_cached_dossier_path(&dossier_dir, session_id, dossier_id).await;

    if let Some(cache_path) = &existing_file {
        let cached_html = read_to_string(cache_path)?;
        let cached_dossier = scrape_dossier(dossier_id, &Html::parse_document(&cached_html))?;

        if is_settled_dossier(&cached_dossier) {
            return Ok(DownloadAction::SkippedSettled);
        }

        if cache_age_days(cache_path).is_some_and(|age| age < recheck_max_age_days(latest_meeting_date))
        {
            return Ok(DownloadAction::SkippedFresh);
        }

        let cached_fingerprint = dossier_content_fingerprint(&cached_html, dossier_id)?;
        let live_html = fetch_html(client, &url, web_request_count).await?;
        let live_fingerprint = dossier_content_fingerprint(&live_html, dossier_id)?;
        if cached_fingerprint == live_fingerprint {
            touch_cache_file(cache_path)?;
            return Ok(DownloadAction::SkippedUnchanged);
        }

        let _ = remove_file(cache_path).await;
        let today = Local::now().naive_local().date();
        let new_path = dossier_dir.join(format!("{}_{}_{}.html", session_id, dossier_id, today));
        fs::write(&new_path, live_html).await?;
        return Ok(DownloadAction::Downloaded);
    }

    let live_html = fetch_html(client, &url, web_request_count).await?;
    let today = Local::now().naive_local().date();
    let new_path = dossier_dir.join(format!("{}_{}_{}.html", session_id, dossier_id, today));
    fs::write(&new_path, live_html).await?;
    Ok(DownloadAction::Downloaded)
}

/// Scrape all cached HTML dossier files.
async fn scrape_all_dossiers(
    session_id: u32,
    mp: &MultiProgress,
) -> Result<DossierScrapeOutput, Box<dyn Error>> {
    let dossier_dir = cache_dir().join(format!("sessions/{}/dossiers", session_id));

    // Collect HTML paths up front so we know the total for the progress bar.
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut entries = read_dir(&dossier_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("html") {
            paths.push(path);
        }
    }

    let pb = mp.add(ProgressBar::new(paths.len() as u64));
    pb.set_style(
        ProgressStyle::with_template(
            "[dossiers-scrape] [{elapsed_precise}] {spinner:.blue} {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}",
        )?
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );

    let mut dossiers = Vec::new();
    let mut subdocuments = Vec::new();

    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let mut parts = stem.splitn(3, '_');
        let _sid = parts.next().unwrap_or("");
        let dossier_id = parts.next().unwrap_or("").to_string();

        let last_updated = parts.next().unwrap_or("").to_string();

        pb.set_message(format!("id={}", dossier_id));

        let content = match read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to read {}: {}", path.display(), e);
                pb.inc(1);
                continue;
            }
        };

        let document = Html::parse_document(&content);
        let dossier = match scrape_dossier(&dossier_id, &document) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Failed to scrape {}: {}", dossier_id, e);
                pb.inc(1);
                continue;
            }
        };

        // Get latest adopted text URL.
        // Preference order:
        // 1. Most recent "ARTIKELEN AANGENOMEN IN PLENUM"
        // 2. Most recent "AANGENOMEN TEKST"
        let latest_adopted_text_url = dossier
            .subdocuments
            .iter()
            .filter(|s| s.file_url.is_some())
            .filter(|s| {
                matches!(
                    s.document_type,
                    DocumentType::ArtikelenAangenomenInPlenum | DocumentType::AangenomenTekst
                )
            })
            .max_by_key(|s| {
                (
                    matches!(s.document_type, DocumentType::ArtikelenAangenomenInPlenum),
                    &s.id,
                )
            })
            .and_then(|s| s.file_url.clone());

        // Get latest report URL.
        // Preference order:
        // 1. Most recent "VERSLAG"
        let latest_report_url = dossier
            .subdocuments
            .iter()
            .rev()
            .find(|s| matches!(s.document_type, DocumentType::Verslag) && s.file_url.is_some())
            .and_then(|s| s.file_url.clone());

        let cache_path_rel = relative_cache_path(&path, &cache_dir());
        let source_url = format!(
            "https://www.dekamer.be/kvvcr/showpage.cfm?section=/flwb&language=nl&cfm=/site/wwwcfm/flwb/flwbn.cfm?lang=N&legislat={}&dossierID={}",
            session_id, dossier_id
        );

        for subdocument in dossier.subdocuments {
            subdocuments.push(ScrapedSubdocument {
                dossier_id: subdocument.dossier_id,
                id: subdocument.id,
                date: subdocument.date,
                document_type: subdocument.document_type.to_string(),
                authors: subdocument.authors.join(","),
                file_url: subdocument.file_url,
                source_url: source_url.clone(),
                cache_path: cache_path_rel.clone(),
            });
        }

        dossiers.push(ScrapedDossier {
            session_id,
            dossier_id,
            last_updated,
            title: dossier.title,
            authors: dossier.authors.join(","),
            submission_date: dossier.submission_date,
            end_date: dossier.end_date,
            vote_date: dossier.vote_date,
            document_type: dossier.document_type.to_string(),
            status: dossier.status.to_string(),
            latest_adopted_text_url,
            latest_report_url,
            eurovoc_main_descriptor: dossier.eurovoc_main_descriptor,
            eurovoc_descriptors: dossier.eurovoc_descriptors,
            source_url,
            cache_path: cache_path_rel,
        });
        pb.inc(1);
    }

    pb.finish_with_message("done");
    Ok(DossierScrapeOutput {
        dossiers,
        subdocuments,
    })
}

fn scrape_dossier(dossier_id: &str, document: &Html) -> Result<Dossier, Box<dyn Error>> {
    let title_selector = Selector::parse("#story h4 center").unwrap();
    let title = document
        .select(&title_selector)
        .next()
        .and_then(|el| el.text().next())
        .unwrap_or("")
        .trim()
        .to_string();

    let mut submission_date = String::new();
    let mut vote_date = String::new();
    let mut end_date = String::new();
    let mut dossier_authors = Vec::new();
    let mut document_type = DocumentType::Onbekend;
    let mut status = DocumentStatus::Onbekend;
    let mut subdocuments = Vec::new();
    let mut eurovoc_main_descriptor = String::new();
    let mut eurovoc_descriptors = String::new();

    let document_table = document
        .select(selector_table())
        .next()
        .ok_or_else(|| format!("no table found in dossier {}", dossier_id))?;

    if let Some(tbody) = document_table.select(selector_tbody()).next() {
        for row in document_table.select(selector_tr()) {
            if row.parent().unwrap() != *tbody {
                continue;
            }

            // Select the columns in this row
            let mut columns = row.select(selector_td());
            let (Some(col_1), Some(col_2)) = (columns.next(), columns.next()) else {
                continue;
            };
            let label = col_1
                .text()
                .collect::<String>()
                .to_lowercase()
                .trim()
                .to_string();
            let value = col_2.text().collect::<String>().trim().to_string();
            let value_lower = value.to_lowercase();

            if label.contains("indieningsdatum") {
                submission_date = normalize_date(&value_lower);
            } else if label.contains("stemming kamer") {
                vote_date = normalize_date(&value_lower);
            } else if label.contains("einddatum") {
                end_date = normalize_date(&value_lower);
            } else if label.contains("auteur(s)") {
                for link in col_2.select(selector_a()) {
                    if let Some(name) = link.text().next() {
                        dossier_authors.push(normalize_author(name));
                    }
                }
                if dossier_authors.is_empty() {
                    for text_node in col_2.text() {
                        let name = text_node.trim();
                        if !name.is_empty() {
                            dossier_authors.push(normalize_author(name));
                        }
                    }
                }
            } else if label.contains("document type") {
                document_type = parse_document_type(&value_lower);
            } else if label.contains("status") {
                status = parse_document_status(&value_lower);
            } else if label.contains("subdocumenten") {
                subdocuments = parse_subdocuments(dossier_id, &col_2);
            } else if label.trim().is_empty() {
                // The 'HANGEND KAMER' row has a blank first cell.
                let candidate = parse_document_status(&value_lower);
                if !matches!(candidate, DocumentStatus::Onbekend) {
                    status = candidate;
                }
            } else if label.contains("eurovoc-hoofddescriptor") {
                eurovoc_main_descriptor = value.trim().to_uppercase();
            } else if label.contains("eurovoc descriptoren")
                || label.contains("eurovoc kandidaat-descriptoren")
            {
                // Source uses " | " as separator; store as comma-separated for JS `.split(",")`
                eurovoc_descriptors = value
                    .split('|')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(",");
            }
        }
    }

    Ok(Dossier {
        title,
        authors: dossier_authors,
        submission_date,
        end_date,
        vote_date,
        document_type,
        status,
        subdocuments,
        eurovoc_main_descriptor,
        eurovoc_descriptors,
    })
}

fn parse_subdocuments(dossier_id: &str, cell: &ElementRef) -> Vec<Subdocument> {
    let subdocument_table = match cell.select(selector_table()).next() {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut subdocuments = Vec::new();

    // Per-subdocument accumulators
    let mut document_id = String::new();
    let mut document_type = DocumentType::Onbekend;
    let mut document_date = String::new();
    let mut document_authors: Vec<String> = Vec::new();
    let mut file_url: Option<String> = None;
    let mut parsing_authors = false;
    let mut complete_subdocument = false;

    for row in subdocument_table.select(selector_tr()) {
        let mut cells = row.select(selector_td());
        let cell_1 = cells.next();
        let cell_2 = cells.next();

        // A row with only one cell (or none) acts as a separator between individual subdocuments.
        if cell_2.is_none() {
            if complete_subdocument {
                subdocuments.push(Subdocument {
                    dossier_id: dossier_id.to_string(),
                    id: document_id.clone(),
                    document_type,
                    date: document_date.clone(),
                    authors: document_authors.clone(),
                    file_url: file_url.clone(),
                });
                document_id.clear();
                document_date.clear();
                document_authors.clear();
                file_url = None;
                complete_subdocument = false;
                parsing_authors = false;
            }
            continue;
        }

        if let (Some(cell_1), Some(cell_2)) = (cell_1, cell_2) {
            let label = cell_1
                .text()
                .collect::<String>()
                .to_lowercase()
                .trim()
                .to_string();
            let value = cell_2
                .text()
                .collect::<String>()
                .to_lowercase()
                .trim()
                .to_string();

            // The link cell (cell_1) contains an <a> whose text is the
            // sub-document number, e.g. "003".
            // stripped down to 3
            if let Some(link) = cell_1.select(selector_a()).last() {
                if let Some(id_text) = link.text().next() {
                    document_id = id_text.trim().trim_start_matches('0').to_string();
                }
            }

            // cell_2 may carry an inline <font> tag with the document type.
            if let Some(font) = cell_2.select(selector_font()).next() {
                let raw_type = font.text().collect::<String>();
                document_type = parse_document_type(raw_type.trim());

                // Capture the linked document URL.
                let pdf_url = cell_1
                    .select(selector_a())
                    .filter_map(|a| a.value().attr("href"))
                    .filter(|href| href.ends_with(".pdf"))
                    .last()
                    .map(|href| {
                        if href.starts_with("http") {
                            href.to_string()
                        } else {
                            format!("{}{}", DEKAMER_BASE, href)
                        }
                    });

                if pdf_url.is_some() {
                    file_url = pdf_url;
                }
            }

            if label.contains("datum ronddeling") {
                document_date = normalize_date(&value);
            }

            if label.contains("auteur(s)") {
                parsing_authors = true;
            }

            if parsing_authors {
                if let Some(link) = cell_2.select(selector_a()).next() {
                    if let Some(name) = link.text().next() {
                        document_authors.push(normalize_author(name));
                    }
                }
            }

            if !document_id.is_empty() && !document_date.is_empty() {
                complete_subdocument = true;
            }
        }
    }

    if complete_subdocument {
        subdocuments.push(Subdocument {
            dossier_id: dossier_id.to_string(),
            id: document_id,
            document_type,
            date: document_date,
            authors: document_authors,
            file_url,
        });
    }

    subdocuments
}

/// Parse the document status from the given raw text.
fn parse_document_status(raw: &str) -> DocumentStatus {
    let raw = raw.trim().to_lowercase();
    if raw.contains("aangenomen") {
        DocumentStatus::Aangenomen
    } else if raw.contains("verworpen") {
        DocumentStatus::Verworpen
    } else if raw.contains("zonder voorwerp") {
        DocumentStatus::ZonderVoorwerp
    } else if raw.contains("hangend") {
        DocumentStatus::HangendKamer
    } else {
        DocumentStatus::Onbekend
    }
}

/// Parse the document type from the given raw text.
fn parse_document_type(raw: &str) -> DocumentType {
    let raw = raw.trim().to_lowercase();
    if raw.contains("voorstel van resolutie") {
        DocumentType::VoorstelVanResolutie
    } else if raw.contains("aanvullend verslag") {
        DocumentType::AanvullendVerslag
    } else if raw.contains("amendement") {
        DocumentType::Amendement
    } else if raw.contains("voorstel tot herziening") {
        DocumentType::VoorstelTotHerziening
    } else if raw.contains("wetsvoorstel") {
        DocumentType::WetsVoorstel
    } else if raw.contains("wetsontwerp") {
        DocumentType::WetsOntwerp
    } else if raw.contains("tabellen of lijsten") {
        DocumentType::TabellenOfLijsten
    } else if raw.contains("verantwoording") {
        DocumentType::Verantwoording
    } else if raw.contains("overgezonden ontwerp") {
        DocumentType::OvergezondenOntwerp
    } else if raw.contains("verslag (verwijzend)") {
        DocumentType::VerslagVerwijzend
    } else if raw.contains("bijlage") {
        DocumentType::Bijlage
    } else if raw.contains("opmerkingen van het rekenhof") {
        DocumentType::OpmerkingenVanHetRekenhof
    } else if raw.contains("verslag") {
        DocumentType::Verslag
    } else if raw.contains("advies van de raad van state") {
        DocumentType::AdviesVanDeRaadVanState
    } else if raw.contains("motie aangenomen") {
        DocumentType::AangenomenMotie
    } else if raw.contains("voorstel van motie") {
        DocumentType::VoorstelVanMotie
    } else if raw.contains("aangenomen tekst") {
        DocumentType::AangenomenTekst
    } else if raw.contains("advies") {
        DocumentType::Advies
    } else if raw.contains("voorstel onderzoekscommissie") {
        DocumentType::VoorstelOnderzoekscommissie
    } else if raw.contains("voorstel reglement") {
        DocumentType::VoorstelReglement
    } else if raw.contains("artikelen bij 1e stemming aangenomen") {
        DocumentType::ArtikelenBijEersteStemmingAangenomen
    } else if raw.contains("artikelen aangenomen in plenum") {
        DocumentType::ArtikelenAangenomenInPlenum
    } else if raw.contains("artikelen in 2e lezing aangenomen") {
        DocumentType::ArtikelenInTweedeLezingAangenomen
    } else if raw.contains("aangenomen tekst") {
        DocumentType::AangenomenTekst
    } else if raw.contains("errata") {
        DocumentType::Errata
    } else if raw.contains("begroting") {
        DocumentType::Begroting
    } else if raw.contains("niet-geevoceerd ontwerp") {
        DocumentType::NietGeevoceerdOntwerp
    } else if raw.contains("kaft") {
        DocumentType::Kaft
    } else {
        DocumentType::Onbekend
    }
}

fn write_dossiers(path: &Path, rows: &[ScrapedDossier]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("session_id", DataType::Utf8, false),
        Field::new("id", DataType::Utf8, false),
        Field::new("last_updated", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("authors", DataType::Utf8, false),
        Field::new("submission_date", DataType::Utf8, false),
        Field::new("end_date", DataType::Utf8, false),
        Field::new("vote_date", DataType::Utf8, false),
        Field::new("document_type", DataType::Utf8, false),
        Field::new("status", DataType::Utf8, false),
        Field::new("latest_adopted_text_url", DataType::Utf8, true),
        Field::new("latest_report_url", DataType::Utf8, true),
        Field::new("eurovoc_main_descriptor", DataType::Utf8, false),
        Field::new("eurovoc_descriptors", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |d| d.session_id.to_string()),
            col!(rows, |d| d.dossier_id.clone()),
            col!(rows, |d| d.last_updated.clone()),
            col!(rows, |d| d.title.clone()),
            col!(rows, |d| d.authors.clone()),
            col!(rows, |d| d.submission_date.clone()),
            col!(rows, |d| d.end_date.clone()),
            col!(rows, |d| d.vote_date.clone()),
            col!(rows, |d| d.document_type.clone()),
            col!(rows, |d| d.status.clone()),
            col_opt!(rows, |d| d.latest_adopted_text_url.clone()),
            col_opt!(rows, |d| d.latest_report_url.clone()),
            col!(rows, |d| d.eurovoc_main_descriptor.clone()),
            col!(rows, |d| d.eurovoc_descriptors.clone()),
            col!(rows, |d| d.source_url.clone()),
            col!(rows, |d| d.cache_path.clone()),
        ],
    )
}

fn write_subdocuments(path: &Path, rows: &[ScrapedSubdocument]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("dossier_id", DataType::Utf8, false),
        Field::new("id", DataType::Utf8, false),
        Field::new("date", DataType::Utf8, false),
        Field::new("type", DataType::Utf8, false),
        Field::new("authors", DataType::Utf8, false),
        Field::new("file_url", DataType::Utf8, true),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));

    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |s| s.dossier_id.clone()),
            col!(rows, |s| s.id.clone()),
            col!(rows, |s| s.date.clone()),
            col!(rows, |s| s.document_type.clone()),
            col!(rows, |s| s.authors.clone()),
            col_opt!(rows, |s| s.file_url.clone()),
            col!(rows, |s| s.source_url.clone()),
            col!(rows, |s| s.cache_path.clone()),
        ],
    )
}

fn normalize_author(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    if lower.contains("gouvernment") || lower.contains("regering") {
        "government".to_string()
    } else {
        name.trim().replace(",", "")
    }
}

fn normalize_date(date: &str) -> String {
    NaiveDate::parse_from_str(date.trim(), "%d/%m/%Y")
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| date.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_dossier_id_strips_leading_zeros() {
        assert_eq!(normalize_dossier_id("0099"), "99");
        assert_eq!(normalize_dossier_id("0001"), "1");
        assert_eq!(normalize_dossier_id("297"), "297");
        assert_eq!(normalize_dossier_id("0000"), "0");
    }

    #[test]
    fn extract_list_from_to_urls_finds_ranges() {
        let html = r#"
            <A HREF="showpage.cfm?section=/flwb&language=nl&amp;cfm=ListFromTo.cfm?legislat=56&amp;from=0&amp;to=99">Van 99 tot 0</A>
            <A HREF="showpage.cfm?section=/flwb&language=nl&cfm=ListFromTo.cfm?legislat=56&from=100&to=199">Van 199 tot 100</A>
            <A HREF="showpage.cfm?section=/flwb&language=nl&cfm=ListFromTo.cfm?legislat=55&from=0&to=99">Other session</A>
        "#;
        let urls = extract_list_from_to_urls(html, 56);
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("from=0&to=99"));
        assert!(urls[1].contains("from=100&to=199"));
    }

    #[test]
    fn extract_flwb_dossier_ids_from_range_page() {
        let html = r#"
            <A HREF="showpage.cfm?section=/flwb&language=nl&cfm=/site/wwwcfm/flwb/flwbn.cfm?lang=N&legislat=56&dossierID=0099">99</A>
            <A HREF="showpage.cfm?section=/flwb&language=nl&cfm=/site/wwwcfm/flwb/flwbn.cfm?lang=N&legislat=56&dossierID=0098">98</A>
            <A HREF="/site/wwwcfm/flwb/lastpdf.cfm?lang=N&dossierID=4">skip</A>
        "#;
        let ids = extract_flwb_dossier_ids(html);
        assert_eq!(ids, vec!["99".to_string(), "98".to_string()]);
    }

    #[test]
    fn recheck_max_age_uses_shorter_window_for_recent_plenary_meeting() {
        let today = Local::now().naive_local().date();
        let recent = (today - chrono::Duration::days(2))
            .format("%Y-%m-%d")
            .to_string();
        let old = (today - chrono::Duration::days(30))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(recheck_max_age_days(&recent), ACTIVE_RECHECK_DAYS);
        assert_eq!(recheck_max_age_days(&old), DEFAULT_RECHECK_DAYS);
        assert_eq!(recheck_max_age_days(""), DEFAULT_RECHECK_DAYS);
    }

    #[test]
    fn dossier_fingerprint_is_stable_for_cached_html() {
        let path = Path::new("cache/sessions/56/dossiers/56_1000_2026-07-01.html");
        if !path.exists() {
            return;
        }
        let html = std::fs::read_to_string(path).expect("read cached dossier html");
        let first = dossier_content_fingerprint(&html, "1000").expect("fingerprint");
        let second = dossier_content_fingerprint(&html, "1000").expect("fingerprint");
        assert_eq!(first, second);
        assert!(first.contains("Aangenomen"));
    }

    #[test]
    fn adopted_dossier_with_old_end_date_is_settled() {
        let path = Path::new("cache/sessions/56/dossiers/56_1000_2026-07-01.html");
        if !path.exists() {
            return;
        }
        let html = std::fs::read_to_string(path).expect("read cached dossier html");
        let dossier =
            scrape_dossier("1000", &Html::parse_document(&html)).expect("scrape dossier");
        assert!(is_settled_dossier(&dossier));
    }
}
