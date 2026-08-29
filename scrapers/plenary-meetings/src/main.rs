use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::report_blocks::read_report_html;
use crawl::upsert_gap;
use crawl::utils::{clean_text, composite_id, max_cached_meeting_id, relative_cache_path};
use crawl::{
    AgendaItemDraft, AnswerDraft, BundlePublisher, GAP_REASON_NOT_FOUND,
    GAP_REASON_UNSUPPORTED_FORMAT, HearingDraft, InterpellationDraft, MANIFEST_STATUS_PARSED,
    MeetingGapRow, MeetingKind, OralQuestionDraft, ReportBlock, ReportBlockRow, SourceManifestRow,
    SourceSpanDraft, UtteranceDraft, VoteAssemblyOutput, content_hash_bytes,
    extract_questions_from_agenda, gap_reason_to_manifest_status, load_prior_gaps, looks_like_pdf,
    manifest_path, materialize_agenda_items, now_rfc3339, parse_plenary_meeting_report,
    read_cache_metadata, reconcile_meeting_coverage, record_gap, validate_manifest_rows,
    write_agenda_items_parquet, write_answers_parquet, write_cache_artifact,
    write_hearings_parquet, write_interpellations_parquet, write_meeting_gaps_parquet,
    write_report_blocks_parquet, write_source_manifest, write_source_spans_parquet,
    write_utterances_parquet, write_vote_bundle,
};
use encoding_rs::WINDOWS_1252;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parquet::arrow::ArrowWriter;
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tokio::fs;

/// REGEXES
static TIME_REGEX: OnceLock<Regex> = OnceLock::new();
static DATE_REGEX: OnceLock<Regex> = OnceLock::new();
static DATE_NUMERIC_REGEX: OnceLock<Regex> = OnceLock::new();
static PROPOSITION_REGEX: OnceLock<Regex> = OnceLock::new();
static PROPOSITION_TOPIC_REGEX: OnceLock<Regex> = OnceLock::new();

fn time_regex() -> &'static Regex {
    TIME_REGEX.get_or_init(|| Regex::new(r"(\d{1,2})\.(\d{2})\s*(?:uur|u)").unwrap())
}

fn date_regex() -> &'static Regex {
    DATE_REGEX.get_or_init(|| Regex::new(r"(\d{1,2})\s+([a-zA-Z]+)\s+(\d{4})").unwrap())
}

fn date_numeric_regex() -> &'static Regex {
    DATE_NUMERIC_REGEX.get_or_init(|| Regex::new(r"(\d{1,2})-(\d{1,2})-(\d{4})").unwrap())
}
fn proposition_regex() -> &'static Regex {
    PROPOSITION_REGEX.get_or_init(|| Regex::new(r#"^(.+?)\s*\((\d+)\/(\d+(?:-\d+)?)\).*"#).unwrap())
    // PROPOSITION_REGEX.get_or_init(|| Regex::new(r#"^((?:Voorstel van resolutie|Proposition de résolution|Wetsvoorstel|Proposition de loi|Wetsontwerp|Projet de loi|Voorstel tot|Proposition visant|Voorstel van bijzondere wet).*)\((\d+)\/(\d+(?:-\d+)?)\).*$"#).unwrap())
}

fn proposition_topic_regex() -> &'static Regex {
    PROPOSITION_TOPIC_REGEX.get_or_init(|| Regex::new(r#"^([^(]*)"#).unwrap())
}

/// SELECTORS
static SELECTOR_SPAN: OnceLock<Selector> = OnceLock::new();
static SELECTOR_TABLE: OnceLock<Selector> = OnceLock::new();

fn selector_span() -> &'static Selector {
    SELECTOR_SPAN.get_or_init(|| Selector::parse("span").unwrap())
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
    source_url: String,
    cache_path: String,
}

struct ScrapedQuestion {
    question_id: String,
    session_id: u32,
    meeting_id: u32,
    questioners: String,
    questionees: String,
    respondents: String,
    topics_nl: String,
    topics_fr: String,
    internal_ids: String,
    question_body_nl: String,
    question_body_fr: String,
    treatment_mode: String,
    date: String,
    source_url: String,
    cache_path: String,
}

struct ScrapedProposition {
    proposition_id: String,
    session_id: u32,
    meeting_id: u32,
    title_nl: String,
    title_fr: String,
    dossier_id: String,
    document_id: String,
    date: String,
    source_url: String,
    cache_path: String,
}

struct ScrapedNotice {
    notice_id: String,
    session_id: u32,
    meeting_id: u32,
    title_nl: String,
    title_fr: String,
    date: String,
    source_url: String,
    cache_path: String,
}

struct MeetingOutput {
    meeting: ScrapedMeeting,
    questions: Vec<ScrapedQuestion>,
    propositions: Vec<ScrapedProposition>,
    notices: Vec<ScrapedNotice>,
    vote_bundle: VoteAssemblyOutput,
    report_blocks: Vec<ReportBlockRow>,
    source_spans: Vec<SourceSpanDraft>,
    agenda_items: Vec<AgendaItemDraft>,
    hearings: Vec<HearingDraft>,
    interpellations: Vec<InterpellationDraft>,
    utterances: Vec<UtteranceDraft>,
    answers: Vec<AnswerDraft>,
}

struct PropositionData {
    topic: String,
    dossier_id: Option<String>,
    document_id: Option<String>,
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
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |m| m.session_id.to_string()),
            col!(rows, |m| m.meeting_id.to_string()),
            col!(rows, |m| m.date.clone()),
            col!(rows, |m| m.time_of_day.clone()),
            col!(rows, |m| m.start_time.clone()),
            col!(rows, |m| m.end_time.clone()),
            col!(rows, |m| m.source_url.clone()),
            col!(rows, |m| m.cache_path.clone()),
        ],
    )
}

fn write_questions(path: &Path, rows: &[ScrapedQuestion]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("question_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("questioners", DataType::Utf8, false),
        Field::new("questionees", DataType::Utf8, false),
        Field::new("respondents", DataType::Utf8, false),
        Field::new("topics_nl", DataType::Utf8, false),
        Field::new("topics_fr", DataType::Utf8, false),
        Field::new("internal_ids", DataType::Utf8, false),
        Field::new("question_body_nl", DataType::Utf8, false),
        Field::new("question_body_fr", DataType::Utf8, false),
        Field::new("treatment_mode", DataType::Utf8, false),
        Field::new("date", DataType::Utf8, false),
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
            col!(rows, |q| q.questionees.clone()),
            col!(rows, |q| q.respondents.clone()),
            col!(rows, |q| q.topics_nl.clone()),
            col!(rows, |q| q.topics_fr.clone()),
            col!(rows, |q| q.internal_ids.clone()),
            col!(rows, |q| q.question_body_nl.clone()),
            col!(rows, |q| q.question_body_fr.clone()),
            col!(rows, |q| q.treatment_mode.clone()),
            col!(rows, |q| q.date.clone()),
            col!(rows, |q| q.source_url.clone()),
            col!(rows, |q| q.cache_path.clone()),
        ],
    )
}

fn write_propositions(path: &Path, rows: &[ScrapedProposition]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("proposition_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("title_nl", DataType::Utf8, false),
        Field::new("title_fr", DataType::Utf8, false),
        Field::new("dossier_id", DataType::Utf8, false),
        Field::new("document_id", DataType::Utf8, false),
        Field::new("date", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |p| p.proposition_id.clone()),
            col!(rows, |p| p.session_id.to_string()),
            col!(rows, |p| p.meeting_id.to_string()),
            col!(rows, |p| p.title_nl.clone()),
            col!(rows, |p| p.title_fr.clone()),
            col!(rows, |p| p.dossier_id.clone()),
            col!(rows, |p| p.document_id.clone()),
            col!(rows, |p| p.date.clone()),
            col!(rows, |p| p.source_url.clone()),
            col!(rows, |p| p.cache_path.clone()),
        ],
    )
}

fn write_notices(path: &Path, rows: &[ScrapedNotice]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("notice_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("title_nl", DataType::Utf8, false),
        Field::new("title_fr", DataType::Utf8, false),
        Field::new("date", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]));
    write_parquet(
        path,
        schema,
        vec![
            col!(rows, |n| n.notice_id.clone()),
            col!(rows, |n| n.session_id.to_string()),
            col!(rows, |n| n.meeting_id.to_string()),
            col!(rows, |n| n.title_nl.clone()),
            col!(rows, |n| n.title_fr.clone()),
            col!(rows, |n| n.date.clone()),
            col!(rows, |n| n.source_url.clone()),
            col!(rows, |n| n.cache_path.clone()),
        ],
    )
}

const MEETING_KIND: &str = "plenary";
const SOURCE_NAME: &str = "plenary_meetings";

const SESSION_IDS: &[u32] = &[56, 55];

fn meeting_url(session_id: u32, meeting_id: u32) -> String {
    format!(
        "https://www.dekamer.be/doc/PCRI/html/{}/ip{:03}x.html",
        session_id, meeting_id
    )
}

fn meeting_cache_file(session_id: u32, meeting_id: u32) -> std::path::PathBuf {
    cache_dir().join(format!(
        "sessions/{}/meetings/plenary/{}-{}.html",
        session_id, session_id, meeting_id
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let client = ScrapingClient::new();

    for &session_id in SESSION_IDS {
        if let Err(err) = scrape_session(&client, session_id).await {
            eprintln!(
                "[meetings-plenary] session {} failed entirely: {}",
                session_id, err
            );
        }
    }

    Ok(())
}

/// Scrape a single session.
async fn scrape_session(client: &ScrapingClient, session_id: u32) -> Result<(), Box<dyn Error>> {
    let session_dir = data_dir()
        .join("sessions")
        .join(session_id.to_string())
        .join("plenary");
    fs::create_dir_all(&session_dir).await?;

    let meeting_id_path = session_dir.join("current_plenary_id.txt");
    let current_meeting_id: u32 = if meeting_id_path.exists() {
        std::fs::read_to_string(&meeting_id_path)?.trim().parse()?
    } else {
        0
    };

    let mut web_request_count = 0u32;
    let gaps_path = session_dir.join("meeting_gaps.parquet");
    let mut gaps = load_prior_gaps(&gaps_path, session_id, MEETING_KIND)?;

    let last_meeting_id = if cache_only() {
        max_cached_meeting_id(session_id, "plenary").unwrap_or(current_meeting_id)
    } else {
        fetch_new_meetings(
            client,
            session_id,
            current_meeting_id,
            &mut web_request_count,
            &mut gaps,
        )
        .await?
    };

    if cache_only() {
        println!("[meetings-plenary] cache-only: parsing meetings 1..={last_meeting_id}");
    } else if last_meeting_id == current_meeting_id {
        println!(
            "[meetings-plenary] session {}: no new meeting available to download",
            session_id
        );
    } else {
        println!(
            "[meetings-plenary] session {}: found new meetings up to {}",
            session_id, last_meeting_id
        );
    }

    let mut all_meetings = Vec::new();
    let mut all_questions = Vec::new();
    let mut all_propositions = Vec::new();
    let mut all_notices = Vec::new();
    let mut all_vote_decisions = Vec::new();
    let mut all_vote_results = Vec::new();
    let mut all_vote_tallies = Vec::new();
    let mut all_vote_members = Vec::new();
    let mut all_vote_span_evidence = Vec::new();
    let mut all_vote_unresolved = Vec::new();
    let mut all_report_blocks = Vec::new();
    let mut all_source_spans = Vec::new();
    let mut all_agenda_items = Vec::new();
    let mut all_hearings = Vec::new();
    let mut all_interpellations = Vec::new();
    let mut all_utterances = Vec::new();
    let mut all_answers = Vec::new();
    let mut parsed_ids = BTreeSet::new();
    let mut manifest_rows = Vec::new();
    let run_mode = if cache_only() {
        SourceManifestRow::run_mode_cache_only()
    } else {
        SourceManifestRow::run_mode_live()
    };

    let mp = MultiProgress::new();
    let meetings_pb = mp.add(ProgressBar::new(last_meeting_id as u64));
    let template = format!(
        "[meetings-plenary] session {} [{{elapsed_precise}}] {{spinner:.blue}} {{bar:40.cyan/blue}} {{pos}}/{{len}} ({{percent}}%) | {{msg}}",
        session_id
    );
    meetings_pb.set_style(ProgressStyle::with_template(&template)?.tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"));
    meetings_pb.set_message(web_request_count.to_string());

    let mut encountered_dossier_ids: HashMap<String, String> = HashMap::new();

    for meeting_id in 1..=last_meeting_id {
        meetings_pb.set_message(format!("reqs={} meeting={}", web_request_count, meeting_id));

        let filepath = meeting_cache_file(session_id, meeting_id);
        let url = meeting_url(session_id, meeting_id);
        let rel_cache = relative_cache_path(&filepath, &cache_dir());

        if let Some(gap) = gaps.get(&meeting_id)
            && gap.reason == GAP_REASON_NOT_FOUND
        {
            push_gap_manifest(&mut manifest_rows, session_id, meeting_id, gap, run_mode);
            meetings_pb.inc(1);
            continue;
        }

        if !cache_only() && !filepath.exists() && !gaps.contains_key(&meeting_id) {
            match download_meeting(client, session_id, meeting_id, &mut web_request_count).await? {
                DownloadOutcome::NotFound => {
                    let gap = MeetingGapRow::new(
                        session_id,
                        MEETING_KIND,
                        meeting_id,
                        GAP_REASON_NOT_FOUND,
                        "HTTP 404",
                        &url,
                        "",
                    )
                    .with_timestamps(now_rfc3339(), now_rfc3339());
                    record_gap(&mut gaps, gap.clone());
                    push_gap_manifest(&mut manifest_rows, session_id, meeting_id, &gap, run_mode);
                    meetings_pb.inc(1);
                    continue;
                }
                DownloadOutcome::Saved { is_pdf } | DownloadOutcome::AlreadyCached { is_pdf } => {
                    if is_pdf {
                        let raw = std::fs::read(&filepath).unwrap_or_default();
                        let meta = read_cache_metadata(&filepath)?;
                        let gap = MeetingGapRow::new(
                            session_id,
                            MEETING_KIND,
                            meeting_id,
                            GAP_REASON_UNSUPPORTED_FORMAT,
                            "PDF report (unsupported HTML meeting format)",
                            &url,
                            &rel_cache,
                        )
                        .with_hashes(content_hash_bytes(&raw))
                        .with_timestamps(
                            meta.as_ref()
                                .map(|m| m.fetched_at.clone())
                                .unwrap_or_else(now_rfc3339),
                            meta.as_ref()
                                .map(|m| m.checked_at.clone())
                                .unwrap_or_else(now_rfc3339),
                        );
                        upsert_gap(&mut gaps, gap.clone());
                        push_gap_manifest(
                            &mut manifest_rows,
                            session_id,
                            meeting_id,
                            &gap,
                            run_mode,
                        );
                        meetings_pb.inc(1);
                        continue;
                    }
                }
            }
        }

        if filepath.exists() {
            let raw = std::fs::read(&filepath)?;
            if looks_like_pdf(&raw) {
                let meta = read_cache_metadata(&filepath)?;
                let gap = MeetingGapRow::new(
                    session_id,
                    MEETING_KIND,
                    meeting_id,
                    GAP_REASON_UNSUPPORTED_FORMAT,
                    "PDF report (unsupported HTML meeting format)",
                    &url,
                    &rel_cache,
                )
                .with_hashes(content_hash_bytes(&raw))
                .with_timestamps(
                    meta.as_ref()
                        .map(|m| m.fetched_at.clone())
                        .unwrap_or_else(now_rfc3339),
                    meta.as_ref()
                        .map(|m| m.checked_at.clone())
                        .unwrap_or_else(now_rfc3339),
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
                "incomplete snapshot: plenary meeting {meeting_id} cache missing at {} — aborting to preserve prior outputs",
                filepath.display()
            )
            .into());
        }

        match parse_meeting_from_cache(session_id, meeting_id, &mut encountered_dossier_ids).await {
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
                all_propositions.extend(output.propositions);
                all_notices.extend(output.notices);
                all_vote_decisions.extend(output.vote_bundle.decisions);
                all_vote_results.extend(output.vote_bundle.results);
                all_vote_tallies.extend(output.vote_bundle.tallies);
                all_vote_members.extend(output.vote_bundle.members);
                all_vote_span_evidence.extend(output.vote_bundle.span_evidence);
                all_vote_unresolved.extend(output.vote_bundle.unresolved_events);
                all_report_blocks.extend(output.report_blocks);
                append_source_spans(&mut all_source_spans, output.source_spans);
                all_agenda_items.extend(output.agenda_items);
                all_hearings.extend(output.hearings);
                all_interpellations.extend(output.interpellations);
                all_utterances.extend(output.utterances);
                all_answers.extend(output.answers);
            }
            Err(err) => {
                return Err(format!(
                    "plenary meeting {meeting_id} failed parser invariants ({err}) — aborting; not publishing a partial snapshot"
                )
                .into());
            }
        }

        meetings_pb.set_message(web_request_count.to_string());
        meetings_pb.inc(1);
    }

    meetings_pb.finish_with_message("done");

    gaps.retain(|id, _| *id <= last_meeting_id);
    reconcile_meeting_coverage(last_meeting_id, &parsed_ids, &gaps)?;
    let gap_rows: Vec<MeetingGapRow> = gaps.values().cloned().collect();
    validate_manifest_rows(&manifest_rows)?;

    let ids_path = cache_dir().join(format!("sessions/{}/dossier_ids.txt", session_id));
    let mut lines: Vec<String> = encountered_dossier_ids
        .iter()
        .map(|(id, date)| format!("{}\t{}", id, date))
        .collect();
    lines.sort();

    let derived_dir = data_dir().join(format!("derived/sessions/{session_id}/plenary"));
    std::fs::create_dir_all(&derived_dir)?;
    let manifest_final = manifest_path(SOURCE_NAME);

    let vote_bundle = VoteAssemblyOutput {
        decisions: all_vote_decisions,
        results: all_vote_results,
        tallies: all_vote_tallies,
        members: all_vote_members,
        span_evidence: all_vote_span_evidence,
        unresolved_events: all_vote_unresolved,
    };

    meetings_pb.finish_with_message("done");

    let mut bundle = BundlePublisher::new("plenary-meetings", &data_dir())?;
    let stage_meetings = bundle.stage_path(&session_dir.join("meetings.parquet"))?;
    let stage_questions = bundle.stage_path(&session_dir.join("questions.parquet"))?;
    let stage_propositions = bundle.stage_path(&session_dir.join("propositions.parquet"))?;
    let stage_notices = bundle.stage_path(&session_dir.join("notices.parquet"))?;
    let stage_votes = bundle.stage_path(&session_dir.join("votes.parquet"))?;
    let stage_vote_results = bundle.stage_path(&session_dir.join("vote_results.parquet"))?;
    let stage_vote_tallies = bundle.stage_path(&session_dir.join("vote_tallies.parquet"))?;
    let stage_vote_members = bundle.stage_path(&session_dir.join("vote_result_members.parquet"))?;
    let stage_vote_unresolved =
        bundle.stage_path(&session_dir.join("vote_unresolved_events.parquet"))?;
    let stage_hearings = bundle.stage_path(&session_dir.join("hearings.parquet"))?;
    let stage_interpellations = bundle.stage_path(&session_dir.join("interpellations.parquet"))?;
    let stage_agenda_items = bundle.stage_path(&session_dir.join("agenda_items.parquet"))?;
    let stage_utterances = bundle.stage_path(&session_dir.join("utterances.parquet"))?;
    let stage_answers = bundle.stage_path(&session_dir.join("answers.parquet"))?;
    let stage_gaps = bundle.stage_path(&gaps_path)?;
    let stage_blocks = bundle.stage_path(&derived_dir.join("report_blocks.parquet"))?;
    let stage_spans = bundle.stage_path(&derived_dir.join("source_spans.parquet"))?;
    let stage_manifest = bundle.stage_path(&manifest_final)?;
    let stage_checkpoint = bundle.stage_path(&meeting_id_path)?;
    let stage_dossier_ids = bundle.stage_path(&ids_path)?;

    write_meetings(&stage_meetings, &all_meetings)?;
    write_questions(&stage_questions, &all_questions)?;
    write_propositions(&stage_propositions, &all_propositions)?;
    write_notices(&stage_notices, &all_notices)?;
    // write_vote_bundle writes multiple files into a directory; stage into a temp dir then
    // map each file.
    let vote_stage_dir = bundle.staging_root().join("votes");
    std::fs::create_dir_all(&vote_stage_dir)?;
    write_vote_bundle(&vote_stage_dir, &vote_bundle)?;
    std::fs::rename(vote_stage_dir.join("votes.parquet"), &stage_votes)?;
    std::fs::rename(
        vote_stage_dir.join("vote_results.parquet"),
        &stage_vote_results,
    )?;
    std::fs::rename(
        vote_stage_dir.join("vote_tallies.parquet"),
        &stage_vote_tallies,
    )?;
    std::fs::rename(
        vote_stage_dir.join("vote_result_members.parquet"),
        &stage_vote_members,
    )?;
    std::fs::rename(
        vote_stage_dir.join("vote_unresolved_events.parquet"),
        &stage_vote_unresolved,
    )?;
    write_hearings_parquet(&stage_hearings, &all_hearings)?;
    write_interpellations_parquet(&stage_interpellations, &all_interpellations)?;
    write_agenda_items_parquet(&stage_agenda_items, &all_agenda_items)?;
    write_utterances_parquet(&stage_utterances, &all_utterances)?;
    write_answers_parquet(&stage_answers, &all_answers)?;
    write_meeting_gaps_parquet(&stage_gaps, &gap_rows)?;
    write_report_blocks_parquet(&stage_blocks, &all_report_blocks)?;
    write_source_spans_parquet(&stage_spans, &all_source_spans)?;
    write_source_manifest(&stage_manifest, &manifest_rows)?;
    std::fs::write(&stage_checkpoint, last_meeting_id.to_string())?;
    std::fs::write(&stage_dossier_ids, lines.join("\n"))?;

    bundle.commit()?;

    println!(
        "[meetings-plenary] session {}: scraped {} meetings using {} web requests ({} gaps)",
        session_id,
        all_meetings.len(),
        web_request_count,
        gap_rows.len()
    );

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

fn append_source_spans(target: &mut Vec<SourceSpanDraft>, rows: Vec<SourceSpanDraft>) {
    target.extend(rows);
}

fn record_dossier(map: &mut HashMap<String, String>, id: &str, date: &str) {
    let entry = map
        .entry(id.to_string())
        .or_insert_with(|| date.to_string());
    if date > entry.as_str() {
        *entry = date.to_string();
    }
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
                    record_gap(
                        gaps,
                        MeetingGapRow::new(
                            session_id,
                            MEETING_KIND,
                            miss,
                            GAP_REASON_NOT_FOUND,
                            "HTTP 404",
                            meeting_url(session_id, miss),
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
                    record_gap(
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
                eprintln!("[meetings-plenary] ip{probe:03} → ok (last={last}, pdf={is_pdf})");
            }
            DownloadOutcome::NotFound => {
                missing_streak.push(probe);
                consecutive_misses += 1;
                eprintln!(
                    "[meetings-plenary] ip{probe:03} → 404 ({consecutive_misses}/2 consecutive misses)"
                );
            }
        }
        probe += 1;
    }

    println!(
        "[meetings-plenary] last meeting for session {} = {}",
        session_id, last
    );

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
            return Err(format!("plenary meeting {meeting_id}: {detail} for {url}").into());
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

async fn parse_meeting_from_cache(
    session_id: u32,
    meeting_id: u32,
    encountered_dossier_ids: &mut HashMap<String, String>,
) -> Result<MeetingOutput, Box<dyn Error>> {
    let root = cache_dir();
    let filepath = meeting_cache_file(session_id, meeting_id);
    if !filepath.exists() {
        return Err(format!(
            "meeting {meeting_id} cache missing at {}",
            filepath.display()
        )
        .into());
    }

    let url = meeting_url(session_id, meeting_id);

    let cache_path = relative_cache_path(&filepath, &root);
    let content = read_report_html(&filepath)?;
    let document = Html::parse_document(&content);

    let date = extract_date_from_document(&document)?;
    let time_of_day = extract_time_of_day_from_document(&document)?;
    let start_time = extract_start_time_from_document(&document)?;
    let end_time = extract_end_time_from_document(&document)?;

    let typo_map: HashMap<String, String> = [
        ("Steven Coengrachts", "Steven Coenegrachts"),
        ("Ridouhane Chahid", "Ridouane Chahid"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    let parsed = parse_plenary_meeting_report(
        &document,
        session_id,
        meeting_id,
        &date,
        &url,
        &cache_path,
        &crawl::content_hash(&content),
    );
    for decision in &parsed.votes.decisions {
        if !decision.dossier_id.is_empty() {
            record_dossier(encountered_dossier_ids, &decision.dossier_id, &date);
        }
    }
    let mut questions: Vec<ScrapedQuestion> = extract_questions_from_agenda(
        &parsed.agenda,
        MeetingKind::Plenary,
        session_id,
        meeting_id,
        &typo_map,
        &url,
        &cache_path,
    )?
    .into_iter()
    .map(scraped_question_from_draft)
    .collect();
    let oral_written_items = &parsed.oral_written_items;
    for item in oral_written_items {
        if let Some(q) = questions
            .iter_mut()
            .find(|q| q.question_id == item.question_id)
        {
            q.question_body_nl = item.question_body_nl.clone();
            q.question_body_fr = item.question_body_fr.clone();
            q.treatment_mode = "oral_written".to_string();
        }
    }
    // Upstream features: questions carry the meeting date, and respondents
    // (actual speakers) may differ from the questionees (addressees).
    for q in questions.iter_mut() {
        q.date = date.clone();
        q.respondents = speakers_from_utterances(&parsed.utterances, &q.question_id)
            .into_iter()
            .filter(|s| !q.questioners.split(',').any(|n| n == s))
            .collect::<Vec<_>>()
            .join(",");
    }
    let propositions = extract_propositions(
        &parsed.blocks,
        session_id,
        meeting_id,
        &date,
        encountered_dossier_ids,
        &url,
        &cache_path,
    )
    .await?;
    let notices = extract_notices(
        &parsed.blocks,
        session_id,
        meeting_id,
        &date,
        &url,
        &cache_path,
    )
    .await?;
    let crawl::MeetingParseOutput {
        votes,
        report_block_rows: report_blocks,
        source_spans,
        agenda,
        hearings,
        interpellations,
        utterances,
        answers,
        ..
    } = parsed;
    let agenda_items = materialize_agenda_items(
        &agenda,
        MeetingKind::Plenary,
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
            source_url: url,
            cache_path,
        },
        questions,
        propositions,
        notices,
        vote_bundle: votes,
        report_blocks,
        source_spans,
        agenda_items,
        hearings,
        interpellations,
        utterances,
        answers,
    })
}

fn scraped_question_from_draft(draft: OralQuestionDraft) -> ScrapedQuestion {
    ScrapedQuestion {
        question_id: draft.question_id,
        session_id: draft.session_id,
        meeting_id: draft.meeting_id,
        questioners: draft.questioners,
        questionees: draft.questionees,
        respondents: String::new(),
        topics_nl: draft.topics_nl,
        topics_fr: draft.topics_fr,
        internal_ids: draft.internal_ids,
        question_body_nl: String::new(),
        question_body_fr: String::new(),
        treatment_mode: String::new(),
        date: String::new(),
        source_url: draft.source_url,
        cache_path: draft.cache_path,
    }
}

/// Collect the distinct speakers of the utterances belonging to an agenda
/// item, excluding the chair — upstream's "respondents" (actual speakers,
/// may differ from the questionee/addressee).
fn speakers_from_utterances(utterances: &[UtteranceDraft], agenda_item_id: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for u in utterances
        .iter()
        .filter(|u| u.agenda_item_id == agenda_item_id)
    {
        if u.speaker_role == "chair" {
            continue;
        }
        let name = u.raw_speaker.trim().to_string();
        if !name.is_empty() && name.to_lowercase() != "de voorzitter" && !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

/// Extract the propositions from the plenary meeting.
/// - Propositions can be found under the <h1> tag with the name "(wets)voorstel" or "(wets)voorstellen" as <h2> tags.
/// - The notices always have a <h2> in Dutch, and another <h2> in French. The language indicators (lang="NL" for example) are often wrong
///   so we decide NL/FR based on position: NL comes first, then FR.
/// - Some notices are included within the propositions section. These are detected and stored as notices.
fn block_heading_title(block: &ReportBlock) -> String {
    let text = clean_text(&block.text).replace('"', "'");
    let without_number = crawl::extract_agenda_number(&text)
        .and_then(|number| text.strip_prefix(&number))
        .unwrap_or(&text);
    without_number
        .trim()
        .trim_start_matches('-')
        .trim()
        .to_string()
}

async fn extract_propositions(
    blocks: &[ReportBlock],
    session_id: u32,
    meeting_id: u32,
    date: &str,
    encountered_dossier_ids: &mut HashMap<String, String>,
    source_url: &str,
    cache_path: &str,
) -> Result<Vec<ScrapedProposition>, Box<dyn Error>> {
    let mut propositions = Vec::new();
    let mut proposition_seq: i32 = 0;
    let mut found = false;
    let mut processing = false;
    let proposition_keywords_nl = ["voorstel", "wetsvoorstel"];
    let proposition_keywords_fr = ["proposition"];

    let mut all_titles: Vec<(Option<u32>, String, bool)> = Vec::new();

    for block in blocks {
        if block.tag == crawl::BlockTag::H1 {
            let text = block.text.to_lowercase();
            let is_dutch_propositions_header = proposition_keywords_nl
                .iter()
                .any(|&keyword| text.contains(keyword));
            if is_dutch_propositions_header {
                found = true;
                processing = true;
            } else if found
                && !proposition_keywords_fr
                    .iter()
                    .any(|&keyword| text.contains(keyword))
            {
                break;
            }
            continue;
        }

        if !processing || block.tag != crawl::BlockTag::H2 {
            continue;
        }

        let number =
            crawl::extract_agenda_number(&block.text).and_then(|value| value.parse::<u32>().ok());
        let text = block_heading_title(block);

        if text.is_empty() {
            continue;
        }

        let is_sub = text.starts_with('-');
        let clean = text.trim_start_matches('-').trim().to_string();
        all_titles.push((number, clean, is_sub));
    }

    // Group by agenda number: each new distinct number starts a new group.
    // Within each group the first half is NL titles, second half is FR titles.
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current_number: Option<u32> = None;

    for (number, text, _) in &all_titles {
        match number {
            Some(n) if Some(*n) != current_number => {
                current_number = Some(*n);
                groups.push(vec![text.clone()]);
            }
            _ => {
                if let Some(group) = groups.last_mut() {
                    group.push(text.clone());
                }
            }
        }
    }

    for group in groups {
        let half = group.len() / 2;
        let nl_titles = &group[..half];
        let fr_titles = &group[half..];

        for (nl, fr) in nl_titles.iter().zip(fr_titles.iter()) {
            let data_nl = extract_proposition_data(nl.clone())?;
            let data_fr = extract_proposition_data(fr.clone())?;
            let dossier_id_opt = data_nl.dossier_id.clone();

            propositions.push(ScrapedProposition {
                proposition_id: composite_id(session_id, meeting_id, proposition_seq),
                session_id,
                meeting_id,
                title_nl: data_nl.topic,
                title_fr: data_fr.topic,
                dossier_id: dossier_id_opt.clone().unwrap_or_default(),
                document_id: data_nl.document_id.unwrap_or_default(),
                date: date.to_string(),
                source_url: source_url.to_string(),
                cache_path: cache_path.to_string(),
            });
            proposition_seq += 1;

            if let Some(ref id) = dossier_id_opt {
                record_dossier(encountered_dossier_ids, id, date);
            }
        }
    }

    Ok(propositions)
}

/// Extract the notices from the plenary meeting.
/// - Notices can be found under the <h1> tag with the name "mededeling" or "mededelingen" as <h2> tags.
/// - The notices always have a <h2> in Dutch, and another <h2> in French. The language indicators (lang="NL" for example) are often wrong
///   so we decide NL/FR based on position: NL comes first, then FR.
/// - Some notices are not put under a separate <h1> header but are included wihin the propositions sector. This is handled in the extract_propositions function.
async fn extract_notices(
    blocks: &[ReportBlock],
    session_id: u32,
    meeting_id: u32,
    date: &str,
    source_url: &str,
    cache_path: &str,
) -> Result<Vec<ScrapedNotice>, Box<dyn Error>> {
    let mut notices = Vec::new();
    let mut notice_seq: i32 = 0;
    let mut found = false;
    let mut processing = false;
    let notice_keywords_nl = ["mededeling", "mededelingen"];
    let notice_keywords_fr = ["communication", "communications"];

    let mut all_titles: Vec<(Option<u32>, String, bool)> = Vec::new();

    for block in blocks {
        if block.tag == crawl::BlockTag::H1 {
            let text = block.text.to_lowercase();
            let is_dutch_notice_header = notice_keywords_nl
                .iter()
                .any(|&keyword| text.contains(keyword));
            if is_dutch_notice_header {
                found = true;
                processing = true;
            } else if found
                && !notice_keywords_fr
                    .iter()
                    .any(|&keyword| text.contains(keyword))
            {
                break;
            }
            continue;
        }

        if !processing || block.tag != crawl::BlockTag::H2 {
            continue;
        }

        let number =
            crawl::extract_agenda_number(&block.text).and_then(|value| value.parse::<u32>().ok());
        let text = block_heading_title(block);

        if text.is_empty() {
            continue;
        }

        let is_sub = text.starts_with('-');
        let clean = text.trim_start_matches('-').trim().to_string();
        all_titles.push((number, clean, is_sub));
    }

    // Group by agenda number: each new distinct number starts a new group.
    // Within each group the first half is NL titles, second half is FR titles.
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current_number: Option<u32> = None;

    for (number, text, _) in &all_titles {
        match number {
            Some(n) if Some(*n) != current_number => {
                current_number = Some(*n);
                groups.push(vec![text.clone()]);
            }
            _ => {
                if let Some(group) = groups.last_mut() {
                    group.push(text.clone());
                }
            }
        }
    }

    for group in groups {
        let half = group.len() / 2;
        let nl_titles = &group[..half];
        let fr_titles = &group[half..];

        for (nl, fr) in nl_titles.iter().zip(fr_titles.iter()) {
            notices.push(ScrapedNotice {
                notice_id: composite_id(session_id, meeting_id, notice_seq),
                session_id,
                meeting_id,
                title_nl: nl.clone(),
                title_fr: fr.clone(),
                date: date.to_string(),
                source_url: source_url.to_string(),
                cache_path: cache_path.to_string(),
            });
            notice_seq += 1;
        }
    }

    Ok(notices)
}

fn extract_proposition_data(proposition_text: String) -> Result<PropositionData, Box<dyn Error>> {
    if let Some(captures) = proposition_regex().captures(&proposition_text) {
        let document_id = captures[3]
            .trim()
            .strip_prefix("1-")
            .unwrap_or(captures[3].trim())
            .to_string();
        return Ok(PropositionData {
            topic: captures[1].trim().to_string(),
            dossier_id: Some(captures[2].trim().to_string()),
            document_id: Some(document_id),
        });
    }
    if let Some(captures) = proposition_topic_regex().captures(&proposition_text) {
        return Ok(PropositionData {
            topic: captures[1].trim().to_string(),
            dossier_id: None,
            document_id: None,
        });
    }
    Err("No regex matched for proposition text".into())
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

    // Format 1: "10 november 2021"
    if let Some(caps) = date_regex().captures(&text) {
        let day = format!("{:02}", caps[1].parse::<u8>()?);
        let month = month_name_to_number(&caps[2])?;
        return Ok(format!("{}-{}-{}", &caps[3], month, day));
    }

    // Format 2): "3-10-2019" (used in plenary meeting 55007)
    if let Some(caps) = date_numeric_regex().captures(&text) {
        let day = format!("{:02}", caps[1].parse::<u8>()?);
        let month = format!("{:02}", caps[2].parse::<u8>()?);
        return Ok(format!("{}-{}-{}", &caps[3], month, day));
    }

    Err("Could not find date in document".into())
}

fn month_name_to_number(name: &str) -> Result<&'static str, Box<dyn Error>> {
    Ok(match &name.to_lowercase()[..] {
        // Dutch
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
        // French
        "janvier" => "01",
        "février" | "fevrier" => "02",
        "mars" => "03",
        "avril" => "04",
        "mai" => "05",
        "juin" => "06",
        "juillet" => "07",
        "août" | "aout" => "08",
        "septembre" => "09",
        "octobre" => "10",
        "novembre" => "11",
        "décembre" | "decembre" => "12",
        other => return Err(format!("Invalid month name: {}", other).into()),
    })
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
    ["wordt geopend", "wordt hervat"]
        .iter()
        .find_map(|phrase| extract_time_from_document(document, phrase).ok())
        .ok_or_else(|| "Could not extract start time from the document".into())
}

fn extract_end_time_from_document(document: &Html) -> Result<String, Box<dyn Error>> {
    [
        "De vergadering wordt gesloten",
        "De vergadering wordt geschorst",
        "De openbare commissievergadering wordt gesloten", // IP55039
    ]
    .iter()
    .find_map(|phrase| extract_time_from_document(document, phrase).ok())
    .ok_or_else(|| "Could not extract end time from the document".into())
}

fn extract_time_from_document(document: &Html, keyword: &str) -> Result<String, Box<dyn Error>> {
    let keyword_lower = keyword.to_lowercase();

    document
        .select(selector_span())
        .filter_map(|span| {
            let text = span.text().collect::<Vec<_>>().join(" ");
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.to_lowercase().contains(&keyword_lower) {
                return None;
            }
            time_regex()
                .captures(&text)
                .map(|caps| format!("{}h{}", &caps[1], &caps[2]))
        })
        .last()
        .ok_or_else(|| "Could not extract time from the document".into())
}

#[cfg(test)]
mod question_extract_tests {
    use super::*;
    use arrow::array::StringArray;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use scraper::Html;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn cached_plenary_html(meeting_id: u32) -> Option<PathBuf> {
        dotenvy::dotenv().ok();
        let path = cache_dir().join(format!("sessions/56/meetings/plenary/56-{meeting_id}.html"));
        if path.exists() { Some(path) } else { None }
    }

    #[tokio::test]
    async fn plenary_82_extracts_questions() {
        let Some(path) = cached_plenary_html(82) else {
            return;
        };
        let content = read_report_html(&path).unwrap();
        let document = Html::parse_document(&content);
        let date = extract_date_from_document(&document).unwrap();
        let parsed = parse_plenary_meeting_report(
            &document,
            56,
            82,
            &date,
            "http://example.test",
            "sessions/56/meetings/plenary/56-82.html",
            &crawl::content_hash(&content),
        );
        let typo_map = HashMap::new();
        let questions: Vec<ScrapedQuestion> = extract_questions_from_agenda(
            &parsed.agenda,
            MeetingKind::Plenary,
            56,
            82,
            &typo_map,
            "http://example.test",
            "sessions/56/meetings/plenary/56-82.html",
        )
        .unwrap()
        .into_iter()
        .map(scraped_question_from_draft)
        .collect();
        assert!(
            questions.len() >= 10,
            "expected at least 10 questions, got {}",
            questions.len()
        );
    }

    #[tokio::test]
    async fn plenary_60_extracts_votes() {
        let Some(path) = cached_plenary_html(60) else {
            return;
        };
        let content = read_report_html(&path).unwrap();
        let document = Html::parse_document(&content);
        let date = extract_date_from_document(&document).unwrap();
        let parsed = crawl::parse_plenary_meeting_report(
            &document,
            56,
            60,
            &date,
            "http://example.test",
            "sessions/56/meetings/plenary/56-60.html",
            &crawl::content_hash(&content),
        );
        assert!(
            parsed.votes.decisions.len() >= 50,
            "expected many vote decisions for meeting 60, got {}",
            parsed.votes.decisions.len()
        );
        assert!(
            parsed
                .votes
                .tallies
                .iter()
                .any(|t| t.option_key == "yes" && t.count > 0),
            "expected at least one vote with yes tallies"
        );
        assert!(
            !parsed.source_spans.is_empty(),
            "expected provenance spans for meeting 60"
        );
    }

    #[tokio::test]
    async fn plenary_82_parse_from_cache_returns_questions() {
        if cached_plenary_html(82).is_none() {
            return;
        }
        dotenvy::dotenv().ok();
        let mut encountered_dossier_ids = HashMap::new();
        let output = parse_meeting_from_cache(56, 82, &mut encountered_dossier_ids)
            .await
            .expect("parse meeting 82 from repo cache");
        assert!(
            output.questions.len() >= 10,
            "meeting 82 returned {} questions",
            output.questions.len()
        );
    }

    #[tokio::test]
    async fn staged_heading_entity_ids_match_provenance_spans() {
        let html = r#"
            <h1>Voorstellen</h1>
            <h2><span>01</span><span>Voorstel over testen (56/1)</span></h2>
            <h2><span>Proposition relative aux tests (56/1)</span></h2>
            <h1>Mededelingen</h1>
            <h2><span>02</span><span>Mededeling over testen</span></h2>
            <h2><span>Communication relative aux tests</span></h2>
        "#;
        let document = Html::parse_document(html);
        let parsed = parse_plenary_meeting_report(
            &document,
            56,
            9,
            "2026-01-01",
            "url",
            "cache",
            &crawl::content_hash(html),
        );
        let mut dossiers = HashMap::new();
        let propositions = extract_propositions(
            &parsed.blocks,
            56,
            9,
            "2026-01-01",
            &mut dossiers,
            "url",
            "cache",
        )
        .await
        .unwrap();
        let notices = extract_notices(&parsed.blocks, 56, 9, "2026-01-01", "url", "cache")
            .await
            .unwrap();
        assert_eq!(propositions.len(), 1);
        assert_eq!(notices.len(), 1);
        assert!(parsed.source_spans.iter().any(|span| {
            span.entity_type == "Proposition"
                && span.entity_id == propositions[0].proposition_id
                && span.span_role == "proposition_body"
        }));
        assert!(parsed.source_spans.iter().any(|span| {
            span.entity_type == "Notice"
                && span.entity_id == notices[0].notice_id
                && span.span_role == "notice_body"
        }));
    }

    #[test]
    fn production_aggregation_persists_unresolved_span_rows() {
        let html = "<html></html>";
        let parsed = parse_plenary_meeting_report(
            &Html::parse_document(html),
            56,
            99,
            "2026-01-01",
            "url",
            "cache",
            &crawl::content_hash(html),
        );
        assert!(parsed.source_spans.iter().any(|span| {
            span.validation_status == "unresolved"
                && span.unresolved_reason == "invalid_half_open_range"
        }));

        let mut aggregated = Vec::new();
        append_source_spans(&mut aggregated, parsed.source_spans);
        let path = std::env::temp_dir().join(format!(
            "plenary-source-spans-{}-{}.parquet",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        write_source_spans_parquet(&path, &aggregated).unwrap();
        let mut reader = ParquetRecordBatchReaderBuilder::try_new(File::open(&path).unwrap())
            .unwrap()
            .build()
            .unwrap();
        let batch = reader.next().unwrap().unwrap();
        let statuses = batch
            .column_by_name("validation_status")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let reasons = batch
            .column_by_name("unresolved_reason")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(statuses.value(0), "unresolved");
        assert_eq!(reasons.value(0), "invalid_half_open_range");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_html_without_meeting_date_fails_parser_invariants() {
        // HTTP 200 with HTML that fails required header invariants must abort
        // publication rather than becoming a source gap.
        let document =
            Html::parse_document("<html><body><p>not a meeting report</p></body></html>");
        let err = extract_date_from_document(&document).unwrap_err();
        assert!(!err.to_string().is_empty());
    }
}
