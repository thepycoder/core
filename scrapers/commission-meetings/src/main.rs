use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::upsert_gap;
use crawl::utils::{max_cached_meeting_id, relative_cache_path};
use crawl::{
    AgendaItemDraft, AnswerDraft, BundlePublisher, GAP_REASON_NOT_FOUND,
    GAP_REASON_UNSUPPORTED_FORMAT, HearingDraft, InterpellationDraft, MANIFEST_STATUS_PARSED,
    MeetingGapRow, MeetingKind, OralQuestionDraft, ReportBlockRow, SourceManifestRow,
    SourceSpanDraft, UtteranceDraft, content_hash, content_hash_bytes, extract_questions_from_agenda,
    gap_reason_to_manifest_status, load_prior_gaps, looks_like_pdf, manifest_path,
    materialize_agenda_items, now_rfc3339, parse_commission_meeting_report, read_cache_metadata,
    read_report_html, reconcile_meeting_coverage, record_gap, validate_manifest_rows,
    write_agenda_items_parquet, write_answers_parquet, write_cache_artifact, write_hearings_parquet,
    write_interpellations_parquet, write_meeting_gaps_parquet, write_report_blocks_parquet,
    write_source_manifest, write_source_spans_parquet, write_utterances_parquet,
};
use encoding_rs::WINDOWS_1252;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parquet::arrow::ArrowWriter;
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tokio::fs;

/// REGEXES
static TIME_REGEX: OnceLock<Regex> = OnceLock::new();
static DATE_REGEX: OnceLock<Regex> = OnceLock::new();
static CHAIR_TITLES_REGEX: OnceLock<Regex> = OnceLock::new();
static CHAIR_REGEX: OnceLock<Regex> = OnceLock::new();

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

fn selector_span() -> &'static Selector {
    SELECTOR_SPAN.get_or_init(|| Selector::parse("span").unwrap())
}
fn selector_span_p() -> &'static Selector {
    SELECTOR_SPAN_P.get_or_init(|| Selector::parse("span, p").unwrap())
}
fn selector_table() -> &'static Selector {
    SELECTOR_TABLE.get_or_init(|| Selector::parse("table").unwrap())
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
    question_body_nl: String,
    question_body_fr: String,
    treatment_mode: String,
    source_url: String,
    cache_path: String,
}

struct MeetingOutput {
    meeting: ScrapedMeeting,
    questions: Vec<ScrapedQuestion>,
    agenda_items: Vec<AgendaItemDraft>,
    hearings: Vec<HearingDraft>,
    interpellations: Vec<InterpellationDraft>,
    utterances: Vec<UtteranceDraft>,
    answers: Vec<AnswerDraft>,
    report_blocks: Vec<ReportBlockRow>,
    source_spans: Vec<SourceSpanDraft>,
}

const MEETING_KIND: &str = "commission";
const SOURCE_NAME: &str = "commission_meetings";

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
        Field::new("question_body_nl", DataType::Utf8, false),
        Field::new("question_body_fr", DataType::Utf8, false),
        Field::new("treatment_mode", DataType::Utf8, false),
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
            col!(rows, |q| q.question_body_nl.clone()),
            col!(rows, |q| q.question_body_fr.clone()),
            col!(rows, |q| q.treatment_mode.clone()),
            col!(rows, |q| q.source_url.clone()),
            col!(rows, |q| q.cache_path.clone()),
        ],
    )
}

fn meeting_url(session_id: u32, meeting_id: u32) -> String {
    format!(
        "https://www.dekamer.be/doc/CCRI/html/{}/ic{:03}x.html",
        session_id, meeting_id
    )
}

fn meeting_cache_file(session_id: u32, meeting_id: u32) -> std::path::PathBuf {
    cache_dir().join(format!(
        "sessions/{}/meetings/commission/{}-{}.html",
        session_id, session_id, meeting_id
    ))
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
    let gaps_path = session_dir.join("meeting_gaps.parquet");
    let mut gaps = load_prior_gaps(&gaps_path, session_id, MEETING_KIND)?;

    eprintln!("[meetings-commission] fetching new reports after meeting {current_meeting_id}…");
    let last_meeting_id = if cache_only() {
        max_cached_meeting_id(session_id, "commission").unwrap_or(current_meeting_id)
    } else {
        fetch_new_meetings(
            &client,
            session_id,
            current_meeting_id,
            &mut web_request_count,
            &mut gaps,
        )
        .await?
    };

    if cache_only() {
        println!("[meetings-commission] cache-only: parsing meetings 1..={last_meeting_id}");
    } else if last_meeting_id == current_meeting_id {
        println!("[meetings-commission] no new meeting available to download");
    } else {
        println!(
            "[meetings-commission] fetched new meetings up to {}",
            last_meeting_id
        );
    }

    let mut all_meetings = Vec::new();
    let mut all_questions = Vec::new();
    let mut all_agenda_items = Vec::new();
    let mut all_hearings = Vec::new();
    let mut all_interpellations = Vec::new();
    let mut all_utterances = Vec::new();
    let mut all_answers = Vec::new();
    let mut all_report_blocks = Vec::new();
    let mut all_source_spans = Vec::new();
    let mut parsed_ids = BTreeSet::new();
    let mut manifest_rows = Vec::new();
    let run_mode = if cache_only() {
        SourceManifestRow::run_mode_cache_only()
    } else {
        SourceManifestRow::run_mode_live()
    };

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
        let filepath = meeting_cache_file(session_id, meeting_id);
        let url = meeting_url(session_id, meeting_id);
        let rel_cache = relative_cache_path(&filepath, &cache_dir());

        if let Some(gap) = gaps.get(&meeting_id) {
            if gap.reason == GAP_REASON_NOT_FOUND {
                push_gap_manifest(&mut manifest_rows, session_id, meeting_id, gap, run_mode);
                meetings_pb.inc(1);
                continue;
            }
        }

        if filepath.exists() {
            let raw = std::fs::read(&filepath)?;
            if looks_like_pdf(&raw) {
                let meta = read_cache_metadata(&filepath)?;
                let hash = meta
                    .as_ref()
                    .map(|m| m.content_hash.clone())
                    .unwrap_or_else(|| content_hash_bytes(&raw));
                let now = now_rfc3339();
                let gap = MeetingGapRow::new(
                    session_id,
                    MEETING_KIND,
                    meeting_id,
                    GAP_REASON_UNSUPPORTED_FORMAT,
                    "PDF report (unsupported HTML meeting format)",
                    &url,
                    &rel_cache,
                )
                .with_hashes(hash.clone())
                .with_timestamps(
                    meta.as_ref()
                        .map(|m| m.fetched_at.as_str())
                        .unwrap_or(now.as_str()),
                    meta.as_ref()
                        .map(|m| m.checked_at.as_str())
                        .unwrap_or(now.as_str()),
                );
                upsert_gap(&mut gaps, gap.clone());
                push_gap_manifest(&mut manifest_rows, session_id, meeting_id, &gap, run_mode);
                meetings_pb.inc(1);
                continue;
            }
        } else if gaps
            .get(&meeting_id)
            .is_some_and(|g| g.reason == GAP_REASON_UNSUPPORTED_FORMAT)
        {
            push_gap_manifest(
                &mut manifest_rows,
                session_id,
                meeting_id,
                gaps.get(&meeting_id).unwrap(),
                run_mode,
            );
            meetings_pb.inc(1);
            continue;
        } else if !filepath.exists() {
            return Err(format!(
                "incomplete snapshot: commission meeting {meeting_id} cache missing at {} — aborting to preserve prior outputs",
                filepath.display()
            )
            .into());
        }

        match parse_meeting(session_id, meeting_id) {
            Ok(output) => {
                let meta = read_cache_metadata(&filepath)?;
                let content_hash = meta
                    .as_ref()
                    .map(|m| m.content_hash.clone())
                    .unwrap_or_else(|| {
                        std::fs::read(&filepath)
                            .map(|b| content_hash_bytes(&b))
                            .unwrap_or_default()
                    });
                let fetched_at = meta
                    .as_ref()
                    .map(|m| m.fetched_at.clone())
                    .unwrap_or_default();
                let checked_at = meta
                    .as_ref()
                    .map(|m| m.checked_at.clone())
                    .unwrap_or_default();
                let content_type = meta
                    .as_ref()
                    .map(|m| m.content_type.clone())
                    .unwrap_or_else(|| "text/html".to_string());

                manifest_rows.push(SourceManifestRow {
                    source: SOURCE_NAME.into(),
                    session_id: session_id.to_string(),
                    item_kind: "meeting".into(),
                    native_item_id: meeting_id.to_string(),
                    source_url: url,
                    cache_path: rel_cache,
                    status: MANIFEST_STATUS_PARSED.into(),
                    row_count: 1,
                    content_type,
                    content_hash,
                    fetched_at,
                    checked_at,
                    run_mode: run_mode.into(),
                    detail: String::new(),
                });

                parsed_ids.insert(meeting_id);
                gaps.remove(&meeting_id);
                all_meetings.push(output.meeting);
                all_questions.extend(output.questions);
                all_agenda_items.extend(output.agenda_items);
                all_hearings.extend(output.hearings);
                all_interpellations.extend(output.interpellations);
                all_utterances.extend(output.utterances);
                all_answers.extend(output.answers);
                all_report_blocks.extend(output.report_blocks);
                all_source_spans.extend(output.source_spans);
            }
            Err(err) => {
                return Err(format!(
                    "commission meeting {meeting_id} failed parser invariants ({err}) — aborting; not publishing a partial snapshot"
                )
                .into());
            }
        }

        meetings_pb.inc(1);
    }

    meetings_pb.finish_with_message("done");

    // Drop gaps beyond the discovery boundary.
    gaps.retain(|id, _| *id <= last_meeting_id);
    reconcile_meeting_coverage(last_meeting_id, &parsed_ids, &gaps)?;

    let gap_rows: Vec<MeetingGapRow> = gaps.values().cloned().collect();
    validate_manifest_rows(&manifest_rows)?;

    let derived_dir = data_dir().join(format!("derived/sessions/{session_id}/commission"));
    std::fs::create_dir_all(&derived_dir)?;
    let manifest_final = manifest_path(SOURCE_NAME);

    let mut bundle = BundlePublisher::new("commission-meetings", &data_dir())?;
    let stage_meetings = bundle.stage_path(&session_dir.join("meetings.parquet"))?;
    let stage_questions = bundle.stage_path(&session_dir.join("questions.parquet"))?;
    let stage_agenda_items = bundle.stage_path(&session_dir.join("agenda_items.parquet"))?;
    let stage_hearings = bundle.stage_path(&session_dir.join("hearings.parquet"))?;
    let stage_interpellations = bundle.stage_path(&session_dir.join("interpellations.parquet"))?;
    let stage_utterances = bundle.stage_path(&session_dir.join("utterances.parquet"))?;
    let stage_answers = bundle.stage_path(&session_dir.join("answers.parquet"))?;
    let stage_gaps = bundle.stage_path(&gaps_path)?;
    let stage_blocks = bundle.stage_path(&derived_dir.join("report_blocks.parquet"))?;
    let stage_spans = bundle.stage_path(&derived_dir.join("source_spans.parquet"))?;
    let stage_manifest = bundle.stage_path(&manifest_final)?;
    let stage_checkpoint = bundle.stage_path(&meeting_id_path)?;

    write_meetings(&stage_meetings, &all_meetings)?;
    write_questions(&stage_questions, &all_questions)?;
    write_agenda_items_parquet(&stage_agenda_items, &all_agenda_items)?;
    write_hearings_parquet(&stage_hearings, &all_hearings)?;
    write_interpellations_parquet(&stage_interpellations, &all_interpellations)?;
    write_utterances_parquet(&stage_utterances, &all_utterances)?;
    write_answers_parquet(&stage_answers, &all_answers)?;
    write_meeting_gaps_parquet(&stage_gaps, &gap_rows)?;
    write_report_blocks_parquet(&stage_blocks, &all_report_blocks)?;
    write_source_spans_parquet(&stage_spans, &all_source_spans)?;
    write_source_manifest(&stage_manifest, &manifest_rows)?;
    std::fs::write(&stage_checkpoint, last_meeting_id.to_string())?;

    bundle.commit()?;

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

fn push_gap_manifest(
    rows: &mut Vec<SourceManifestRow>,
    session_id: u32,
    meeting_id: u32,
    gap: &MeetingGapRow,
    run_mode: &str,
) {
    rows.push(SourceManifestRow {
        source: SOURCE_NAME.into(),
        session_id: session_id.to_string(),
        item_kind: "meeting".into(),
        native_item_id: meeting_id.to_string(),
        source_url: gap.source_url.clone(),
        cache_path: gap.cache_path.clone(),
        status: gap_reason_to_manifest_status(&gap.reason).into(),
        row_count: 0,
        content_type: if gap.reason == GAP_REASON_UNSUPPORTED_FORMAT {
            "application/pdf".into()
        } else {
            String::new()
        },
        content_hash: gap.content_hash.clone(),
        fetched_at: gap.fetched_at.clone(),
        checked_at: gap.checked_at.clone(),
        run_mode: run_mode.into(),
        detail: gap.detail.clone(),
    });
}

async fn fetch_new_meetings(
    client: &ScrapingClient,
    session_id: u32,
    current_id: u32,
    web_request_count: &mut u32,
    gaps: &mut BTreeMap<u32, MeetingGapRow>,
) -> Result<u32, Box<dyn Error>> {
    let mut last = current_id;
    let mut consecutive_misses = 0u32;
    let mut probe = current_id + 1;
    let mut missing_streak: Vec<u32> = Vec::new();

    while consecutive_misses < 2 {
        match download_meeting(client, session_id, probe, web_request_count).await? {
            DownloadOutcome::Saved { is_pdf } | DownloadOutcome::AlreadyCached { is_pdf } => {
                for miss in missing_streak.drain(..) {
                    let url = meeting_url(session_id, miss);
                    record_gap(
                        gaps,
                        MeetingGapRow::new(
                            session_id,
                            MEETING_KIND,
                            miss,
                            GAP_REASON_NOT_FOUND,
                            "HTTP 404",
                            url,
                            "",
                        )
                        .with_timestamps(now_rfc3339(), now_rfc3339()),
                    );
                }
                last = probe;
                consecutive_misses = 0;
                if is_pdf {
                    let filepath = meeting_cache_file(session_id, probe);
                    let rel = relative_cache_path(&filepath, &cache_dir());
                    let raw = std::fs::read(&filepath).unwrap_or_default();
                    let meta = read_cache_metadata(&filepath)?;
                    upsert_gap(
                        gaps,
                        MeetingGapRow::new(
                            session_id,
                            MEETING_KIND,
                            probe,
                            GAP_REASON_UNSUPPORTED_FORMAT,
                            "PDF report (unsupported HTML meeting format)",
                            meeting_url(session_id, probe),
                            rel,
                        )
                        .with_hashes(content_hash_bytes(&raw))
                        .with_timestamps(
                            meta.as_ref()
                                .map(|m| m.fetched_at.clone())
                                .unwrap_or_else(now_rfc3339),
                            meta.as_ref()
                                .map(|m| m.checked_at.clone())
                                .unwrap_or_else(now_rfc3339),
                        ),
                    );
                }
                eprintln!("[meetings-commission] ic{probe:03} → ok (last={last}, pdf={is_pdf})");
            }
            DownloadOutcome::NotFound => {
                missing_streak.push(probe);
                consecutive_misses += 1;
                eprintln!(
                    "[meetings-commission] ic{probe:03} → 404 ({consecutive_misses}/2 consecutive misses)"
                );
            }
        }
        probe += 1;
    }

    // Trailing misses in missing_streak are stop signals only — not gaps.
    Ok(last)
}

enum DownloadOutcome {
    Saved { is_pdf: bool },
    AlreadyCached { is_pdf: bool },
    NotFound,
}

async fn download_meeting(
    client: &ScrapingClient,
    session_id: u32,
    meeting_id: u32,
    web_request_count: &mut u32,
) -> Result<DownloadOutcome, Box<dyn Error>> {
    let filepath = meeting_cache_file(session_id, meeting_id);
    if filepath.exists() {
        let raw = std::fs::read(&filepath)?;
        return Ok(DownloadOutcome::AlreadyCached {
            is_pdf: looks_like_pdf(&raw),
        });
    }

    let url = meeting_url(session_id, meeting_id);
    let response = client.get(&url).await?;
    *web_request_count += 1;
    match crawl::classify_meeting_http_status(response.status()) {
        Ok(crawl::MeetingHttpOutcome::NotFound) => return Ok(DownloadOutcome::NotFound),
        Ok(crawl::MeetingHttpOutcome::Success) => {}
        Err(detail) => {
            return Err(format!("commission meeting {meeting_id}: {detail} for {url}").into());
        }
    }

    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let raw_bytes = response.bytes().await?;
    let is_pdf = looks_like_pdf(&raw_bytes)
        || content_type
            .to_ascii_lowercase()
            .contains("application/pdf");

    if is_pdf {
        write_cache_artifact(&filepath, &raw_bytes, &url, "application/pdf")?;
        return Ok(DownloadOutcome::Saved { is_pdf: true });
    }

    let (decoded_str, _, _) = WINDOWS_1252.decode(&raw_bytes);
    write_cache_artifact(
        &filepath,
        decoded_str.as_ref().as_bytes(),
        &url,
        "text/html; charset=windows-1252",
    )?;
    Ok(DownloadOutcome::Saved { is_pdf: false })
}

fn parse_meeting(session_id: u32, meeting_id: u32) -> Result<MeetingOutput, Box<dyn Error>> {
    let filepath = meeting_cache_file(session_id, meeting_id);
    let url = meeting_url(session_id, meeting_id);

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

    let parsed = parse_commission_meeting_report(
        &document,
        session_id,
        meeting_id,
        &url,
        &cache_path,
        &content_hash(&content),
    );

    let mut questions: Vec<ScrapedQuestion> = extract_questions_from_agenda(
        &parsed.agenda,
        MeetingKind::Commission,
        session_id,
        meeting_id,
        &HashMap::new(),
        &url,
        &cache_path,
    )?
    .into_iter()
    .map(scraped_question_from_draft)
    .collect();

    for item in &parsed.oral_written_items {
        if let Some(q) = questions
            .iter_mut()
            .find(|q| q.question_id == item.question_id)
        {
            q.question_body_nl = item.question_body_nl.clone();
            q.question_body_fr = item.question_body_fr.clone();
            q.treatment_mode = "oral_written".to_string();
        }
    }

    let agenda_items = materialize_agenda_items(
        &parsed.agenda,
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
        agenda_items,
        hearings: parsed.hearings,
        interpellations: parsed.interpellations,
        utterances: parsed.utterances,
        answers: parsed.answers,
        report_blocks: parsed.report_block_rows,
        source_spans: parsed.source_spans,
    })
}

fn scraped_question_from_draft(draft: OralQuestionDraft) -> ScrapedQuestion {
    ScrapedQuestion {
        question_id: draft.question_id,
        session_id: draft.session_id,
        meeting_id: draft.meeting_id,
        questioners: draft.questioners,
        respondents: draft.respondents,
        topics_nl: draft.topics_nl,
        topics_fr: draft.topics_fr,
        internal_ids: draft.internal_ids,
        question_body_nl: String::new(),
        question_body_fr: String::new(),
        treatment_mode: String::new(),
        source_url: draft.source_url,
        cache_path: draft.cache_path,
    }
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
    use crawl::discover_last_from_probes;

    #[test]
    fn discover_skips_gap_and_continues() {
        let exists = |id: u32| id != 67 && id <= 70;
        let result = discover_last_from_probes(66, 2, exists);
        assert_eq!(result.last_id, 70);
        assert_eq!(result.interior_missing, vec![67]);
    }

    #[test]
    fn discover_stops_after_two_consecutive_misses() {
        let exists = |id: u32| id == 68;
        let result = discover_last_from_probes(66, 2, exists);
        assert_eq!(result.last_id, 68);
        assert_eq!(result.interior_missing, vec![67]);
    }

    #[test]
    fn trailing_404_is_not_recorded_as_gap() {
        let exists = |_id: u32| false;
        let result = discover_last_from_probes(10, 2, exists);
        assert_eq!(result.last_id, 10);
        assert!(result.interior_missing.is_empty());
    }
}
