use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use chrono::{Local, NaiveDate};
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::utils::relative_cache_path;
use crawl::{
    BundlePublisher, MANIFEST_STATUS_NOT_FOUND, MANIFEST_STATUS_PARSED, SourceManifestRow,
    content_hash_bytes, days_between_rfc3339, manifest_path, now_rfc3339, read_cache_metadata,
    require_cache_present, touch_checked_at, validate_manifest_rows, write_cache_artifact,
    write_source_manifest, write_text_atomic,
};
use encoding_rs::WINDOWS_1252;
use http::StatusCode;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parquet::arrow::ArrowWriter;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::fs::{File, read_to_string};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::fs;

const DEKAMER_BASE: &str = "https://www.dekamer.be";
const SESSION_ID: u32 = 56;
const SOURCE_NAME: &str = "dossiers";
/// Re-check open dossiers at most once per week unless linked to a recent plenary meeting.
const DEFAULT_RECHECK_DAYS: i64 = 7;
/// Dossiers tied to a plenary meeting in the last week are re-checked daily.
const ACTIVE_RECHECK_DAYS: i64 = 1;
/// Terminal (settled) dossiers are re-checked on a slower cadence, not permanently skipped.
const SETTLED_RECHECK_DAYS: i64 = 90;

static SELECTOR_TR: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TD: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TABLE: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TBODY: OnceLock<Selector> = OnceLock::new();
static SELECTOR_A: OnceLock<Selector> = OnceLock::new();
static SELECTOR_FONT: OnceLock<Selector> = OnceLock::new();
static LIST_FROM_TO_REGEX: OnceLock<Regex> = OnceLock::new();
static FLWB_DOSSIER_ID_REGEX: OnceLock<Regex> = OnceLock::new();
static FLWB_DOCUMENT_ID_REGEX: OnceLock<Regex> = OnceLock::new();

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
    FLWB_DOSSIER_ID_REGEX
        .get_or_init(|| Regex::new(r#"(?i)flwbn\.cfm[^"'<>]*dossierID=(\d+)"#).unwrap())
}

fn flwb_document_id_regex() -> &'static Regex {
    FLWB_DOCUMENT_ID_REGEX
        .get_or_init(|| Regex::new(r"(?i)/(\d{2}K\d{7})\.pdf(?:[?#].*)?$").unwrap())
}

/// The output of this scraper.
struct DossierScrapeOutput {
    dossiers: Vec<ScrapedDossier>,
    subdocuments: Vec<ScrapedSubdocument>,
    manifest_rows: Vec<SourceManifestRow>,
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
    original_text_url: Option<String>,
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
    original_text_url: Option<String>,
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
    Naturalisatielijsten,
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
    VoorstelVanNaturalisatieAkte,
    VoorstelVanVerklaring,
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

    let session_id = SESSION_ID;
    let session_dir = data_dir().join("sessions").join(session_id.to_string());
    fs::create_dir_all(&session_dir).await?;

    let mp = MultiProgress::new();
    let mut web_request_count = 0u32;
    let run_mode = if cache_only() {
        SourceManifestRow::run_mode_cache_only()
    } else {
        SourceManifestRow::run_mode_live()
    };

    let (id_dates, not_found) = if cache_only() {
        println!("[dossiers] cache-only: loading inventory, parsing cached HTML only");
        let id_dates = load_dossier_inventory(session_id)?;
        ensure_cache_only_inventory_present(session_id, &id_dates)?;
        (id_dates, HashSet::new())
    } else {
        let client = ScrapingClient::new();
        let mut id_dates = load_plenary_dossier_ids(session_id);
        let flwb_ids =
            discover_all_dossier_ids(session_id, &client, &mut web_request_count).await?;
        let plenary_only = id_dates.len();
        for id in flwb_ids {
            id_dates.entry(id).or_default();
        }
        let flwb_only = id_dates.len().saturating_sub(plenary_only);
        println!(
            "[dossiers] {} dossier ids to download ({} plenary, {} FLWB-only)",
            id_dates.len(),
            plenary_only,
            flwb_only
        );
        persist_dossier_inventory(session_id, &id_dates)?;
        let not_found =
            download_dossiers(session_id, &id_dates, &client, &mut web_request_count, &mp).await?;
        (id_dates, not_found)
    };

    let DossierScrapeOutput {
        dossiers,
        subdocuments,
        manifest_rows,
    } = scrape_expected_dossiers(session_id, &id_dates, &not_found, run_mode, &mp)?;

    reconcile_dossier_outputs(&id_dates, &dossiers, &subdocuments, &manifest_rows)?;
    publish_dossier_bundle(&session_dir, &dossiers, &subdocuments, &manifest_rows)?;

    println!(
        "[dossiers] scraped {} dossiers ({} not_found) using {} web requests",
        dossiers.len(),
        not_found.len(),
        web_request_count
    );
    Ok(())
}

fn publish_dossier_bundle(
    session_dir: &Path,
    dossiers: &[ScrapedDossier],
    subdocuments: &[ScrapedSubdocument],
    manifest_rows: &[SourceManifestRow],
) -> Result<(), Box<dyn Error>> {
    validate_manifest_rows(manifest_rows)?;

    let manifest_final = manifest_path(SOURCE_NAME);
    let mut bundle = BundlePublisher::new("dossiers", &data_dir())?;
    let stage_dossiers = bundle.stage_path(&session_dir.join("dossiers.parquet"))?;
    let stage_subdocuments = bundle.stage_path(&session_dir.join("subdocuments.parquet"))?;
    let stage_manifest = bundle.stage_path(&manifest_final)?;

    write_dossiers(&stage_dossiers, dossiers)?;
    write_subdocuments(&stage_subdocuments, subdocuments)?;
    write_source_manifest(&stage_manifest, manifest_rows)?;
    bundle.commit()?;
    Ok(())
}

async fn download_dossiers(
    session_id: u32,
    id_dates: &HashMap<String, String>,
    client: &ScrapingClient,
    web_request_count: &mut u32,
    mp: &MultiProgress,
) -> Result<HashSet<String>, Box<dyn Error>> {
    let pb = mp.add(ProgressBar::new(id_dates.len() as u64));
    pb.set_style(
        ProgressStyle::with_template(
            "[dossiers-download] [{elapsed_precise}] {spinner:.blue} {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}",
        )?
        .tick_chars("⠋⠙⠹⠼⠼⠴⠦⠧⠇⠏"),
    );

    let mut stats = DownloadStats::default();
    let mut not_found = HashSet::new();
    for (id, latest_meeting_date) in id_dates {
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
        if action == DownloadAction::NotFound {
            not_found.insert(id.clone());
        }
        stats.record(action);
        pb.inc(1);
    }

    pb.finish_with_message("done");
    println!(
        "[dossiers] download cache: {} settled, {} fresh, {} unchanged, {} updated, {} not_found ({} web requests)",
        stats.skipped_settled,
        stats.skipped_fresh,
        stats.skipped_unchanged,
        stats.downloaded,
        stats.not_found,
        web_request_count
    );
    Ok(not_found)
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
    parse_dossier_id_tsv(&content)
}

fn dossier_inventory_path(session_id: u32) -> PathBuf {
    cache_dir().join(format!("sessions/{}/dossier_inventory.tsv", session_id))
}

fn parse_dossier_id_tsv(content: &str) -> HashMap<String, String> {
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

fn persist_dossier_inventory(
    session_id: u32,
    id_dates: &HashMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let path = dossier_inventory_path(session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut lines: Vec<String> = id_dates
        .iter()
        .map(|(id, date)| format!("{id}\t{date}"))
        .collect();
    lines.sort();
    write_text_atomic(&path, &(lines.join("\n") + "\n"))?;
    Ok(())
}

fn load_dossier_inventory(session_id: u32) -> Result<HashMap<String, String>, Box<dyn Error>> {
    let path = dossier_inventory_path(session_id);
    if path.exists() {
        return Ok(parse_dossier_id_tsv(&std::fs::read_to_string(&path)?));
    }

    // One-time bootstrap so cache-only reparse works before the first live inventory write.
    // Prefer plenary discovery sidecar; union dossier ids already present in the HTML cache.
    let mut id_dates = load_plenary_dossier_ids(session_id);
    for id in dossier_ids_from_cache_filenames(session_id) {
        id_dates.entry(id).or_default();
    }
    if id_dates.is_empty() {
        return Err(format!(
            "cache-only incomplete snapshot: expected dossier inventory at {} is missing and no prior plenary/cache ids found — aborting",
            path.display()
        )
        .into());
    }
    eprintln!(
        "[dossiers] bootstrapped dossier_inventory.tsv from plenary ids + cache filenames ({} ids)",
        id_dates.len()
    );
    persist_dossier_inventory(session_id, &id_dates)?;
    Ok(id_dates)
}

fn dossier_ids_from_cache_filenames(session_id: u32) -> Vec<String> {
    let dir = cache_dir().join(format!("sessions/{}/dossiers", session_id));
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let prefix = format!("{session_id}_");
    let mut ids = HashSet::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".html") else {
            continue;
        };
        let Some(rest) = stem.strip_prefix(&prefix) else {
            continue;
        };
        // `{id}_{version…}` — dossier id is the first underscore-separated segment.
        if let Some(id) = rest.split('_').next()
            && !id.is_empty()
        {
            ids.insert(id.to_string());
        }
    }
    ids.into_iter().collect()
}

fn ensure_cache_only_inventory_present(
    session_id: u32,
    id_dates: &HashMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let dossier_dir = cache_dir().join(format!("sessions/{}/dossiers", session_id));
    for id in id_dates.keys() {
        let path = find_cached_dossier_path(&dossier_dir, session_id, id);
        match path {
            Some(p) => require_cache_present(&p, &format!("dossier {id}"))?,
            None => {
                return Err(format!(
                    "cache-only incomplete snapshot: expected dossier {id} cache under {} is missing — aborting to preserve prior outputs",
                    dossier_dir.display()
                )
                .into());
            }
        }
    }
    Ok(())
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
    let list_html = match fetch_html(client, &list_url, web_request_count).await? {
        FetchHtml::Html(html) => html,
        FetchHtml::NotFound => {
            return Err(format!("FLWB list page 404 for session {session_id}").into());
        }
    };
    let range_urls = extract_list_from_to_urls(&list_html, session_id);

    if range_urls.is_empty() {
        return Err(
            format!("FLWB discovery found no ListFromTo ranges for session {session_id}").into(),
        );
    }

    let range_count = range_urls.len();
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for range_url in range_urls {
        let range_html = match fetch_html(client, &range_url, web_request_count).await? {
            FetchHtml::Html(html) => html,
            FetchHtml::NotFound => {
                return Err(format!("FLWB range page 404: {range_url}").into());
            }
        };
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

enum FetchHtml {
    Html(String),
    NotFound,
}

async fn fetch_html(
    client: &ScrapingClient,
    url: &str,
    web_request_count: &mut u32,
) -> Result<FetchHtml, Box<dyn Error>> {
    let response = client.get(url).await?;
    *web_request_count += 1;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(FetchHtml::NotFound);
    }
    if !response.status().is_success() {
        return Err(format!("unexpected HTTP {} for {url}", response.status()).into());
    }
    let raw_bytes = response.bytes().await?;
    let (decoded_str, _, _) = WINDOWS_1252.decode(&raw_bytes);
    Ok(FetchHtml::Html(decoded_str.into_owned()))
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
    NotFound,
}

#[derive(Debug, Default)]
struct DownloadStats {
    skipped_settled: u32,
    skipped_fresh: u32,
    skipped_unchanged: u32,
    downloaded: u32,
    not_found: u32,
}

impl DownloadStats {
    fn record(&mut self, action: DownloadAction) {
        match action {
            DownloadAction::SkippedSettled => self.skipped_settled += 1,
            DownloadAction::SkippedFresh => self.skipped_fresh += 1,
            DownloadAction::SkippedUnchanged => self.skipped_unchanged += 1,
            DownloadAction::Downloaded => self.downloaded += 1,
            DownloadAction::NotFound => self.not_found += 1,
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
    authors: String,
    file_url: String,
}

#[derive(Serialize)]
struct DossierContentFingerprint {
    title: String,
    authors: String,
    status: String,
    submission_date: String,
    end_date: String,
    vote_date: String,
    document_type: String,
    eurovoc_main_descriptor: String,
    eurovoc_descriptors: String,
    subdocuments: Vec<SubdocumentFingerprint>,
}

fn sorted_csv(values: &[String]) -> String {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted.join(",")
}

fn sorted_comma_separated(raw: &str) -> String {
    let mut parts: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    parts.sort();
    parts.join(",")
}

/// Fingerprint of dossier output-relevant fields (title, authors, dates, type, status,
/// Eurovoc, and subdocument ids/dates/types/authors/file URLs). Multi-value fields are
/// sorted so source-order-only changes stay stable.
fn dossier_content_fingerprint(html: &str, dossier_id: &str) -> Result<String, Box<dyn Error>> {
    let document = Html::parse_document(html);
    let dossier = scrape_dossier(dossier_id, &document)?;
    let mut subdocuments = dossier
        .subdocuments
        .iter()
        .map(|subdocument| {
            let mut authors = subdocument.authors.clone();
            authors.sort();
            SubdocumentFingerprint {
                id: subdocument.id.clone(),
                date: subdocument.date.clone(),
                document_type: subdocument.document_type.to_string(),
                authors: authors.join(","),
                file_url: subdocument.file_url.clone().unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    subdocuments.sort_by(|left, right| left.id.cmp(&right.id));

    let fingerprint = DossierContentFingerprint {
        title: dossier.title,
        authors: sorted_csv(&dossier.authors),
        status: dossier.status.to_string(),
        submission_date: dossier.submission_date,
        end_date: dossier.end_date,
        vote_date: dossier.vote_date,
        document_type: dossier.document_type.to_string(),
        eurovoc_main_descriptor: dossier.eurovoc_main_descriptor,
        eurovoc_descriptors: sorted_comma_separated(&dossier.eurovoc_descriptors),
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

/// Terminal dossiers with an end/vote date more than a week ago use the slower recheck cadence.
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

fn recheck_max_age_days(latest_meeting_date: &str, settled: bool) -> i64 {
    if settled {
        return SETTLED_RECHECK_DAYS;
    }
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

/// Days since `.meta.json` `checked_at` (not file mtime).
fn days_since_checked_at(path: &Path) -> Option<i64> {
    let meta = read_cache_metadata(path).ok()??;
    days_between_rfc3339(&meta.checked_at, &now_rfc3339())
}

fn compact_utc_stamp(rfc3339: &str) -> String {
    // 2026-07-15T19:21:00Z -> 20260715T192100Z
    rfc3339.chars().filter(|c| *c != '-' && *c != ':').collect()
}

fn dossier_versioned_cache_name(session_id: u32, dossier_id: &str, bytes: &[u8]) -> String {
    let stamp = compact_utc_stamp(&now_rfc3339());
    let hash8 = &content_hash_bytes(bytes)[..8];
    format!("{session_id}_{dossier_id}_{stamp}_{hash8}.html")
}

fn list_dossier_cache_candidates(
    dossier_dir: &Path,
    session_id: u32,
    dossier_id: &str,
) -> Vec<PathBuf> {
    let filename_prefix = format!("{session_id}_{dossier_id}_");
    let Ok(entries) = std::fs::read_dir(dossier_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with(&filename_prefix) && name.ends_with(".html")
        })
        .collect()
}

/// Deterministically pick the newest retained dossier HTML version.
fn select_newest_dossier_cache(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .max_by(|left, right| dossier_cache_sort_key(left).cmp(&dossier_cache_sort_key(right)))
        .cloned()
}

fn dossier_cache_sort_key(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn find_cached_dossier_path(
    dossier_dir: &Path,
    session_id: u32,
    dossier_id: &str,
) -> Option<PathBuf> {
    let candidates = list_dossier_cache_candidates(dossier_dir, session_id, dossier_id);
    select_newest_dossier_cache(&candidates)
}

fn dossier_url(session_id: u32, dossier_id: &str) -> String {
    format!(
        "https://www.dekamer.be/kvvcr/showpage.cfm?section=/flwb&language=nl&cfm=/site/wwwcfm/flwb/flwbn.cfm?lang=N&legislat={}&dossierID={}",
        session_id, dossier_id
    )
}

/// Checks whether a cached dossier HTML is still current.
///
/// Uses the content fingerprint. Terminal dossiers use a 90-day recheck interval;
/// open dossiers are re-checked weekly (daily when linked to a recent plenary meeting).
/// Freshness uses `.meta.json` `checked_at`, not file mtime. Content changes retain the
/// previous cache file and write a new versioned artifact.
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
    let existing_file = find_cached_dossier_path(&dossier_dir, session_id, dossier_id);

    if let Some(cache_path) = &existing_file {
        let cached_html = read_to_string(cache_path)?;
        let cached_dossier = scrape_dossier(dossier_id, &Html::parse_document(&cached_html))?;
        let settled = is_settled_dossier(&cached_dossier);
        let max_age = recheck_max_age_days(latest_meeting_date, settled);

        if days_since_checked_at(cache_path).is_some_and(|age| age < max_age) {
            return Ok(if settled {
                DownloadAction::SkippedSettled
            } else {
                DownloadAction::SkippedFresh
            });
        }

        let cached_fingerprint = dossier_content_fingerprint(&cached_html, dossier_id)?;
        match fetch_html(client, &url, web_request_count).await? {
            FetchHtml::NotFound => {
                // Keep the prior version; treat as unchanged for this run.
                touch_checked_at(cache_path)?;
                return Ok(DownloadAction::SkippedUnchanged);
            }
            FetchHtml::Html(live_html) => {
                let live_fingerprint = dossier_content_fingerprint(&live_html, dossier_id)?;
                if cached_fingerprint == live_fingerprint {
                    touch_checked_at(cache_path)?;
                    return Ok(DownloadAction::SkippedUnchanged);
                }

                let bytes = live_html.as_bytes();
                let new_path =
                    dossier_dir.join(dossier_versioned_cache_name(session_id, dossier_id, bytes));
                write_cache_artifact(&new_path, bytes, &url, "text/html; charset=windows-1252")?;
                return Ok(DownloadAction::Downloaded);
            }
        }
    }

    match fetch_html(client, &url, web_request_count).await? {
        FetchHtml::NotFound => Ok(DownloadAction::NotFound),
        FetchHtml::Html(live_html) => {
            let bytes = live_html.as_bytes();
            let new_path =
                dossier_dir.join(dossier_versioned_cache_name(session_id, dossier_id, bytes));
            write_cache_artifact(&new_path, bytes, &url, "text/html; charset=windows-1252")?;
            Ok(DownloadAction::Downloaded)
        }
    }
}

/// Scrape expected dossier IDs from inventory (newest cache file each).
fn scrape_expected_dossiers(
    session_id: u32,
    id_dates: &HashMap<String, String>,
    not_found: &HashSet<String>,
    run_mode: &str,
    mp: &MultiProgress,
) -> Result<DossierScrapeOutput, Box<dyn Error>> {
    let dossier_dir = cache_dir().join(format!("sessions/{}/dossiers", session_id));
    let mut expected_ids: Vec<String> = id_dates.keys().cloned().collect();
    expected_ids.sort_by(|a, b| {
        a.parse::<u32>()
            .unwrap_or(0)
            .cmp(&b.parse::<u32>().unwrap_or(0))
    });

    let pb = mp.add(ProgressBar::new(expected_ids.len() as u64));
    pb.set_style(
        ProgressStyle::with_template(
            "[dossiers-scrape] [{elapsed_precise}] {spinner:.blue} {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}",
        )?
        .tick_chars("⠋⠙⠹⠼⠼⠴⠦⠧⠇⠏"),
    );

    let mut dossiers = Vec::new();
    let mut subdocuments = Vec::new();
    let mut manifest_rows = Vec::new();

    for dossier_id in &expected_ids {
        pb.set_message(format!("id={}", dossier_id));
        let source_url = dossier_url(session_id, dossier_id);

        if not_found.contains(dossier_id) {
            manifest_rows.push(SourceManifestRow {
                source: SOURCE_NAME.into(),
                session_id: session_id.to_string(),
                item_kind: "dossier".into(),
                native_item_id: dossier_id.clone(),
                source_url,
                cache_path: String::new(),
                status: MANIFEST_STATUS_NOT_FOUND.into(),
                row_count: 0,
                content_type: String::new(),
                content_hash: String::new(),
                fetched_at: String::new(),
                checked_at: now_rfc3339(),
                run_mode: run_mode.into(),
                detail: "HTTP 404".into(),
            });
            pb.inc(1);
            continue;
        }

        let path =
            find_cached_dossier_path(&dossier_dir, session_id, dossier_id).ok_or_else(|| {
                format!(
                    "missing cache for expected dossier {dossier_id} under {}",
                    dossier_dir.display()
                )
            })?;
        if cache_only() {
            require_cache_present(&path, &format!("dossier {dossier_id}"))?;
        }

        let content = read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        let document = Html::parse_document(&content);
        let dossier = scrape_dossier(dossier_id, &document)
            .map_err(|e| format!("Failed to scrape dossier {dossier_id}: {e}"))?;

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let prefix = format!("{session_id}_{dossier_id}_");
        let last_updated = stem
            .strip_prefix(&prefix)
            .unwrap_or(stem.as_str())
            .to_string();

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

        let latest_report_url = dossier
            .subdocuments
            .iter()
            .rev()
            .find(|s| matches!(s.document_type, DocumentType::Verslag) && s.file_url.is_some())
            .and_then(|s| s.file_url.clone());

        let cache_path_rel = relative_cache_path(&path, &cache_dir());
        let meta = read_cache_metadata(&path)?;
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

        let subdoc_count = dossier.subdocuments.len() as u32;
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
            dossier_id: dossier_id.clone(),
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
            original_text_url: dossier.original_text_url,
            source_url: source_url.clone(),
            cache_path: cache_path_rel.clone(),
        });

        manifest_rows.push(SourceManifestRow {
            source: SOURCE_NAME.into(),
            session_id: session_id.to_string(),
            item_kind: "dossier".into(),
            native_item_id: dossier_id.clone(),
            source_url,
            cache_path: cache_path_rel,
            status: MANIFEST_STATUS_PARSED.into(),
            row_count: 1 + subdoc_count,
            content_type: "text/html".into(),
            content_hash,
            fetched_at,
            checked_at,
            run_mode: run_mode.into(),
            detail: String::new(),
        });
        pb.inc(1);
    }

    pb.finish_with_message("done");
    Ok(DossierScrapeOutput {
        dossiers,
        subdocuments,
        manifest_rows,
    })
}

fn reconcile_dossier_outputs(
    id_dates: &HashMap<String, String>,
    dossiers: &[ScrapedDossier],
    subdocuments: &[ScrapedSubdocument],
    manifest_rows: &[SourceManifestRow],
) -> Result<(), Box<dyn Error>> {
    let expected: HashSet<String> = id_dates.keys().cloned().collect();
    let manifest_keys: HashSet<String> = manifest_rows
        .iter()
        .filter(|r| r.item_kind == "dossier")
        .map(|r| r.native_item_id.clone())
        .collect();
    if expected != manifest_keys {
        let missing: Vec<_> = expected.difference(&manifest_keys).cloned().collect();
        let extra: Vec<_> = manifest_keys.difference(&expected).cloned().collect();
        return Err(format!(
            "dossier inventory/manifest mismatch: missing={missing:?} extra={extra:?}"
        )
        .into());
    }

    let parsed: HashSet<String> = manifest_rows
        .iter()
        .filter(|r| r.status == MANIFEST_STATUS_PARSED)
        .map(|r| r.native_item_id.clone())
        .collect();
    let not_found: HashSet<String> = manifest_rows
        .iter()
        .filter(|r| r.status == MANIFEST_STATUS_NOT_FOUND)
        .map(|r| r.native_item_id.clone())
        .collect();
    if !parsed.is_disjoint(&not_found) {
        return Err("dossier manifest has overlapping parsed and not_found ids".into());
    }

    let dossier_ids: HashSet<String> = dossiers.iter().map(|d| d.dossier_id.clone()).collect();
    if dossier_ids != parsed {
        let missing: Vec<_> = parsed.difference(&dossier_ids).cloned().collect();
        let extra: Vec<_> = dossier_ids.difference(&parsed).cloned().collect();
        return Err(format!(
            "parsed dossiers mismatch manifest: missing={missing:?} extra={extra:?}"
        )
        .into());
    }

    for sub in subdocuments {
        if !parsed.contains(&sub.dossier_id) {
            return Err(format!(
                "subdocument {} references dossier {} outside parsed set",
                sub.id, sub.dossier_id
            )
            .into());
        }
    }
    Ok(())
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
    let mut original_text_url: Option<String> = None;

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
            let label = normalize_label(col_1.text().collect::<String>());
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
            } else if label.contains("document kamer") {
                if original_text_url.is_none() {
                    original_text_url = col_2
                        .select(selector_a())
                        .filter_map(|a| a.value().attr("href"))
                        .find(|href| href.ends_with(".pdf"))
                        .map(|href| {
                            if href.starts_with("http") {
                                href.to_string()
                            } else {
                                format!("{}{}", DEKAMER_BASE, href)
                            }
                        });
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

    // The initial `/001` document is rendered as a top-level `Document Kamer`
    // row, while later documents live in the nested `Subdocumenten` table.
    // Example: dossier 62, https://www.dekamer.be/FLWB/PDF/56/0062/56K0062001.pdf.
    // Keep the primary document first so the source order remains `/001`, `/002`, ….
    if let Some(primary_document) = parse_primary_document(dossier_id, &document_table)
        && !subdocuments
            .iter()
            .any(|subdocument| subdocument.id == primary_document.id)
    {
        subdocuments.insert(0, primary_document);
    }

    // Upstream behaviour: the original submitted text scraped from the
    // "Document Kamer" row is actually subdocument 1 (the main document).
    // Make sure it shows up in `subdocuments`, backfilling the primary
    // document's file_url when the top-level table did not expose a PDF.
    // Only insert when `parse_primary_document` found nothing, otherwise
    // the primary document would appear twice under different ids.
    if let Some(existing) = subdocuments.iter_mut().find(|s| s.id == "1") {
        if existing.file_url.is_none() {
            existing.file_url = original_text_url.clone();
        }
    } else if subdocuments.is_empty()
        && (original_text_url.is_some() || !submission_date.is_empty())
    {
        subdocuments.insert(
            0,
            Subdocument {
                dossier_id: dossier_id.to_string(),
                id: "1".to_string(),
                document_type,
                date: submission_date.clone(),
                authors: dossier_authors.clone(),
                file_url: original_text_url.clone(),
            },
        );
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
        original_text_url,
    })
}

/// Parse the primary `/001` document from the top-level dossier table.
///
/// dekamer.be separates it from the nested `Subdocumenten` table used for
/// subsequent documents. We only accept a PDF whose FLWB id belongs to this
/// dossier, so linked documents from another dossier are not ingested here.
fn parse_primary_document(dossier_id: &str, document_table: &ElementRef) -> Option<Subdocument> {
    let tbody = document_table.select(selector_tbody()).next()?;
    let expected_prefix = format!("{SESSION_ID:02}K{:0>4}", normalize_dossier_id(dossier_id));
    let mut document_id = String::new();
    let mut document_type = DocumentType::Onbekend;
    let mut document_date = String::new();
    let mut document_authors = Vec::new();
    let mut file_url = None;
    let mut parsing_primary = false;

    for row in document_table.select(selector_tr()) {
        if row.parent().is_none_or(|parent| parent != *tbody) {
            continue;
        }

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

        if !parsing_primary {
            let Some(pdf_url) = pdf_url_from_cell(&col_2) else {
                continue;
            };
            let Some(captures) = flwb_document_id_regex().captures(&pdf_url) else {
                continue;
            };
            let source_document_id = captures[1].to_uppercase();
            if label.contains("document")
                && !label.contains("gekoppeld")
                && source_document_id.starts_with(&expected_prefix)
            {
                document_id = source_document_id;
                file_url = Some(pdf_url);
                parsing_primary = true;
            }
            continue;
        }

        if label.contains("indieningsdatum") && document_date.is_empty() {
            document_date = normalize_date(&value);
        } else if label.contains("document type") {
            document_type = parse_document_type(&value);
        } else if label.contains("auteur(s)") {
            document_authors.extend(
                col_2
                    .select(selector_a())
                    .filter_map(|link| link.text().next())
                    .map(normalize_author),
            );
        }
    }

    // A few valid primary records expose only the PDF (for example dossier
    // 1506 / 56K1506001) and no separate date row. Retain the document rather
    // than silently dropping the only source-backed record.
    if document_id.is_empty() {
        return None;
    }

    Some(Subdocument {
        dossier_id: dossier_id.to_string(),
        id: document_id,
        document_type,
        date: document_date,
        authors: document_authors,
        file_url,
    })
}

fn pdf_url_from_cell(cell: &ElementRef) -> Option<String> {
    cell.select(selector_a())
        .filter_map(|link| link.value().attr("href"))
        .filter(|href| flwb_document_id_regex().is_match(href))
        .last()
        .map(|href| {
            if href.starts_with("http") {
                href.to_string()
            } else {
                format!("{DEKAMER_BASE}{href}")
            }
        })
}

fn normalize_label(text: String) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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
                    id: canonical_document_id(dossier_id, &document_id, file_url.as_deref()),
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

            let cell_1_text = cell_1.text().collect::<String>();
            // The link cell (cell_1) usually contains an <a> whose text is the
            // sub-document number, e.g. "003", stripped down to 3.
            // Some subdocuments ("niet beschikbaar") have no <a> at all so the number is plain text instead.
            // Only attempt this on the header row of a subdocument (i.e. before
            // document_id has been set) — later rows like "Datum ronddeling" or
            // "Auteur(s)" also lack an <a> and must not clobber the id.
            if document_id.is_empty() {
                if let Some(link) = cell_1.select(selector_a()).last() {
                    if let Some(id_text) = link.text().next() {
                        document_id = id_text.trim().trim_start_matches('0').to_string();
                    }
                } else if let Some(id_text) = cell_1_text.split_whitespace().next() {
                    document_id = id_text.trim().trim_start_matches('0').to_string();
                }
            }

            // cell_2 may carry an inline <font> tag with the document type.
            if let Some(font) = cell_2.select(selector_font()).next() {
                let raw_type = font.text().collect::<String>();
                document_type = parse_document_type(raw_type.trim());

                // Capture the linked document URL.
                let pdf_url = pdf_url_from_cell(&cell_1);

                if pdf_url.is_some() {
                    file_url = pdf_url;
                }
            }

            // Explicitly flag documents marked as unavailable (no PDF exists).
            if cell_1_text.to_lowercase().contains("niet beschikbaar") {
                file_url = Some("NIET_BESCHIKBAAR".to_string());
            }

            if label.contains("datum ronddeling") {
                document_date = normalize_date(&value);
            }

            if label.contains("auteur(s)") {
                parsing_authors = true;
            }

            if parsing_authors
                && let Some(link) = cell_2.select(selector_a()).next()
                && let Some(name) = link.text().next()
            {
                document_authors.push(normalize_author(name));
            }

            if !document_id.is_empty() && !document_date.is_empty() {
                complete_subdocument = true;
            }
        }
    }

    if complete_subdocument {
        subdocuments.push(Subdocument {
            dossier_id: dossier_id.to_string(),
            id: canonical_document_id(dossier_id, &document_id, file_url.as_deref()),
            document_type,
            date: document_date,
            authors: document_authors,
            file_url,
        });
    }

    subdocuments
}

fn canonical_document_id(
    dossier_id: &str,
    document_number: &str,
    file_url: Option<&str>,
) -> String {
    if let Some(url) = file_url
        && let Some(captures) = flwb_document_id_regex().captures(url)
    {
        return captures[1].to_uppercase();
    }

    format!(
        "{SESSION_ID:02}K{:0>4}{:0>3}",
        normalize_dossier_id(dossier_id),
        document_number.trim().trim_start_matches('0')
    )
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
    } else if raw.contains("voorstel van naturalisatieakte") {
        DocumentType::VoorstelVanNaturalisatieAkte
    } else if raw.contains("voorstel van verklaring") {
        DocumentType::VoorstelVanVerklaring
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
    } else if raw.contains("errata") {
        DocumentType::Errata
    } else if raw.contains("begroting") {
        DocumentType::Begroting
    } else if raw.contains("niet-geevoceerd ontwerp") {
        DocumentType::NietGeevoceerdOntwerp
    } else if raw.contains("kaft") {
        DocumentType::Kaft
    } else if raw.contains("naturalisatielijsten") {
        DocumentType::Naturalisatielijsten
    } else {
        DocumentType::Onbekend
    }
}

fn write_dossiers(path: &Path, rows: &[ScrapedDossier]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("session_id", DataType::Utf8, false),
        Field::new("dossier_id", DataType::Utf8, false),
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
        Field::new("original_text_url", DataType::Utf8, true),
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
            col_opt!(rows, |d| d.original_text_url.clone()),
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

    fn sample_dossier_html(
        title: &str,
        authors_html: &str,
        eurovoc_main: &str,
        eurovoc_descriptors: &str,
        file_url: &str,
        status: &str,
        end_date: &str,
    ) -> String {
        format!(
            r#"
            <div id="story"><h4><center>{title}</center></h4></div>
            <table><tbody>
                <tr>
                    <td>Document Kamer</td>
                    <td>
                        <a href="{file_url}">56K0999001</a>
                        <br>WETSVOORSTEL - KAMER
                    </td>
                </tr>
                <tr><td>Indieningsdatum</td><td>22/07/2024</td></tr>
                <tr><td>Einddatum</td><td>{end_date}</td></tr>
                <tr><td>Document type</td><td>05 WETSVOORSTEL</td></tr>
                <tr><td>Status</td><td>{status}</td></tr>
                <tr><td>Auteur(s)</td><td>{authors_html}</td></tr>
                <tr><td>Eurovoc-hoofddescriptor</td><td>{eurovoc_main}</td></tr>
                <tr><td>Eurovoc descriptoren</td><td>{eurovoc_descriptors}</td></tr>
            </tbody></table>
            "#
        )
    }

    #[test]
    fn normalize_dossier_id_strips_leading_zeros() {
        assert_eq!(normalize_dossier_id("0099"), "99");
        assert_eq!(normalize_dossier_id("0001"), "1");
        assert_eq!(normalize_dossier_id("297"), "297");
        assert_eq!(normalize_dossier_id("0000"), "0");
    }

    #[test]
    fn canonical_document_id_uses_full_flwb_id_from_pdf_url() {
        assert_eq!(
            canonical_document_id(
                "1243",
                "2",
                Some("https://www.dekamer.be/FLWB/PDF/56/1243/56K1243002.pdf"),
            ),
            "56K1243002"
        );
        assert_eq!(canonical_document_id("1243", "2", None), "56K1243002");
    }

    #[test]
    fn scrape_dossier_includes_top_level_primary_document() {
        // Dossier 62 uses this top-level `Document Kamer` layout for /001;
        // its later documents, if any, are in a separate `Subdocumenten` table.
        let html = r#"
            <div id="story"><h4><center>Voorbeeld-dossier</center></h4></div>
            <table><tbody>
                <tr>
                    <td>Document Kamer</td>
                    <td>
                        <a href="/FLWB/PDF/56/0062/56K0062001.pdf">56K0062001</a>
                        <br>WETSVOORSTEL - KAMER
                    </td>
                </tr>
                <tr><td>Indieningsdatum</td><td>22/07/2024</td></tr>
                <tr><td>Document type</td><td>05 WETSVOORSTEL</td></tr>
                <tr>
                    <td>Auteur(s)</td>
                    <td><a>Nathalie, Muylle</a><a>Els, Van Hoof</a></td>
                </tr>
                <tr>
                    <td>Gekoppeld(e)/verbonden document(en)</td>
                    <td><a href="/FLWB/PDF/56/0415/56K0415001.pdf">56K0415001</a></td>
                </tr>
            </tbody></table>
        "#;

        let dossier = scrape_dossier("62", &Html::parse_document(html)).expect("scrape dossier");
        assert_eq!(dossier.subdocuments.len(), 1);
        let primary = &dossier.subdocuments[0];
        assert_eq!(primary.id, "56K0062001");
        assert_eq!(primary.date, "2024-07-22");
        assert_eq!(primary.document_type.to_string(), "WetsVoorstel");
        assert_eq!(primary.authors, vec!["Nathalie Muylle", "Els Van Hoof"]);
        assert_eq!(
            primary.file_url.as_deref(),
            Some("https://www.dekamer.be/FLWB/PDF/56/0062/56K0062001.pdf")
        );
    }

    #[test]
    fn scrape_dossier_retains_primary_document_without_metadata_rows() {
        // Dossier 1506 (56K1506001) has a valid top-level PDF but no
        // Indieningsdatum row and an empty document-type value.
        let html = r#"
            <table><tbody>
                <tr>
                    <td>Document Kamer</td>
                    <td><a href="/FLWB/PDF/56/1506/56K1506001.pdf">56K1506001</a></td>
                </tr>
                <tr><td>Document type</td><td>00</td></tr>
            </tbody></table>
        "#;

        let dossier = scrape_dossier("1506", &Html::parse_document(html)).expect("scrape dossier");
        assert_eq!(dossier.subdocuments.len(), 1);
        let primary = &dossier.subdocuments[0];
        assert_eq!(primary.id, "56K1506001");
        assert!(primary.date.is_empty());
        assert_eq!(primary.document_type.to_string(), "Onbekend");
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
        assert_eq!(recheck_max_age_days(&recent, false), ACTIVE_RECHECK_DAYS);
        assert_eq!(recheck_max_age_days(&old, false), DEFAULT_RECHECK_DAYS);
        assert_eq!(recheck_max_age_days("", false), DEFAULT_RECHECK_DAYS);
        assert_eq!(recheck_max_age_days(&recent, true), SETTLED_RECHECK_DAYS);
        assert_eq!(recheck_max_age_days("", true), SETTLED_RECHECK_DAYS);
    }

    #[test]
    fn adopted_dossier_with_old_end_date_is_settled() {
        let today = Local::now().naive_local().date();
        let end = (today - chrono::Duration::days(30))
            .format("%d/%m/%Y")
            .to_string();
        let html = sample_dossier_html(
            "Settled dossier",
            "<a>Alice, A</a>",
            "LAW",
            "JUSTICE | RIGHTS",
            "/FLWB/PDF/56/0999/56K0999001.pdf",
            "Aangenomen",
            &end,
        );
        let dossier = scrape_dossier("999", &Html::parse_document(&html)).expect("scrape");
        assert!(is_settled_dossier(&dossier));
    }

    #[test]
    fn open_dossier_is_not_settled() {
        let html = sample_dossier_html(
            "Open dossier",
            "<a>Alice, A</a>",
            "LAW",
            "JUSTICE",
            "/FLWB/PDF/56/0999/56K0999001.pdf",
            "Hangend Kamer",
            "",
        );
        let dossier = scrape_dossier("999", &Html::parse_document(&html)).expect("scrape");
        assert!(!is_settled_dossier(&dossier));
    }

    #[test]
    fn fingerprint_changes_when_title_authors_eurovoc_or_file_url_change() {
        let base = sample_dossier_html(
            "Title A",
            "<a>Bob, B</a><a>Alice, A</a>",
            "LAW",
            "JUSTICE | RIGHTS",
            "/FLWB/PDF/56/0999/56K0999001.pdf",
            "Hangend Kamer",
            "",
        );
        let base_fp = dossier_content_fingerprint(&base, "999").expect("fp");

        let title_changed = sample_dossier_html(
            "Title B",
            "<a>Bob, B</a><a>Alice, A</a>",
            "LAW",
            "JUSTICE | RIGHTS",
            "/FLWB/PDF/56/0999/56K0999001.pdf",
            "Hangend Kamer",
            "",
        );
        assert_ne!(
            base_fp,
            dossier_content_fingerprint(&title_changed, "999").unwrap()
        );

        let authors_changed = sample_dossier_html(
            "Title A",
            "<a>Bob, B</a><a>Carol, C</a>",
            "LAW",
            "JUSTICE | RIGHTS",
            "/FLWB/PDF/56/0999/56K0999001.pdf",
            "Hangend Kamer",
            "",
        );
        assert_ne!(
            base_fp,
            dossier_content_fingerprint(&authors_changed, "999").unwrap()
        );

        let eurovoc_changed = sample_dossier_html(
            "Title A",
            "<a>Bob, B</a><a>Alice, A</a>",
            "LAW",
            "JUSTICE | HEALTH",
            "/FLWB/PDF/56/0999/56K0999001.pdf",
            "Hangend Kamer",
            "",
        );
        assert_ne!(
            base_fp,
            dossier_content_fingerprint(&eurovoc_changed, "999").unwrap()
        );

        let file_url_changed = sample_dossier_html(
            "Title A",
            "<a>Bob, B</a><a>Alice, A</a>",
            "LAW",
            "JUSTICE | RIGHTS",
            "/FLWB/PDF/56/0999/56K0999002.pdf",
            "Hangend Kamer",
            "",
        );
        assert_ne!(
            base_fp,
            dossier_content_fingerprint(&file_url_changed, "999").unwrap()
        );
    }

    #[test]
    fn fingerprint_stable_when_only_author_order_changes() {
        let order_a = sample_dossier_html(
            "Title A",
            "<a>Bob, B</a><a>Alice, A</a>",
            "LAW",
            "RIGHTS | JUSTICE",
            "/FLWB/PDF/56/0999/56K0999001.pdf",
            "Hangend Kamer",
            "",
        );
        let order_b = sample_dossier_html(
            "Title A",
            "<a>Alice, A</a><a>Bob, B</a>",
            "LAW",
            "JUSTICE | RIGHTS",
            "/FLWB/PDF/56/0999/56K0999001.pdf",
            "Hangend Kamer",
            "",
        );
        assert_eq!(
            dossier_content_fingerprint(&order_a, "999").unwrap(),
            dossier_content_fingerprint(&order_b, "999").unwrap()
        );
    }

    #[test]
    fn select_newest_dossier_cache_picks_lexicographically_latest() {
        let older = PathBuf::from("56_1000_20260701T120000Z_aaaaaaaa.html");
        let newer = PathBuf::from("56_1000_20260715T120000Z_bbbbbbbb.html");
        let legacy = PathBuf::from("56_1000_2026-07-01.html");
        let selected = select_newest_dossier_cache(&[older.clone(), newer.clone(), legacy]);
        assert_eq!(selected, Some(newer));
    }
}
