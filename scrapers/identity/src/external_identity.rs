use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use crawl::paths::data_dir;
use identity::external::{
    ExternalAliasRecord, ExternalContextRecord, ExternalKind, ExternalPersonRecord,
    alias_norms_for, classify_named_external, department_external_id, institutional_external_id,
    is_institutional_label, is_procedural_role, person_external_id, procedural_external_id,
};
use identity::normalize::clean_raw_name;
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::{Bucket, Resolution, Resolver};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

const SESSION_ID: &str = "56";

#[derive(Debug, Clone)]
struct Candidate {
    raw_name: String,
    bucket: String,
    context_id: String,
    context_label: String,
    meeting_id: String,
    meeting_kind: String,
    meeting_date: String,
    question_id: String,
    topics_nl: String,
    topics_fr: String,
    utterance_excerpt: String,
    source_url: String,
    cache_path: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let root = data_dir();
    let out_dir = root.join("identity");
    std::fs::create_dir_all(&out_dir)?;

    let person_resolver = Resolver::load(&root)?;
    let mut candidates = Vec::new();
    seed_institutional_and_roles(&mut candidates);
    collect_from_questions(&root, &person_resolver, &mut candidates)?;
    collect_from_utterances(&root, &person_resolver, &mut candidates)?;
    collect_from_dossiers(&root, &person_resolver, &mut candidates)?;
    collect_from_written_routes(&root, &mut candidates)?;

    let mut persons: HashMap<String, ExternalPersonRecord> = HashMap::new();
    let mut aliases: Vec<ExternalAliasRecord> = Vec::new();
    let mut alias_seen: HashSet<(String, String)> = HashSet::new();
    let mut contexts: Vec<ExternalContextRecord> = Vec::new();
    let mut context_seen: HashSet<String> = HashSet::new();

    for cand in candidates {
        let ext_id = classify_and_id(&cand.raw_name, &cand.bucket);
        let kind = classify_kind(&cand.raw_name, &cand.bucket, &ext_id);

        persons
            .entry(ext_id.clone())
            .or_insert_with(|| ExternalPersonRecord {
                external_person_id: ext_id.clone(),
                display_name: display_name_for(&cand.raw_name, &ext_id),
                kind: kind.as_str().to_string(),
                source: "bootstrap".to_string(),
                first_seen_bucket: cand.bucket.clone(),
                source_url: cand.source_url.clone(),
                cache_path: cand.cache_path.clone(),
            });

        for norm in alias_norms_for(&cand.raw_name) {
            if alias_seen.insert((norm.clone(), ext_id.clone())) {
                aliases.push(ExternalAliasRecord {
                    alias_norm: norm,
                    external_person_id: ext_id.clone(),
                    source: "bootstrap".to_string(),
                    confidence: "exact".to_string(),
                });
            }
        }

        let context_id = format!("{}_{}", ext_id, cand.context_id);
        if context_seen.insert(context_id.clone()) {
            contexts.push(ExternalContextRecord {
                context_id,
                external_person_id: ext_id,
                meeting_id: cand.meeting_id,
                meeting_kind: cand.meeting_kind,
                meeting_date: cand.meeting_date,
                question_id: cand.question_id,
                question_topics_nl: cand.topics_nl,
                question_topics_fr: cand.topics_fr,
                utterance_excerpt: cand.utterance_excerpt,
                source_url: cand.source_url,
                cache_path: cand.cache_path,
                raw_field: cand.raw_name,
            });
        }
    }

    let mut person_rows: Vec<ExternalPersonRecord> = persons.into_values().collect();
    person_rows.sort_by(|a, b| a.external_person_id.cmp(&b.external_person_id));
    aliases.sort_by(|a, b| a.alias_norm.cmp(&b.alias_norm));
    contexts.sort_by(|a, b| a.context_id.cmp(&b.context_id));

    let persons_path = out_dir.join("external_persons.parquet");
    write_external_persons(&persons_path, &person_rows)?;
    write_external_aliases(&out_dir.join("external_person_aliases.parquet"), &aliases)?;
    write_external_contexts(&out_dir.join("external_person_contexts.parquet"), &contexts)?;

    eprintln!(
        "Wrote {} external persons, {} aliases, {} contexts",
        person_rows.len(),
        aliases.len(),
        contexts.len()
    );
    Ok(())
}

fn seed_institutional_and_roles(candidates: &mut Vec<Candidate>) {
    let seeds = [
        ("Voorzitter", "speakers", "ext:role:voorzitter"),
        ("Le président", "speakers", "ext:role:le-president"),
        ("La présidente", "speakers", "ext:role:la-presidente"),
        (
            "Medewerker van de minister",
            "speakers",
            "ext:org:minister-staff",
        ),
        (
            "Medewerkster van de minister",
            "speakers",
            "ext:org:minister-staff",
        ),
        (
            "Greffe/Griffie (AUTEUR)",
            "authors",
            "ext:org:greffe-griffie",
        ),
        ("Chambre/Kamer (AUTEUR)", "authors", "ext:org:chambre-kamer"),
        (
            "Commission/Commissie (AUTEUR)",
            "authors",
            "ext:org:commission-commissie",
        ),
        (
            "Commissions/Commissies (AUTEUR)",
            "authors",
            "ext:org:commissions-commissies",
        ),
        ("(AUTEUR)", "authors", "ext:org:auteur"),
        ("Sénat/Senaat (AUTEUR)", "authors", "ext:org:senat-senaat"),
    ];
    for (name, bucket, ctx) in seeds {
        candidates.push(Candidate {
            raw_name: name.to_string(),
            bucket: bucket.to_string(),
            context_id: ctx.to_string(),
            context_label: "seed".to_string(),
            meeting_id: String::new(),
            meeting_kind: String::new(),
            meeting_date: String::new(),
            question_id: String::new(),
            topics_nl: String::new(),
            topics_fr: String::new(),
            utterance_excerpt: String::new(),
            source_url: String::new(),
            cache_path: String::new(),
        });
    }
}

fn collect_from_questions(
    root: &Path,
    resolver: &Resolver,
    candidates: &mut Vec<Candidate>,
) -> Result<(), Box<dyn Error>> {
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for (meeting_kind, rel) in [
        ("plenary", "plenary/questions.parquet"),
        ("commission", "commission/questions.parquet"),
    ] {
        let path = root.join(format!("sessions/{SESSION_ID}/{rel}"));
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let question_ids = read_string_column(&batch, "question_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let respondents = read_string_column(&batch, "respondents")?;
            let topics_nl = read_string_column(&batch, "topics_nl")?;
            let topics_fr = read_string_column(&batch, "topics_fr")?;
            let questioners = read_string_column(&batch, "questioners")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                let qid = format!(
                    "{SESSION_ID}_{meeting_kind}_{}_{}",
                    meeting_ids[i], question_ids[i]
                );
                for name in split_csv(&respondents[i]) {
                    if should_add_external(resolver, &name, Bucket::Respondent) {
                        let key = (name.clone(), "respondents".to_string());
                        if seen.insert(key) {
                            candidates.push(Candidate {
                                raw_name: name,
                                bucket: "respondents".to_string(),
                                context_id: qid.clone(),
                                context_label: format!("question {qid}"),
                                meeting_id: meeting_ids[i].clone(),
                                meeting_kind: meeting_kind.to_string(),
                                meeting_date: String::new(),
                                question_id: qid.clone(),
                                topics_nl: topics_nl[i].clone(),
                                topics_fr: topics_fr[i].clone(),
                                utterance_excerpt: String::new(),
                                source_url: source_urls[i].clone(),
                                cache_path: cache_paths[i].clone(),
                            });
                        }
                    }
                }

                for name in split_csv(&questioners[i]) {
                    let cleaned = clean_raw_name(&name);
                    if cleaned.is_empty() {
                        continue;
                    }
                    if should_add_external(resolver, &cleaned, Bucket::Questioner) {
                        let key = (cleaned.clone(), "questioners".to_string());
                        if seen.insert(key) {
                            candidates.push(Candidate {
                                raw_name: cleaned,
                                bucket: "questioners".to_string(),
                                context_id: qid.clone(),
                                context_label: format!("question {qid}"),
                                meeting_id: meeting_ids[i].clone(),
                                meeting_kind: meeting_kind.to_string(),
                                meeting_date: String::new(),
                                question_id: qid.clone(),
                                topics_nl: topics_nl[i].clone(),
                                topics_fr: topics_fr[i].clone(),
                                utterance_excerpt: String::new(),
                                source_url: source_urls[i].clone(),
                                cache_path: cache_paths[i].clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn collect_from_utterances(
    root: &Path,
    resolver: &Resolver,
    candidates: &mut Vec<Candidate>,
) -> Result<(), Box<dyn Error>> {
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for (meeting_kind, rel) in [
        ("plenary", "plenary/utterances.parquet"),
        ("commission", "commission/utterances.parquet"),
    ] {
        let path = root.join(format!("sessions/{SESSION_ID}/{rel}"));
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let utterance_ids = read_string_column(&batch, "utterance_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let raw_speakers = read_string_column(&batch, "raw_speaker")?;
            let texts = read_string_column(&batch, "text")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                let speaker = clean_raw_name(&raw_speakers[i]);
                if speaker.is_empty() {
                    continue;
                }
                if should_add_external(resolver, &speaker, Bucket::Speaker) {
                    let key = (speaker.clone(), "speakers".to_string());
                    if seen.insert(key) {
                        candidates.push(Candidate {
                            raw_name: speaker,
                            bucket: "speakers".to_string(),
                            context_id: utterance_ids[i].clone(),
                            context_label: format!("utterance {}", utterance_ids[i]),
                            meeting_id: meeting_ids[i].clone(),
                            meeting_kind: meeting_kind.to_string(),
                            meeting_date: String::new(),
                            question_id: String::new(),
                            topics_nl: String::new(),
                            topics_fr: String::new(),
                            utterance_excerpt: clip(&texts[i], 500),
                            source_url: source_urls[i].clone(),
                            cache_path: cache_paths[i].clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn collect_from_dossiers(
    root: &Path,
    resolver: &Resolver,
    candidates: &mut Vec<Candidate>,
) -> Result<(), Box<dyn Error>> {
    let mut seen: HashSet<(String, String)> = HashSet::new();

    let dossiers_path = root.join(format!("sessions/{SESSION_ID}/dossiers.parquet"));
    if dossiers_path.exists() {
        for batch in read_all_rows(&dossiers_path)? {
            let session_ids = read_string_column(&batch, "session_id")?;
            let dossier_ids = read_string_column(&batch, "id")?;
            let authors = read_string_column(&batch, "authors")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                let target_id = format!("{}/{}", session_ids[i], dossier_ids[i]);
                ingest_author_candidates(
                    resolver,
                    &authors[i],
                    &target_id,
                    &format!("dossier {}", dossier_ids[i]),
                    &source_urls[i],
                    &cache_paths[i],
                    candidates,
                    &mut seen,
                );
            }
        }
    }

    let subdocs_path = root.join(format!("sessions/{SESSION_ID}/subdocuments.parquet"));
    if subdocs_path.exists() {
        for batch in read_all_rows(&subdocs_path)? {
            let dossier_ids = read_string_column(&batch, "dossier_id")?;
            let doc_ids = read_string_column(&batch, "id")?;
            let authors = read_string_column(&batch, "authors")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                ingest_author_candidates(
                    resolver,
                    &authors[i],
                    &doc_ids[i],
                    &format!("dossier {} doc {}", dossier_ids[i], doc_ids[i]),
                    &source_urls[i],
                    &cache_paths[i],
                    candidates,
                    &mut seen,
                );
            }
        }
    }

    Ok(())
}

fn collect_from_written_routes(
    root: &Path,
    candidates: &mut Vec<Candidate>,
) -> Result<(), Box<dyn Error>> {
    let path = root.join(format!("sessions/{SESSION_ID}/written/routes.parquet"));
    if !path.exists() {
        return Ok(());
    }
    let mut seen: HashSet<String> = HashSet::new();
    for batch in read_all_rows(&path)? {
        let question_ids = read_string_column(&batch, "question_id")?;
        let deptnums = read_string_column(&batch, "deptnum")?;
        let dept_nl = read_string_column(&batch, "dept_title_nl")?;
        let dept_fr = read_string_column(&batch, "dept_title_fr")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            if deptnums[i].is_empty() || !seen.insert(deptnums[i].clone()) {
                continue;
            }
            let display = if !dept_nl[i].is_empty() {
                dept_nl[i].clone()
            } else {
                dept_fr[i].clone()
            };
            let raw_name = format!("deptnum:{}|{}", deptnums[i], display);
            candidates.push(Candidate {
                raw_name: raw_name.clone(),
                bucket: "written_dept".to_string(),
                context_id: department_external_id(&deptnums[i]),
                context_label: format!("written route {}", question_ids[i]),
                meeting_id: String::new(),
                meeting_kind: String::new(),
                meeting_date: String::new(),
                question_id: question_ids[i].clone(),
                topics_nl: dept_nl[i].clone(),
                topics_fr: dept_fr[i].clone(),
                utterance_excerpt: String::new(),
                source_url: source_urls[i].clone(),
                cache_path: cache_paths[i].clone(),
            });
            if !dept_fr[i].is_empty() && dept_fr[i] != dept_nl[i] {
                let fr_raw = format!("deptnum:{}|{}", deptnums[i], dept_fr[i]);
                candidates.push(Candidate {
                    raw_name: fr_raw,
                    bucket: "written_dept".to_string(),
                    context_id: department_external_id(&deptnums[i]),
                    context_label: format!("written route alias {}", question_ids[i]),
                    meeting_id: String::new(),
                    meeting_kind: String::new(),
                    meeting_date: String::new(),
                    question_id: question_ids[i].clone(),
                    topics_nl: dept_nl[i].clone(),
                    topics_fr: dept_fr[i].clone(),
                    utterance_excerpt: String::new(),
                    source_url: source_urls[i].clone(),
                    cache_path: cache_paths[i].clone(),
                });
            }
        }
    }
    Ok(())
}

fn ingest_author_candidates(
    resolver: &Resolver,
    authors_csv: &str,
    context_id: &str,
    context_label: &str,
    source_url: &str,
    cache_path: &str,
    candidates: &mut Vec<Candidate>,
    seen: &mut HashSet<(String, String)>,
) {
    for name in split_csv(authors_csv) {
        if is_government_author(&name) {
            continue;
        }
        if should_add_external(resolver, &name, Bucket::Author) {
            let key = (name.clone(), "authors".to_string());
            if seen.insert(key) {
                candidates.push(Candidate {
                    raw_name: name,
                    bucket: "authors".to_string(),
                    context_id: context_id.to_string(),
                    context_label: context_label.to_string(),
                    meeting_id: String::new(),
                    meeting_kind: String::new(),
                    meeting_date: String::new(),
                    question_id: String::new(),
                    topics_nl: String::new(),
                    topics_fr: String::new(),
                    utterance_excerpt: String::new(),
                    source_url: source_url.to_string(),
                    cache_path: cache_path.to_string(),
                });
            }
        }
    }
}

fn is_government_author(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    lower == "government" || lower.contains("gouvernment") || lower.contains("regering")
}

fn should_add_external(resolver: &Resolver, raw: &str, bucket: Bucket) -> bool {
    let trimmed = clean_raw_name(raw);
    if trimmed.is_empty() || trimmed == "N ." {
        return false;
    }
    if is_institutional_label(&trimmed) || is_procedural_role(&trimmed) {
        return true;
    }
    !matches!(
        resolver.resolve_person(&trimmed, bucket),
        Resolution::Resolved(_)
    )
}

fn classify_and_id(raw: &str, bucket: &str) -> String {
    if bucket == "written_dept" {
        if let Some(rest) = raw.strip_prefix("deptnum:") {
            let deptnum = rest.split('|').next().unwrap_or(rest);
            return department_external_id(deptnum);
        }
    }
    if let Some((id, _)) = institutional_external_id(raw) {
        return id.to_string();
    }
    if let Some((id, _)) = procedural_external_id(raw) {
        return id.to_string();
    }
    person_external_id(raw)
}

fn classify_kind(raw: &str, bucket: &str, ext_id: &str) -> ExternalKind {
    if ext_id.starts_with("ext:org:") {
        return ExternalKind::Institutional;
    }
    if ext_id.starts_with("ext:role:") {
        return ExternalKind::ProceduralRole;
    }
    classify_named_external(raw, bucket)
}

fn display_name_for(raw: &str, ext_id: &str) -> String {
    if let Some(rest) = raw.strip_prefix("deptnum:") {
        if let Some((_dept, title)) = rest.split_once('|') {
            return title.to_string();
        }
    }
    if let Some((_, label)) = institutional_external_id(raw) {
        return label.to_string();
    }
    if let Some((_, label)) = procedural_external_id(raw) {
        return label.to_string();
    }
    raw.trim().to_string()
}

fn split_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(limit).collect();
        format!("{truncated}…")
    }
}

fn write_external_persons(
    path: &Path,
    rows: &[ExternalPersonRecord],
) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("external_person_id", false),
        utf8_field("display_name", false),
        utf8_field("kind", false),
        utf8_field("source", false),
        utf8_field("first_seen_bucket", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
    ]);
    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }
    write_parquet(
        path,
        schema,
        vec![
            col!(|r| r.external_person_id.clone()),
            col!(|r| r.display_name.clone()),
            col!(|r| r.kind.clone()),
            col!(|r| r.source.clone()),
            col!(|r| r.first_seen_bucket.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
        ],
    )?;
    Ok(())
}

fn write_external_aliases(path: &Path, rows: &[ExternalAliasRecord]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("alias_norm", false),
        utf8_field("external_person_id", false),
        utf8_field("source", false),
        utf8_field("confidence", false),
    ]);
    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }
    write_parquet(
        path,
        schema,
        vec![
            col!(|r| r.alias_norm.clone()),
            col!(|r| r.external_person_id.clone()),
            col!(|r| r.source.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )?;
    Ok(())
}

fn write_external_contexts(
    path: &Path,
    rows: &[ExternalContextRecord],
) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("context_id", false),
        utf8_field("external_person_id", false),
        utf8_field("meeting_id", false),
        utf8_field("meeting_kind", false),
        utf8_field("meeting_date", false),
        utf8_field("question_id", false),
        utf8_field("question_topics_nl", false),
        utf8_field("question_topics_fr", false),
        utf8_field("utterance_excerpt", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
        utf8_field("raw_field", false),
    ]);
    macro_rules! col {
        ($f:expr) => {
            Arc::new(StringArray::from(rows.iter().map($f).collect::<Vec<_>>())) as ArrayRef
        };
    }
    write_parquet(
        path,
        schema,
        vec![
            col!(|r| r.context_id.clone()),
            col!(|r| r.external_person_id.clone()),
            col!(|r| r.meeting_id.clone()),
            col!(|r| r.meeting_kind.clone()),
            col!(|r| r.meeting_date.clone()),
            col!(|r| r.question_id.clone()),
            col!(|r| r.question_topics_nl.clone()),
            col!(|r| r.question_topics_fr.clone()),
            col!(|r| r.utterance_excerpt.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.raw_field.clone()),
        ],
    )?;
    Ok(())
}
