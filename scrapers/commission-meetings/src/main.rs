use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, data_dir};
use crawl::utils::{clean_text, composite_scoped_id, relative_cache_path};
use crawl::{
    extract_utterances_from_document, read_report_html, write_utterances_parquet, MeetingKind,
    UtteranceDraft,
};
use encoding_rs::WINDOWS_1252;
use http::StatusCode;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parquet::arrow::ArrowWriter;
use regex::Regex;
use scraper::{Html, Selector};
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tokio::fs;

/// REGEXES
static QUESTION_REGEX: OnceLock<Regex> = OnceLock::new();
static TIME_REGEX: OnceLock<Regex> = OnceLock::new();
static DATE_REGEX: OnceLock<Regex> = OnceLock::new();
static CHAIR_TITLES_REGEX: OnceLock<Regex> = OnceLock::new();
static CHAIR_REGEX: OnceLock<Regex> = OnceLock::new();

fn question_regex() -> &'static Regex {
    // FIXME: Respondents like "de vice-eersteminister en minister van Werk, Economie en Landbouw"
    //        are captured as-is; normalising them is left for a future pass.
    // NOTE: Handles question IDs in the format of `(56002763C)`, `(nr. 6003263c)` and `(n° 6003263c)`
    // NOTE: Handles both ” and " quotes (which is a mistake in meeting 157 question 8)
    // NOTE: Handles missing questionee (which is a mistake in meeting 357 question 35)
    QUESTION_REGEX.get_or_init(|| Regex::new(r#"(?m)(?:(?:Vraag van|Question de)\s)?([^\n]+?)(?:\s+(?:aan|à|au)\s+([^\n]+?))?(?:\s*\(.*?\))?\s*(?:over|sur)\s*["'“”](.+?)["'“”]\s*\(?(?:n[°ro]\.?\s*)?(\d{6,8}[A-Za-z])\)?"#).unwrap())
}

fn time_regex() -> &'static Regex {
    TIME_REGEX.get_or_init(|| Regex::new(r"(\d{1,2})[.:](\d{2})\s*uur\b").unwrap())
}

fn date_regex() -> &'static Regex {
    DATE_REGEX.get_or_init(|| Regex::new(r"(\d{1,2})\s+([a-zA-Z]+)\s+(\d{4})").unwrap())
}

fn chair_titles_regex() -> &'static Regex {
    CHAIR_TITLES_REGEX
        .get_or_init(|| Regex::new(r"(?i)\b(?:de\s+)?(?:mevrouw|heer|mevrouwen|heren)\b").unwrap())
}

fn chair_regex() -> &'static Regex {
    CHAIR_REGEX
        .get_or_init(|| Regex::new(r"(?i)voorgezeten\s+door\s+([^\.]+?)\s*(?:\.|$)").unwrap())
}

/// SELECTORS
static SELECTOR_SPAN: OnceLock<Selector> = OnceLock::new();
static SELECTOR_SPAN_P: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TABLE: OnceLock<Selector> = OnceLock::new();
static SELECTOR_H2_OR_P: OnceLock<Selector> = OnceLock::new();

fn selector_span() -> &'static Selector {
    SELECTOR_SPAN.get_or_init(|| Selector::parse("span").unwrap())
}
fn selector_span_p() -> &'static Selector {
    SELECTOR_SPAN_P.get_or_init(|| Selector::parse("span, p").unwrap())
}
fn selector_table() -> &'static Selector {
    SELECTOR_TABLE.get_or_init(|| Selector::parse("table").unwrap())
}
fn selector_h2_or_p() -> &'static Selector {
    SELECTOR_H2_OR_P.get_or_init(|| Selector::parse("h2, p").unwrap())
}

struct ScrapedMeeting {
    session_id: u32,
    meeting_id: u32,
    date: String,
    time_of_day: String,
    start_time: String,
    end_time: String,
    commission: String,
    chair: String,
    source_url: String,
    cache_path: String,
}

struct ScrapedQuestion {
    question_id: String,
    session_id: u32,
    meeting_id: u32,
    questioners: String,
    respondents: String,
    topics_nl: String,
    topics_fr: String,
    internal_ids: String,
    source_url: String,
    cache_path: String,
}

struct MeetingOutput {
    meeting: ScrapedMeeting,
    questions: Vec<ScrapedQuestion>,
    utterances: Vec<UtteranceDraft>,
}

#[derive(Debug, Clone)]
struct MeetingGap {
    meeting_id: u32,
    reason: String,
    detail: String,
}

fn record_gap(gaps: &mut std::collections::BTreeMap<u32, MeetingGap>, gap: MeetingGap) {
    gaps.entry(gap.meeting_id)
        .or_insert(gap);
}

/// Scan forward from `start_id`, treating `exists(probe_id)` as whether the report is online.
/// Stops after `max_consecutive_misses` consecutive missing ids.
fn discover_last_from_probes(
    start_id: u32,
    max_consecutive_misses: u32,
    mut exists: impl FnMut(u32) -> bool,
) -> (u32, Vec<u32>) {
    let mut last = start_id;
    let mut consecutive_misses = 0u32;
    let mut probe = start_id + 1;
    let mut missing = Vec::new();

    while consecutive_misses < max_consecutive_misses {
        if exists(probe) {
            last = probe;
            consecutive_misses = 0;
        } else {
            missing.push(probe);
            consecutive_misses += 1;
        }
        probe += 1;
    }

    (last, missing)
}

struct QuestionData {
    questioners: Vec<String>,
    respondents: Vec<String>,
    topics: Vec<String>,
    internal_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum Commission {
    Landsverdediging,
    Justitie,
    BuitenlandseBetrekkingen,
    FinancienEnBegroting,
    SocialeZakenWerkEnPensioenen,
    BinnenlandseZakenVeiligheidMigratieEnBestuurszaken,
    EconomieConsumentenBeschermingEnDigitalisering,
    MobiliteitOverheidsbedrijvenEnFederaleInstellingen,
    GezondheidEnGelijkeKansen,
    EnergieLeefmilieuEnKlimaat,
    InterparlementaireKlimaatdialoog,
    GrondwetEnInstitutioneleVernieuwing,
    Onbekend,
}

impl fmt::Display for Commission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Commission::BinnenlandseZakenVeiligheidMigratieEnBestuurszaken => {
                write!(
                    f,
                    "binnenlandse zaken, veiligheid, migratie en bestuurszaken"
                )
            }
            Commission::Landsverdediging => write!(f, "landsverdediging"),
            Commission::Justitie => write!(f, "justitie"),
            Commission::BuitenlandseBetrekkingen => write!(f, "buitenlandse betrekkingen"),
            Commission::FinancienEnBegroting => write!(f, "financiën en begroting"),
            Commission::SocialeZakenWerkEnPensioenen => {
                write!(f, "sociale zaken, werk en pensioenen")
            }
            Commission::EconomieConsumentenBeschermingEnDigitalisering => {
                write!(f, "economie, consumentenbescherming en digitalisering")
            }
            Commission::MobiliteitOverheidsbedrijvenEnFederaleInstellingen => {
                write!(f, "mobiliteit, overheidsbedrijven en federale instellingen")
            }
            Commission::GezondheidEnGelijkeKansen => write!(f, "gezondheid en gelijke kansen"),
            Commission::EnergieLeefmilieuEnKlimaat => write!(f, "energie, leefmilieu en klimaat"),
            Commission::InterparlementaireKlimaatdialoog => {
                write!(f, "interparlementaire klimaatdialoog")
            }
            Commission::GrondwetEnInstitutioneleVernieuwing => {
                write!(f, "grondwet en institutionele vernieuwing")
            }
            Commission::Onbekend => write!(f, "onbekend"),
        }
    }
}

fn parse_commission_type(raw: &str) -> Commission {
    let raw = raw.trim().to_lowercase();
    if raw.contains("binnenlandse") {
        Commission::BinnenlandseZakenVeiligheidMigratieEnBestuurszaken
    } else if raw.contains("justitie") {
        Commission::Justitie
    } else if raw.contains("gezondheid") {
        Commission::GezondheidEnGelijkeKansen
    } else if raw.contains("economie") {
        Commission::EconomieConsumentenBeschermingEnDigitalisering
    } else if raw.contains("buitenlandse") {
        Commission::BuitenlandseBetrekkingen
    } else if raw.contains("mobiliteit") {
        Commission::MobiliteitOverheidsbedrijvenEnFederaleInstellingen
    } else if raw.contains("landsverdediging") {
        Commission::Landsverdediging
    } else if raw.contains("energie") {
        Commission::EnergieLeefmilieuEnKlimaat
    } else if raw.contains("sociale") {
        Commission::SocialeZakenWerkEnPensioenen
    } else if raw.contains("begroting") {
        Commission::FinancienEnBegroting
    } else if raw.contains("klimaatdialoog") {
        Commission::InterparlementaireKlimaatdialoog
    } else if raw.contains("grondwet") {
        Commission::GrondwetEnInstitutioneleVernieuwing
    } else {
        Commission::Onbekend
    }
}

macro_rules! col {
    ($rows:expr, $f:expr) => {
        Arc::new(StringArray::from($rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
    };
}

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

fn write_meetings(path: &Path, rows: &[ScrapedMeeting]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("date", DataType::Utf8, false),
        Field::new("time_of_day", DataType::Utf8, false),
        Field::new("start_time", DataType::Utf8, false),
        Field::new("end_time", DataType::Utf8, false),
        Field::new("commission", DataType::Utf8, false),
        Field::new("chair", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |c| c.session_id.to_string()),
            col!(rows, |c| c.meeting_id.to_string()),
            col!(rows, |c| c.date.clone()),
            col!(rows, |c| c.time_of_day.clone()),
            col!(rows, |c| c.start_time.clone()),
            col!(rows, |c| c.end_time.clone()),
            col!(rows, |c| c.commission.clone()),
            col!(rows, |c| c.chair.clone()),
            col!(rows, |c| c.source_url.clone()),
            col!(rows, |c| c.cache_path.clone()),
        ],
    )
}

fn write_questions(path: &Path, rows: &[ScrapedQuestion]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("question_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("questioners", DataType::Utf8, false),
        Field::new("respondents", DataType::Utf8, false),
        Field::new("topics_nl", DataType::Utf8, false),
        Field::new("topics_fr", DataType::Utf8, false),
        Field::new("internal_ids", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |q| q.question_id.clone()),
            col!(rows, |q| q.session_id.to_string()),
            col!(rows, |q| q.meeting_id.to_string()),
            col!(rows, |q| q.questioners.clone()),
            col!(rows, |q| q.respondents.clone()),
            col!(rows, |q| q.topics_nl.clone()),
            col!(rows, |q| q.topics_fr.clone()),
            col!(rows, |q| q.internal_ids.clone()),
            col!(rows, |q| q.source_url.clone()),
            col!(rows, |q| q.cache_path.clone()),
        ],
    )
}

fn write_gaps(path: &Path, gaps: &[MeetingGap]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("reason", DataType::Utf8, false),
        Field::new("detail", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(gaps, |g| g.meeting_id.to_string()),
            col!(gaps, |g| g.reason.clone()),
            col!(gaps, |g| g.detail.clone()),
        ],
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let client = ScrapingClient::new();
    let session_id: u32 = 56;

    let session_dir = data_dir()
        .join("sessions")
        .join(session_id.to_string())
        .join("commission");
    fs::create_dir_all(&session_dir).await?;

    let meeting_id_path = data_dir().join("current_commission_id.txt");
    let current_meeting_id: u32 = std::fs::read_to_string(&meeting_id_path)?.trim().parse()?;

    let mut web_request_count = 0u32;
    let mut gaps = std::collections::BTreeMap::new();

    eprintln!(
        "[meetings-commission] fetching new reports after meeting {current_meeting_id}…"
    );
    let last_meeting_id = fetch_new_meetings(
        &client,
        session_id,
        current_meeting_id,
        &mut web_request_count,
        &mut gaps,
    )
    .await?;

    if last_meeting_id == current_meeting_id {
        println!("[meetings-commission] no new meeting available to download");
    } else {
        println!(
            "[meetings-commission] fetched new meetings up to {}",
            last_meeting_id
        );
    }

    let mut all_meetings = Vec::new();
    let mut all_questions = Vec::new();
    let mut all_utterances = Vec::new();

    let mp = MultiProgress::new();
    let meetings_pb = mp.add(ProgressBar::new(last_meeting_id as u64));
    meetings_pb.set_style(
        ProgressStyle::with_template(
            "[meetings-commission] [{elapsed_precise}] {spinner:.blue} {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}",
        )?
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );

    meetings_pb.set_message("parsing".to_string());

    for meeting_id in 1..=last_meeting_id {
        if gaps
            .get(&meeting_id)
            .is_some_and(|g| g.reason == "not_found")
        {
            meetings_pb.inc(1);
            continue;
        }

        match parse_meeting(session_id, meeting_id) {
            Ok(output) => {
                all_meetings.push(output.meeting);
                all_questions.extend(output.questions);
                all_utterances.extend(output.utterances);
            }
            Err(err) => {
                record_gap(
                    &mut gaps,
                    MeetingGap {
                        meeting_id,
                        reason: "parse_failed".to_string(),
                        detail: err.to_string(),
                    },
                );
            }
        }

        meetings_pb.inc(1);
    }

    meetings_pb.finish_with_message("done");

    std::fs::write(&meeting_id_path, last_meeting_id.to_string())?;

    write_meetings(&session_dir.join("meetings.parquet"), &all_meetings)?;
    write_questions(&session_dir.join("questions.parquet"), &all_questions)?;
    write_utterances_parquet(&session_dir.join("utterances.parquet"), &all_utterances)?;

    let gap_rows: Vec<MeetingGap> = gaps.into_values().collect();
    write_gaps(&session_dir.join("meeting_gaps.parquet"), &gap_rows)?;

    println!(
        "[meetings-commission] scraped {} meetings using {} web requests ({} gaps recorded)",
        all_meetings.len(),
        web_request_count,
        gap_rows.len(),
    );
    if !gap_rows.is_empty() {
        let preview: Vec<String> = gap_rows
            .iter()
            .take(10)
            .map(|g| format!("{} ({})", g.meeting_id, g.reason))
            .collect();
        println!(
            "[meetings-commission] gaps: {}{}",
            preview.join(", "),
            if gap_rows.len() > 10 {
                format!(" … +{} more", gap_rows.len() - 10)
            } else {
                String::new()
            }
        );
    }
    Ok(())
}

async fn fetch_new_meetings(
    client: &ScrapingClient,
    session_id: u32,
    current_id: u32,
    web_request_count: &mut u32,
    gaps: &mut std::collections::BTreeMap<u32, MeetingGap>,
) -> Result<u32, Box<dyn Error>> {
    let mut last = current_id;
    let mut consecutive_misses = 0u32;
    let mut probe = current_id + 1;

    while consecutive_misses < 2 {
        match download_meeting(client, session_id, probe, web_request_count).await? {
            DownloadOutcome::Saved => {
                last = probe;
                consecutive_misses = 0;
                eprintln!("[meetings-commission] ic{probe:03} → downloaded (last={last})");
            }
            DownloadOutcome::AlreadyCached => {
                last = probe;
                consecutive_misses = 0;
                eprintln!("[meetings-commission] ic{probe:03} → cached (last={last})");
            }
            DownloadOutcome::NotFound => {
                record_gap(
                    gaps,
                    MeetingGap {
                        meeting_id: probe,
                        reason: "not_found".to_string(),
                        detail: "HTTP 404".to_string(),
                    },
                );
                consecutive_misses += 1;
                eprintln!(
                    "[meetings-commission] ic{probe:03} → 404 ({consecutive_misses}/2 consecutive misses)"
                );
            }
        }
        probe += 1;
    }

    Ok(last)
}

enum DownloadOutcome {
    Saved,
    AlreadyCached,
    NotFound,
}

async fn download_meeting(
    client: &ScrapingClient,
    session_id: u32,
    meeting_id: u32,
    web_request_count: &mut u32,
) -> Result<DownloadOutcome, Box<dyn Error>> {
    let filepath = cache_dir().join(format!(
        "sessions/{}/meetings/commission/{}-{}.html",
        session_id, session_id, meeting_id
    ));
    if filepath.exists() {
        return Ok(DownloadOutcome::AlreadyCached);
    }

    let url = format!(
        "https://www.dekamer.be/doc/CCRI/html/{}/ic{:03}x.html",
        session_id, meeting_id
    );
    let response = client.get(&url).await?;
    *web_request_count += 1;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(DownloadOutcome::NotFound);
    }
    let raw_bytes = response.bytes().await?;
    let (decoded_str, _, _) = WINDOWS_1252.decode(&raw_bytes);
    if let Some(parent) = filepath.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&filepath, decoded_str.as_ref())?;
    Ok(DownloadOutcome::Saved)
}

fn parse_meeting(session_id: u32, meeting_id: u32) -> Result<MeetingOutput, Box<dyn Error>> {
    let filepath = cache_dir().join(format!(
        "sessions/{}/meetings/commission/{}-{}.html",
        session_id, session_id, meeting_id
    ));
    let url = format!(
        "https://www.dekamer.be/doc/CCRI/html/{}/ic{:03}x.html",
        session_id, meeting_id
    );

    if !filepath.exists() {
        return Err(format!("meeting {meeting_id} cache missing").into());
    }

    let cache_path = relative_cache_path(&filepath, &cache_dir());
    let content = read_report_html(&filepath)?;
    let document = Html::parse_document(&content);

    let date = extract_date_from_document(&document)?;
    let time_of_day = extract_time_of_day_from_document(&document)?;
    let start_time = extract_start_time_from_document(&document)?;
    let end_time = extract_end_time_from_document(&document)?;
    let chair = extract_chair_from_document(&document)?;
    let commission = extract_commission_from_document(&document)?;

    let questions = extract_questions(
        &document,
        session_id,
        meeting_id,
        &url,
        &cache_path,
    )?;

    let utterances = extract_utterances_from_document(
        &document,
        MeetingKind::Commission,
        session_id,
        meeting_id,
        &url,
        &cache_path,
    );

    Ok(MeetingOutput {
        meeting: ScrapedMeeting {
            session_id,
            meeting_id,
            date,
            time_of_day,
            start_time,
            end_time,
            commission,
            chair,
            source_url: url,
            cache_path,
        },
        questions,
        utterances,
    })
}

fn extract_questions(
    document: &Html,
    session_id: u32,
    meeting_id: u32,
    source_url: &str,
    cache_path: &str,
) -> Result<Vec<ScrapedQuestion>, Box<dyn Error>> {
    let mut questions = Vec::new();
    let mut previous_nl = String::new();
    let mut previous_fr = String::new();
    let mut question_seq: i32 = 0;

    // Commission reports always contain questions from the start; no section header needed.
    let french_indicators = ["questions jointes", "question de"];
    let dutch_indicators = ["samengevoegde vragen", "toegevoegde vragen", "vraag van"];

    let flush = |seq: i32, nl: &str, fr: &str| -> Result<Option<ScrapedQuestion>, Box<dyn Error>> {
        if nl.is_empty() && fr.is_empty() {
            return Ok(None);
        }
        let data_nl = extract_question_data(nl)?;
        let data_fr = extract_question_data(fr)?;
        Ok(Some(ScrapedQuestion {
            question_id: composite_scoped_id(session_id, "commission", meeting_id, seq),
            session_id,
            meeting_id,
            questioners: data_nl.questioners.join(","),
            respondents: data_nl.respondents.join(","),
            topics_nl: data_nl.topics.join(";"),
            topics_fr: data_fr.topics.join(";"),
            internal_ids: data_nl.internal_ids.join(","),
            source_url: source_url.to_string(),
            cache_path: cache_path.to_string(),
        }))
    };

    for element in document.select(selector_h2_or_p()) {
        let tag = element.value().name();

        if tag == "h2" {
            let mut found_nl: Option<String> = None;
            let mut found_fr: Option<String> = None;

            // Dutch spans — swap to FR if they look French.
            if let Some(span) = element
                .select(selector_span())
                .filter(|s| matches!(s.value().attr("lang"), Some("NL") | Some("NL-BE")))
                .last()
            {
                let text =
                    clean_text(&span.text().collect::<Vec<_>>().join(" ")).replace("\"", "'");
                if french_indicators
                    .iter()
                    .any(|w| text.to_lowercase().contains(w))
                {
                    found_fr = Some(text);
                } else {
                    found_nl = Some(text);
                }
            }

            // French spans — swap to NL if they look Dutch.
            if let Some(span) = element
                .select(selector_span())
                .filter(|s| s.value().attr("lang") == Some("FR"))
                .last()
            {
                let text =
                    clean_text(&span.text().collect::<Vec<_>>().join(" ")).replace("\"", "'");
                if dutch_indicators
                    .iter()
                    .any(|w| text.to_lowercase().contains(w))
                {
                    found_nl = Some(text);
                } else {
                    found_fr = Some(text);
                }
            }

            let is_hearing = found_nl
                .as_deref()
                .map_or(false, |t| t.to_lowercase().contains("hoorzitting"))
                || found_fr
                    .as_deref()
                    .map_or(false, |t| t.to_lowercase().contains("audition"));

            if is_hearing {
                // Flush any pending question that came before this hearing,
                // then reset state so the hearing's discussion doesn't bleed in.
                if !previous_nl.is_empty() && !previous_fr.is_empty() {
                    if let Some(q) = flush(question_seq, &previous_nl, &previous_fr)? {
                        questions.push(q);
                        question_seq += 1;
                    }
                }
                previous_nl.clear();
                previous_fr.clear();
                continue;
            }

            // NOTE: ic017x.html uses "toegevoegde vragen" instead of "Samengevoegde" for grouped questions.
            let is_group_start = found_nl.as_deref().map_or(false, |t| {
                t.starts_with("Samengevoegde") || t.contains("toegevoegde vragen")
            }) || found_fr.as_deref().map_or(false, |t| t.contains("jointes"));
            let is_subquestion = found_nl.as_deref().map_or(false, |t| t.starts_with("-"))
                || found_fr.as_deref().map_or(false, |t| t.starts_with("-"));
            let is_single = found_nl
                .as_deref()
                .map_or(false, |t| t.starts_with("Vraag van"))
                || found_fr
                    .as_deref()
                    .map_or(false, |t| t.starts_with("Question de"));

            if is_group_start || is_single {
                if !previous_nl.is_empty() && !previous_fr.is_empty() {
                    if let Some(q) = flush(question_seq, &previous_nl, &previous_fr)? {
                        questions.push(q);
                        question_seq += 1;
                    }
                    previous_nl.clear();
                    previous_fr.clear();
                }
                if let Some(t) = found_nl {
                    previous_nl = t;
                }
                if let Some(t) = found_fr {
                    previous_fr = t;
                }
            } else if is_subquestion {
                if let Some(t) = found_nl {
                    previous_nl.push('\n');
                    previous_nl.push_str(&t);
                }
                if let Some(t) = found_fr {
                    previous_fr.push('\n');
                    previous_fr.push_str(&t);
                }
            }
        }

        if tag == "p" {
            let _text = element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
        }
    }

    // Flush the last question.
    if !previous_nl.is_empty() && !previous_fr.is_empty() {
        if let Some(q) = flush(question_seq, &previous_nl, &previous_fr)? {
            questions.push(q);
        }
    }

    Ok(questions)
}

fn extract_question_data(question_text: &str) -> Result<QuestionData, Box<dyn Error>> {
    let mut questioners = Vec::new();
    let mut topics = Vec::new();
    let mut respondents = Vec::new();
    let mut internal_ids = Vec::new();

    for capture in question_regex().captures_iter(question_text) {
        let Some(questioner) = normalize_questioner_name(&capture[1]) else {
            continue;
        };
        let respondent = capture
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| "Onbekend".to_string());
        let topic = capture[3].trim().to_string();
        let internal_id = format!("Q{}", capture[4].trim());

        questioners.push(questioner);
        if !respondents.contains(&respondent) {
            respondents.push(respondent);
        }
        internal_ids.push(internal_id);
        topics.push(topic);
    }

    Ok(QuestionData {
        questioners,
        respondents,
        topics,
        internal_ids,
    })
}

/// Strip scrape artefacts from a captured questioner field.
fn normalize_questioner_name(raw: &str) -> Option<String> {
    let mut name = raw.trim().trim_start_matches('-').trim().to_string();
    if name.is_empty() {
        return None;
    }

    static QUESTION_PREFIX: OnceLock<Regex> = OnceLock::new();
    let prefix = QUESTION_PREFIX.get_or_init(|| {
        Regex::new(r"(?i)^(?:vraag van|question de)\s+").unwrap()
    });
    name = prefix.replace(&name, "").trim().to_string();
    if name.is_empty() {
        return None;
    }

    name = name.replace("- ", "").replace("de heer ", "");
    name = name.trim().trim_end_matches('-').trim().to_string();
    if name.is_empty() {
        return None;
    }

    Some(name)
}

fn extract_date_from_document(document: &Html) -> Result<String, Box<dyn Error>> {
    let first_table = document
        .select(selector_table())
        .next()
        .ok_or("No table found")?;

    let text: String = first_table
        .select(selector_span())
        .flat_map(|s| s.text().map(str::to_owned))
        .collect::<Vec<_>>()
        .join(" ");

    let caps = date_regex()
        .captures(&text)
        .ok_or("Could not find date in document")?;

    let day = format!("{:02}", caps[1].parse::<u8>()?);
    let month = match &caps[2].to_lowercase()[..] {
        "januari" => "01",
        "februari" => "02",
        "maart" => "03",
        "april" => "04",
        "mei" => "05",
        "juni" => "06",
        "juli" => "07",
        "augustus" => "08",
        "september" => "09",
        "oktober" => "10",
        "november" => "11",
        "december" => "12",
        _ => return Err("Invalid month name".into()),
    };
    Ok(format!("{}-{}-{}", &caps[3], month, day))
}

fn extract_time_of_day_from_document(document: &Html) -> Result<String, Box<dyn Error>> {
    for span in document.select(selector_span()) {
        let text = span
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_lowercase();
        match text.as_str() {
            "namiddag" => return Ok("afternoon".to_string()),
            "voormiddag" => return Ok("morning".to_string()),
            "avond" => return Ok("evening".to_string()),
            _ => {}
        }
    }
    Err("Could not extract time of day from the document".into())
}

fn extract_start_time_from_document(document: &Html) -> Result<String, Box<dyn Error>> {
    // NOTE: Commission 270 used "14:13 uur" (colon) instead of the usual "14.13 uur" (dot).
    let keywords = [
        "De behandeling van de",
        "De behandeling van de vragen en de interpellatie vangt aan om",
        "De behandeling van de vragen en interpellaties vangt aan",
        "De behandeling van de vragen en van de interpellatie vangt aan om",
        "De openbare commissievergadering wordt geopend",
        "De vergadering wordt geopend",
        "De behandeling van de vragen vangt aan",
        "De gedachtewisseling vangt aan",
        "De behandeling van de interpellatie vangt",
    ];
    extract_time_from_document(document, &keywords)
        .ok_or_else(|| "Could not extract start time from the document".into())
}

fn extract_end_time_from_document(document: &Html) -> Result<String, Box<dyn Error>> {
    let keywords = [
        "De openbare commissievergadering wordt gesloten",
        "De gedachtewisseling met de ministers eindigt",
        "De behandeling van de vragen eindigt",
        "De gedachtewisseling eindigt",
        "De vergadering wordt gesloten",
        "De behandeling van de interpellatie eindigt",
        "De behandeling van de interpellaties eindigt",
        "De behandeling van de vragen en interpellaties eindigt om",
    ];
    extract_time_from_document(document, &keywords)
        .ok_or_else(|| "Could not extract end time from the document".into())
}

fn extract_time_from_document(document: &Html, keywords: &[&str]) -> Option<String> {
    let mut last_time: Option<String> = None;

    for node in document.select(selector_span_p()) {
        let text = node.text().collect::<Vec<_>>().join(" ").replace('\n', " ");
        if keywords.iter().any(|&kw| text.contains(kw)) {
            if let Some(caps) = time_regex().captures(&text) {
                last_time = Some(format!("{}h{}", &caps[1], &caps[2]));
            }
        }
    }
    last_time
}

fn extract_chair_from_document(document: &Html) -> Result<String, Box<dyn Error>> {
    for node in document.select(selector_span_p()) {
        let text = node.text().collect::<Vec<_>>().join(" ").replace('\n', " ");
        if let Some(caps) = chair_regex().captures(&text) {
            let chunk = caps[1].replace('\u{00A0}', " ").trim().to_string();
            let names: Vec<String> = chunk
                .split(" en ")
                .filter_map(|part| {
                    let clean = chair_titles_regex()
                        .replace_all(part.trim(), "")
                        .trim()
                        .to_string();
                    if clean.is_empty() { None } else { Some(clean) }
                })
                .collect();
            if !names.is_empty() {
                return Ok(names.join(", "));
            }
        }
    }
    Err("Could not extract chair".into())
}

fn extract_commission_from_document(document: &Html) -> Result<String, Box<dyn Error>> {
    let first_table = document
        .select(selector_table())
        .next()
        .ok_or("No table found")?;

    let raw = first_table
        .select(selector_span())
        .next()
        .ok_or("No span in first table")?
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    Ok(parse_commission_type(&raw).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_skips_gap_and_continues() {
        let exists = |id: u32| id != 67 && id <= 70;
        let (last, missing) = discover_last_from_probes(66, 2, exists);
        assert_eq!(last, 70);
        assert_eq!(missing, vec![67, 71, 72]);
    }

    #[test]
    fn discover_stops_after_two_consecutive_misses() {
        let exists = |id: u32| id == 68;
        let (last, missing) = discover_last_from_probes(66, 2, exists);
        assert_eq!(last, 68);
        assert_eq!(missing, vec![67, 69, 70]);
    }

    #[test]
    fn normalize_questioner_strips_subquestion_header() {
        assert_eq!(
            normalize_questioner_name("-Vraag van Xavier Dubois"),
            Some("Xavier Dubois".to_string())
        );
        assert_eq!(
            normalize_questioner_name("-Natalie Eggermont"),
            Some("Natalie Eggermont".to_string())
        );
        assert_eq!(
            normalize_questioner_name("Question de François De Smet"),
            Some("François De Smet".to_string())
        );
    }

    #[test]
    fn extract_question_data_parses_merged_subquestion_block() {
        let text = "-Vraag van Xavier Dubois aan Bernard Quintin (Veiligheid) over \"test topic\" (56001234C)";
        let data = extract_question_data(text).unwrap();
        assert_eq!(data.questioners, vec!["Xavier Dubois".to_string()]);
        assert_eq!(data.respondents, vec!["Bernard Quintin".to_string()]);
    }
}
