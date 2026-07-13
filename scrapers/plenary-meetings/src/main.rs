use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use crawl::client::ScrapingClient;
use crawl::paths::{cache_dir, cache_only, data_dir};
use crawl::report_blocks::read_report_html;
use crawl::utils::{
    clean_text, composite_id, composite_scoped_id, max_cached_meeting_id, relative_cache_path,
};
use crawl::{
    AgendaItem, AnswerDraft, HearingDraft, InterpellationDraft, ItemKind, QuestionHeadingRole,
    ReportBlock, ReportBlockRow, SourceSpanDraft, UtteranceDraft, VoteAssemblyOutput,
    classify_question_heading_bilingual, classify_question_heading_text, has_pending_question_text,
    parse_plenary_meeting_report, write_answers_parquet, write_hearings_parquet,
    write_interpellations_parquet, write_report_blocks_parquet, write_source_spans_parquet,
    write_utterances_parquet, write_vote_bundle,
};
use encoding_rs::WINDOWS_1252;
use http::StatusCode;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parquet::arrow::ArrowWriter;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tokio::fs;

/// REGEXES
static QUESTION_REGEX: OnceLock<Regex> = OnceLock::new();
static TIME_REGEX: OnceLock<Regex> = OnceLock::new();
static DATE_REGEX: OnceLock<Regex> = OnceLock::new();
static PROPOSITION_REGEX: OnceLock<Regex> = OnceLock::new();
static PROPOSITION_TOPIC_REGEX: OnceLock<Regex> = OnceLock::new();

fn question_regex() -> &'static Regex {
    // NOTE: Handles question IDs in the format of `(56001442P)`
    QUESTION_REGEX.get_or_init(|| {
        Regex::new(r#"(?m)(?:(?:Vraag van|Question de)\s)?([^\n]+?)\s+(?:aan|à)\s+([^\n]+?)\s*\([^)]*\)\s*(?:over|sur)\s*(.+?)(?:\s*\((\d{8}[A-Z])\))?\s*$"#).unwrap()
    })
}

fn time_regex() -> &'static Regex {
    TIME_REGEX.get_or_init(|| Regex::new(r"(\d{1,2})\.(\d{2})\s*uur").unwrap())
}

fn date_regex() -> &'static Regex {
    DATE_REGEX.get_or_init(|| Regex::new(r"(\d{1,2})\s+([a-zA-Z]+)\s+(\d{4})").unwrap())
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
static SELECTOR_H1_OR_H2_OR_P: OnceLock<Selector> = OnceLock::new();

fn selector_span() -> &'static Selector {
    SELECTOR_SPAN.get_or_init(|| Selector::parse("span").unwrap())
}
fn selector_table() -> &'static Selector {
    SELECTOR_TABLE.get_or_init(|| Selector::parse("table").unwrap())
}
fn selector_h1_or_h2_or_p() -> &'static Selector {
    SELECTOR_H1_OR_H2_OR_P.get_or_init(|| Selector::parse("h1, h2, p").unwrap())
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

struct ScrapedProposition {
    proposition_id: String,
    session_id: u32,
    meeting_id: u32,
    title_nl: String,
    title_fr: String,
    dossier_id: String,
    document_id: String,
    source_url: String,
    cache_path: String,
}

struct ScrapedNotice {
    notice_id: String,
    session_id: u32,
    meeting_id: u32,
    title_nl: String,
    title_fr: String,
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
    hearings: Vec<HearingDraft>,
    interpellations: Vec<InterpellationDraft>,
    utterances: Vec<UtteranceDraft>,
    answers: Vec<AnswerDraft>,
}

struct QuestionData {
    questioners: Vec<String>,
    respondents: Vec<String>,
    topics: Vec<String>,
    internal_ids: Vec<String>,
}

impl Default for QuestionData {
    fn default() -> Self {
        Self {
            questioners: Vec::new(),
            respondents: Vec::new(),
            topics: Vec::new(),
            internal_ids: Vec::new(),
        }
    }
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

fn write_propositions(path: &Path, rows: &[ScrapedProposition]) -> Result<(), Box<dyn Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("proposition_id", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("meeting_id", DataType::Utf8, false),
        Field::new("title_nl", DataType::Utf8, false),
        Field::new("title_fr", DataType::Utf8, false),
        Field::new("dossier_id", DataType::Utf8, false),
        Field::new("document_id", DataType::Utf8, false),
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
            col!(rows, |n| n.source_url.clone()),
            col!(rows, |n| n.cache_path.clone()),
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
        .join("plenary");
    fs::create_dir_all(&session_dir).await?;

    let meeting_id_path = data_dir().join("current_plenary_id.txt");
    let current_meeting_id: u32 = std::fs::read_to_string(&meeting_id_path)?.trim().parse()?;

    let mut web_request_count = 0u32;
    let last_meeting_id = if cache_only() {
        max_cached_meeting_id(session_id, "plenary").unwrap_or(current_meeting_id)
    } else {
        discover_last_meeting_id(
            &client,
            session_id,
            current_meeting_id,
            &mut web_request_count,
        )
        .await?
    };

    if cache_only() {
        println!("[meetings-plenary] cache-only: parsing meetings 1..={last_meeting_id}");
    } else if last_meeting_id == current_meeting_id {
        println!("[meetings-plenary] no new meeting available to download");
    } else {
        println!(
            "[meetings-plenary] found new meetings up to {}",
            last_meeting_id
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
    let mut all_hearings = Vec::new();
    let mut all_interpellations = Vec::new();
    let mut all_utterances = Vec::new();
    let mut all_answers = Vec::new();

    let mp = MultiProgress::new();
    let meetings_pb = mp.add(ProgressBar::new(last_meeting_id as u64));
    meetings_pb.set_style(
        ProgressStyle::with_template(
            "[meetings-plenary] [{elapsed_precise}] {spinner:.blue} {bar:40.cyan/blue} {pos}/{len} ({percent}%) | {msg}",
        )?
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );

    meetings_pb.set_message(web_request_count.to_string());

    // Collect dossier ids mentioned within the meetings
    let mut encountered_dossier_ids: HashMap<String, String> = HashMap::new();

    for meeting_id in 1..=last_meeting_id {
        meetings_pb.set_message(format!("reqs={} meeting={}", web_request_count, meeting_id));

        let result = if cache_only() {
            parse_meeting_from_cache(session_id, meeting_id, &mut encountered_dossier_ids).await
        } else {
            scrape_meeting(
                &client,
                session_id,
                meeting_id,
                &mut web_request_count,
                &mut encountered_dossier_ids,
            )
            .await
        };

        match result {
            Ok(output) => {
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
                all_hearings.extend(output.hearings);
                all_interpellations.extend(output.interpellations);
                all_utterances.extend(output.utterances);
                all_answers.extend(output.answers);
            }
            Err(err) => {
                eprintln!("[meetings-plenary] failed meeting {}: {}", meeting_id, err);
            }
        };

        meetings_pb.set_message(web_request_count.to_string());
        meetings_pb.inc(1);
    }

    // Write collected dossier ids
    let ids_path = cache_dir().join(format!("sessions/{}/dossier_ids.txt", session_id));
    let mut lines: Vec<String> = encountered_dossier_ids
        .iter()
        .map(|(id, date)| format!("{}\t{}", id, date))
        .collect();
    lines.sort();
    std::fs::write(&ids_path, lines.join("\n"))?;

    meetings_pb.finish_with_message("done");

    std::fs::write(&meeting_id_path, last_meeting_id.to_string())?;

    write_meetings(&session_dir.join("meetings.parquet"), &all_meetings)?;
    write_questions(&session_dir.join("questions.parquet"), &all_questions)?;
    write_propositions(&session_dir.join("propositions.parquet"), &all_propositions)?;
    write_notices(&session_dir.join("notices.parquet"), &all_notices)?;
    let vote_bundle = VoteAssemblyOutput {
        decisions: all_vote_decisions,
        results: all_vote_results,
        tallies: all_vote_tallies,
        members: all_vote_members,
        span_evidence: all_vote_span_evidence,
        unresolved_events: all_vote_unresolved,
    };
    write_vote_bundle(&session_dir, &vote_bundle)?;

    let derived_dir = data_dir().join(format!("derived/sessions/{session_id}/plenary"));
    std::fs::create_dir_all(&derived_dir)?;
    write_report_blocks_parquet(
        &derived_dir.join("report_blocks.parquet"),
        &all_report_blocks,
    )?;
    write_source_spans_parquet(&derived_dir.join("source_spans.parquet"), &all_source_spans)?;
    write_hearings_parquet(&session_dir.join("hearings.parquet"), &all_hearings)?;
    write_interpellations_parquet(
        &session_dir.join("interpellations.parquet"),
        &all_interpellations,
    )?;
    write_utterances_parquet(&session_dir.join("utterances.parquet"), &all_utterances)?;
    write_answers_parquet(&session_dir.join("answers.parquet"), &all_answers)?;

    println!(
        "[meetings-plenary] scraped {} meetings using {} web requests",
        all_meetings.len(),
        web_request_count
    );
    Ok(())
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

async fn discover_last_meeting_id(
    client: &ScrapingClient,
    session_id: u32,
    current_id: u32,
    web_request_count: &mut u32,
) -> Result<u32, Box<dyn Error>> {
    let mut last = current_id;
    loop {
        let probe = last + 1;
        let url = format!(
            "https://www.dekamer.be/doc/PCRI/html/{}/ip{:03}x.html",
            session_id, probe
        );
        let resp = client.get(&url).await?;
        *web_request_count += 1;
        if resp.status() == StatusCode::NOT_FOUND {
            break;
        }
        last = probe;
    }
    Ok(last)
}

async fn scrape_meeting(
    client: &ScrapingClient,
    session_id: u32,
    meeting_id: u32,
    web_request_count: &mut u32,
    encountered_dossier_ids: &mut HashMap<String, String>,
) -> Result<MeetingOutput, Box<dyn Error>> {
    let filepath = cache_dir().join(format!(
        "sessions/{}/meetings/plenary/{}-{}.html",
        session_id, session_id, meeting_id
    ));
    let url = format!(
        "https://www.dekamer.be/doc/PCRI/html/{}/ip{:03}x.html",
        session_id, meeting_id
    );

    if !filepath.exists() {
        let response = client.get(&url).await?;
        *web_request_count += 1;
        let raw_bytes = response.bytes().await?;
        let (decoded_str, _, _) = WINDOWS_1252.decode(&raw_bytes);
        if let Some(parent) = filepath.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&filepath, decoded_str.as_ref())?;
    }

    parse_meeting_from_cache(session_id, meeting_id, encountered_dossier_ids).await
}

async fn parse_meeting_from_cache(
    session_id: u32,
    meeting_id: u32,
    encountered_dossier_ids: &mut HashMap<String, String>,
) -> Result<MeetingOutput, Box<dyn Error>> {
    let root = cache_dir();
    let filepath = root.join(format!(
        "sessions/{}/meetings/plenary/{}-{}.html",
        session_id, session_id, meeting_id
    ));
    if !filepath.exists() {
        return Err(format!(
            "meeting {meeting_id} cache missing at {}",
            filepath.display()
        )
        .into());
    }

    let url = format!(
        "https://www.dekamer.be/doc/PCRI/html/{}/ip{:03}x.html",
        session_id, meeting_id
    );

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
    let mut questions = extract_questions_from_agenda(
        &parsed.agenda,
        session_id,
        meeting_id,
        &typo_map,
        &url,
        &cache_path,
    )?;
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
    let notices =
        extract_notices(&parsed.blocks, session_id, meeting_id, &url, &cache_path).await?;
    let crawl::MeetingParseOutput {
        votes,
        report_block_rows: report_blocks,
        source_spans,
        hearings,
        interpellations,
        utterances,
        answers,
        ..
    } = parsed;

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
        hearings,
        interpellations,
        utterances,
        answers,
    })
}

fn extract_questions_from_agenda(
    agenda: &[AgendaItem],
    session_id: u32,
    meeting_id: u32,
    typo_map: &HashMap<String, String>,
    source_url: &str,
    cache_path: &str,
) -> Result<Vec<ScrapedQuestion>, Box<dyn Error>> {
    agenda
        .iter()
        .filter(|item| item.item_kind == ItemKind::Question)
        .map(|item| {
            let data_nl = extract_question_data(typo_map, &item.title_nl)?;
            let data_fr = extract_question_data(typo_map, &item.title_fr)?;
            let mut internal_ids = item.internal_ids.clone();
            internal_ids.extend(data_nl.internal_ids);
            internal_ids.extend(data_fr.internal_ids);
            internal_ids.sort();
            internal_ids.dedup();
            Ok(ScrapedQuestion {
                question_id: item.item_id.clone(),
                session_id,
                meeting_id,
                questioners: data_nl.questioners.join(","),
                respondents: data_nl.respondents.join(","),
                topics_nl: data_nl.topics.join(";"),
                topics_fr: data_fr.topics.join(";"),
                internal_ids: internal_ids.join(","),
                question_body_nl: String::new(),
                question_body_fr: String::new(),
                treatment_mode: String::new(),
                source_url: source_url.to_string(),
                cache_path: cache_path.to_string(),
            })
        })
        .collect()
}

#[allow(dead_code)]
async fn extract_questions(
    document: &Html,
    session_id: u32,
    meeting_id: u32,
    typo_map: &HashMap<String, String>,
    source_url: &str,
    cache_path: &str,
) -> Result<Vec<ScrapedQuestion>, Box<dyn Error>> {
    let mut questions = Vec::new();
    let mut previous_nl = String::new();
    let mut previous_fr = String::new();
    let mut question_seq: i32 = 0;
    let mut found_questions_section = false;
    let mut processing = false;

    let flush_question = |seq: i32,
                          nl: &str,
                          fr: &str,
                          typo_map: &HashMap<String, String>|
     -> Result<Option<ScrapedQuestion>, Box<dyn Error>> {
        if nl.is_empty() && fr.is_empty() {
            return Ok(None);
        }
        let data_nl = extract_question_data(typo_map, nl)?;
        let data_fr = extract_question_data(typo_map, fr)?;
        Ok(Some(ScrapedQuestion {
            question_id: composite_scoped_id(session_id, "plenary", meeting_id, seq),
            session_id,
            meeting_id,
            questioners: data_nl.questioners.join(","),
            respondents: data_nl.respondents.join(","),
            topics_nl: data_nl.topics.join(";"),
            topics_fr: data_fr.topics.join(";"),
            internal_ids: data_nl.internal_ids.join(","),
            question_body_nl: String::new(),
            question_body_fr: String::new(),
            treatment_mode: String::new(),
            source_url: source_url.to_string(),
            cache_path: cache_path.to_string(),
        }))
    };

    // The keywords that indicate the questions section has started.
    let questions_section_keywords = crawl::question_boundaries::QUESTIONS_SECTION_KEYWORDS;

    for element in document.select(selector_h1_or_h2_or_p()) {
        let tag = element.value().name();

        if tag == "h1" {
            let text = element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .replace("\n", " ")
                .trim()
                .to_lowercase();

            if questions_section_keywords
                .iter()
                .any(|&keyword| text.contains(keyword))
            {
                found_questions_section = true;
                processing = true;
            } else if found_questions_section {
                if let Some(q) = flush_question(question_seq, &previous_nl, &previous_fr, typo_map)?
                {
                    questions.push(q);
                }
                break;
            }
            continue;
        }

        if !processing {
            continue;
        }

        if tag == "h2" {
            let (mut found_nl, mut found_fr) = extract_bilingual_spans(&element);
            let full_heading =
                clean_text(&element.text().collect::<Vec<_>>().join(" ")).replace("\"", "'");
            if classify_question_heading_bilingual(found_nl.as_deref(), found_fr.as_deref())
                == QuestionHeadingRole::Unrelated
            {
                let role = classify_question_heading_text(&full_heading);
                if role != QuestionHeadingRole::Unrelated {
                    if is_likely_french(&full_heading) {
                        found_fr = Some(full_heading);
                    } else {
                        found_nl = Some(full_heading);
                    }
                }
            }

            let heading_role =
                classify_question_heading_bilingual(found_nl.as_deref(), found_fr.as_deref());

            if matches!(
                heading_role,
                QuestionHeadingRole::Unrelated | QuestionHeadingRole::Hearing
            ) {
                continue;
            }

            let is_group_start = matches!(heading_role, QuestionHeadingRole::GroupStart);
            let is_subquestion = matches!(heading_role, QuestionHeadingRole::SubQuestion);
            let is_single = matches!(heading_role, QuestionHeadingRole::Single);
            let is_fr_group_header = matches!(heading_role, QuestionHeadingRole::FrGroupHeader);

            if is_group_start || is_single {
                if has_pending_question_text(&previous_nl, &previous_fr) {
                    if let Some(q) =
                        flush_question(question_seq, &previous_nl, &previous_fr, typo_map)?
                    {
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
            } else if is_fr_group_header {
                if let Some(t) = found_fr.or(found_nl) {
                    if previous_fr.is_empty() {
                        previous_fr = t;
                    }
                }
            }
        }

        if tag == "p" {
            let text = element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();

            if text.contains("Het incident is gesloten") || text.contains("L'incident est clos") {
                if let Some(q) = flush_question(question_seq, &previous_nl, &previous_fr, typo_map)?
                {
                    questions.push(q);
                    question_seq += 1;
                }
                previous_nl.clear();
                previous_fr.clear();
                continue;
            }
        }
    }

    if has_pending_question_text(&previous_nl, &previous_fr) {
        if let Some(q) = flush_question(question_seq, &previous_nl, &previous_fr, typo_map)? {
            questions.push(q);
        }
    }

    Ok(questions)
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
                source_url: source_url.to_string(),
                cache_path: cache_path.to_string(),
            });
            notice_seq += 1;
        }
    }

    Ok(notices)
}

fn extract_bilingual_spans(element: &ElementRef) -> (Option<String>, Option<String>) {
    let french_indicators = ["questions jointes"];
    let dutch_indicators = ["samengevoegde vragen"];

    let mut nl: Option<String> = None;
    let mut fr: Option<String> = None;

    if let Some(span) = element
        .select(selector_span())
        .filter(|s| matches!(s.value().attr("lang"), Some("NL") | Some("NL-BE")))
        .last()
    {
        let text = clean_text(&span.text().collect::<Vec<_>>().join(" ")).replace("\"", "");
        if french_indicators
            .iter()
            .any(|w| text.to_lowercase().contains(w))
        {
            fr = Some(text);
        } else {
            nl = Some(text);
        }
    }
    if let Some(span) = element
        .select(selector_span())
        .filter(|s| matches!(s.value().attr("lang"), Some("FR") | Some("FR-BE")))
        .last()
    {
        let text = clean_text(&span.text().collect::<Vec<_>>().join(" ")).replace("\"", "");

        // NOTE: We override FR -> NL if clearly dutch based on some indicator words.
        if text.to_lowercase().contains(" aan ") {
            nl = Some(text);
        } else if dutch_indicators
            .iter()
            .any(|w| text.to_lowercase().contains(w))
        {
            nl = Some(text);
        } else {
            fr = Some(text);
        }
    }
    if nl.is_none() && fr.is_none() {
        let full = clean_text(&element.text().collect::<Vec<_>>().join(" ")).replace("\"", "'");
        if !full.is_empty() {
            nl = Some(full);
        }
    }
    (nl, fr)
}

/// Check if the text is likely French based on common French patterns and no Dutch core words.
fn is_likely_french(text: &str) -> bool {
    let lower = text.to_lowercase();

    // French contractions and unambiguous function words that don't appear in Dutch
    let french_patterns = [
        "d'",
        "d’",
        "l'",
        "qu'", // contractions
        "à la",
        "à l'", // safer than bare "à"
        " au ",
        " aux ",
        " les ",
        " du ",
        " des ",
        " une ",
        " pour ",
        " dans ",
        " sont ",
        " qui ",
        " sur ",
        "constitutionnelle", // based on detected language mislabeling issue in plenary meeting 19
        "comptes",           // based on detected language mislabeling issue in plenary meeting  19
        "commission",        // based on detected language mislabeling issue in plenary meeting 19
    ];

    if french_patterns.iter().any(|&p| lower.contains(p)) {
        return true;
    }

    // Accented chars + no Dutch core words = probably a French loanword context
    let has_accented = lower.chars().any(|c| "éèêëàâîïôùûü".contains(c));
    has_accented && !is_likely_dutch(text)
}

/// Check if the text is likely Dutch based on common Dutch core words and no French patterns.
fn is_likely_dutch(text: &str) -> bool {
    let lower = text.to_lowercase();
    let dutch_core_words = [
        "van", "het", "een", "met", "tot", "aan", "bij", "naar", "over", "uit", "zijn", "voor",
        "ons",
    ];
    lower
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphabetic()))
        .any(|w| dutch_core_words.contains(&w))
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

fn extract_question_data(
    typo_map: &HashMap<String, String>,
    question_text: &str,
) -> Result<QuestionData, Box<dyn Error>> {
    let mut questioners = Vec::new();
    let mut topics = Vec::new();
    let mut respondents = Vec::new();
    let mut internal_ids = Vec::new();

    for capture in question_regex().captures_iter(question_text) {
        let questioner_raw = capture[1].trim().replace("- ", "");
        let questioner = typo_map
            .get(&questioner_raw)
            .cloned()
            .unwrap_or(questioner_raw);
        let respondent = capture[2].trim().to_string();
        let topic = capture
            .get(3)
            .or_else(|| capture.get(4))
            .or_else(|| capture.get(5))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();

        // Create the internal question ID format (Q56001734P).
        let internal_id = capture
            .get(4)
            .map(|m| format!("Q{}", m.as_str().trim()))
            .unwrap_or_default();

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
    ["wordt geopend", "wordt hervat"]
        .iter()
        .find_map(|phrase| extract_time_from_document(document, phrase).ok())
        .ok_or_else(|| "Could not extract start time from the document".into())
}

fn extract_end_time_from_document(document: &Html) -> Result<String, Box<dyn Error>> {
    [
        "De vergadering wordt gesloten",
        "De vergadering wordt geschorst",
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
    use std::fs::read_to_string;
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
        let content = read_to_string(&path).unwrap();
        let document = Html::parse_document(&content);
        let typo_map = HashMap::new();
        let questions = extract_questions(
            &document,
            56,
            82,
            &typo_map,
            "http://example.test",
            "sessions/56/meetings/plenary/56-82.html",
        )
        .await
        .unwrap();
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
        let notices = extract_notices(&parsed.blocks, 56, 9, "url", "cache")
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
}
