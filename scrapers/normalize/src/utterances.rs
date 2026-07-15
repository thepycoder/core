use crate::common::{SESSION_ID, UnresolvedRow, dedupe_unresolved, reason_label};
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use crawl::utils::{ensure_question_id, normalize_site_ref};
use identity::actor_resolver::{ActorResolution, ActorResolver};
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
use identity::resolver::Bucket;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct UtteranceRow {
    pub utterance_id: String,
    pub session_id: String,
    pub meeting_id: String,
    pub meeting_kind: String,
    pub agenda_id: String,
    pub turn_number: String,
    pub seq: String,
    pub item_kind: String,
    pub item_id: String,
    pub question_ids: String,
    pub dossier_id: String,
    pub document_id: String,
    pub motion_id: String,
    pub vote_id: String,
    pub raw_speaker: String,
    pub speaker_role: String,
    pub speaker_entity_type: String,
    pub speaker_entity_id: String,
    pub text: String,
    pub language: String,
    pub block_start: String,
    pub block_end: String,
    pub source_section: String,
    pub source_url: String,
    pub cache_path: String,
    pub speaker_person_id: String,
    pub confidence: String,
}

pub struct UtteranceOutput {
    pub rows: Vec<UtteranceRow>,
    pub unresolved: Vec<UnresolvedRow>,
}

fn skip_speaker(raw: &str, speaker_role: &str) -> bool {
    let lower = raw.trim().to_lowercase();
    if lower.is_empty() || lower == "onbekend" || lower == "n ." {
        return true;
    }
    speaker_role == "chair" && matches!(lower.as_str(), "voorzitter" | "président" | "president")
}

pub fn normalize_utterances(
    data_dir: &Path,
    actor_resolver: &ActorResolver,
) -> Result<UtteranceOutput, Box<dyn Error>> {
    let mut rows = Vec::new();
    let mut unresolved = Vec::new();
    let (interpellation_targets, canonical_interpellation_ids) =
        load_interpellation_targets(data_dir)?;

    for (meeting_kind, rel_path) in [
        (
            "plenary",
            format!("sessions/{SESSION_ID}/plenary/utterances.parquet"),
        ),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/utterances.parquet"),
        ),
    ] {
        let path = data_dir.join(rel_path);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let utterance_ids = read_string_column(&batch, "utterance_id")?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let agenda_ids = read_string_column(&batch, "agenda_id")?;
            let turn_numbers = read_string_column(&batch, "turn_number")?;
            let seqs = read_string_column(&batch, "seq")?;
            let item_kinds = read_string_column(&batch, "item_kind")?;
            let item_ids = read_string_column(&batch, "item_id")?;
            let question_ids = read_string_column(&batch, "question_ids")?;
            let dossier_ids = read_string_column(&batch, "dossier_id")?;
            let document_ids = read_string_column(&batch, "document_id")?;
            let motion_ids = read_string_column(&batch, "motion_id")?;
            let vote_ids = read_string_column(&batch, "vote_id")?;
            let raw_speakers = read_string_column(&batch, "raw_speaker")?;
            let speaker_roles = read_string_column(&batch, "speaker_role")?;
            let texts = read_string_column(&batch, "text")?;
            let languages = read_string_column(&batch, "language")?;
            let block_starts = read_string_column(&batch, "block_start")?;
            let block_ends = read_string_column(&batch, "block_end")?;
            let source_sections = read_string_column(&batch, "source_section")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;

            for i in 0..batch.num_rows() {
                let speaker = raw_speakers[i].trim().to_string();
                let role = speaker_roles[i].trim().to_string();
                let (speaker_person_id, speaker_entity_type, speaker_entity_id, confidence) =
                    if skip_speaker(&speaker, &role) {
                        (String::new(), String::new(), String::new(), String::new())
                    } else {
                        let detail = actor_resolver.resolve_actor_detail(&speaker, Bucket::Speaker);
                        match detail.resolution {
                            ActorResolution::Person(person_id) => (
                                person_id.clone(),
                                "Person".to_string(),
                                person_id,
                                "exact".to_string(),
                            ),
                            ActorResolution::ExternalPerson(ext_id) => (
                                String::new(),
                                "ExternalPerson".to_string(),
                                ext_id,
                                "exact".to_string(),
                            ),
                            ActorResolution::Unresolved(reason) => {
                                unresolved.push(UnresolvedRow {
                                    raw_name: detail.raw_name,
                                    typo_corrected: detail.typo_corrected,
                                    norm_primary: detail.norm_primary,
                                    norm_reordered: detail.norm_reordered,
                                    reason: reason_label(&reason).to_string(),
                                    source_bucket: "speakers".to_string(),
                                    role: "speaker".to_string(),
                                    context_id: utterance_ids[i].clone(),
                                    context_label: format!("utterance {}", utterance_ids[i]),
                                    raw_field: speaker.clone(),
                                    source_url: source_urls[i].clone(),
                                    cache_path: cache_paths[i].clone(),
                                    ..UnresolvedRow::default()
                                });
                                (String::new(), String::new(), String::new(), String::new())
                            }
                        }
                    };

                let mut item_id = item_ids[i].clone();
                if item_kinds[i] == "interpellation" {
                    let actual_id = ensure_question_id(&session_ids[i], meeting_kind, &item_id);
                    let scope = (
                        session_ids[i].clone(),
                        meeting_kind.to_string(),
                        meeting_ids[i].clone(),
                    );
                    if !canonical_interpellation_ids.contains(&(
                        scope.0.clone(),
                        scope.1.clone(),
                        actual_id,
                    )) {
                        let mut candidates = HashSet::new();
                        for site_ref in question_ids[i].split(',') {
                            let site_ref = normalize_site_ref(site_ref);
                            if let Some(targets) = interpellation_targets.get(&(
                                scope.0.clone(),
                                scope.1.clone(),
                                scope.2.clone(),
                                site_ref,
                            )) {
                                candidates.extend(targets.iter().cloned());
                            }
                        }
                        if candidates.len() == 1 {
                            item_id = candidates.into_iter().next().unwrap_or(item_id);
                        }
                    }
                }

                rows.push(UtteranceRow {
                    utterance_id: utterance_ids[i].clone(),
                    session_id: session_ids[i].clone(),
                    meeting_id: meeting_ids[i].clone(),
                    meeting_kind: meeting_kind.to_string(),
                    agenda_id: agenda_ids[i].clone(),
                    turn_number: turn_numbers[i].clone(),
                    seq: seqs[i].clone(),
                    item_kind: item_kinds[i].clone(),
                    item_id,
                    question_ids: question_ids[i].clone(),
                    dossier_id: dossier_ids[i].clone(),
                    document_id: document_ids[i].clone(),
                    motion_id: motion_ids[i].clone(),
                    vote_id: vote_ids[i].clone(),
                    raw_speaker: speaker,
                    speaker_role: role,
                    speaker_entity_type,
                    speaker_entity_id,
                    text: texts[i].clone(),
                    language: languages[i].clone(),
                    block_start: block_starts[i].clone(),
                    block_end: block_ends[i].clone(),
                    source_section: source_sections[i].clone(),
                    source_url: source_urls[i].clone(),
                    cache_path: cache_paths[i].clone(),
                    speaker_person_id,
                    confidence,
                });
            }
        }
    }

    rows.sort_by(|a, b| {
        a.meeting_kind
            .cmp(&b.meeting_kind)
            .then(a.meeting_id.cmp(&b.meeting_id))
            .then(a.seq.cmp(&b.seq))
            .then(a.utterance_id.cmp(&b.utterance_id))
    });
    dedupe_unresolved(&mut unresolved);
    Ok(UtteranceOutput { rows, unresolved })
}

fn load_interpellation_targets(
    data_dir: &Path,
) -> Result<
    (
        HashMap<(String, String, String, String), HashSet<String>>,
        HashSet<(String, String, String)>,
    ),
    Box<dyn Error>,
> {
    let mut targets = HashMap::new();
    let mut canonical_ids = HashSet::new();
    for (meeting_kind, rel_path) in [
        (
            "plenary",
            format!("sessions/{SESSION_ID}/plenary/interpellations.parquet"),
        ),
        (
            "commission",
            format!("sessions/{SESSION_ID}/commission/interpellations.parquet"),
        ),
    ] {
        let path = data_dir.join(rel_path);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let ids = read_string_column(&batch, "interpellation_id")?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let internal_ids = read_string_column(&batch, "internal_ids")?;
            for i in 0..batch.num_rows() {
                let canonical_id = ensure_question_id(&session_ids[i], meeting_kind, &ids[i]);
                let scope = (
                    session_ids[i].clone(),
                    meeting_kind.to_string(),
                    meeting_ids[i].clone(),
                );
                canonical_ids.insert((scope.0.clone(), scope.1.clone(), canonical_id.clone()));
                for site_ref in internal_ids[i].split(',') {
                    let site_ref = normalize_site_ref(site_ref);
                    if !site_ref.is_empty() {
                        targets
                            .entry((scope.0.clone(), scope.1.clone(), scope.2.clone(), site_ref))
                            .or_insert_with(HashSet::new)
                            .insert(canonical_id.clone());
                    }
                }
            }
        }
    }
    Ok((targets, canonical_ids))
}

pub fn write_utterances(path: &Path, rows: &[UtteranceRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("utterance_id", false),
        utf8_field("session_id", false),
        utf8_field("meeting_id", false),
        utf8_field("meeting_kind", false),
        utf8_field("agenda_id", false),
        utf8_field("turn_number", false),
        utf8_field("seq", false),
        utf8_field("item_kind", false),
        utf8_field("item_id", false),
        utf8_field("question_ids", false),
        utf8_field("dossier_id", false),
        utf8_field("document_id", false),
        utf8_field("motion_id", false),
        utf8_field("vote_id", false),
        utf8_field("raw_speaker", false),
        utf8_field("speaker_role", false),
        utf8_field("speaker_entity_type", false),
        utf8_field("speaker_entity_id", false),
        utf8_field("text", false),
        utf8_field("language", false),
        utf8_field("block_start", false),
        utf8_field("block_end", false),
        utf8_field("source_section", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
        utf8_field("speaker_person_id", false),
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
            col!(|r| r.utterance_id.clone()),
            col!(|r| r.session_id.clone()),
            col!(|r| r.meeting_id.clone()),
            col!(|r| r.meeting_kind.clone()),
            col!(|r| r.agenda_id.clone()),
            col!(|r| r.turn_number.clone()),
            col!(|r| r.seq.clone()),
            col!(|r| r.item_kind.clone()),
            col!(|r| r.item_id.clone()),
            col!(|r| r.question_ids.clone()),
            col!(|r| r.dossier_id.clone()),
            col!(|r| r.document_id.clone()),
            col!(|r| r.motion_id.clone()),
            col!(|r| r.vote_id.clone()),
            col!(|r| r.raw_speaker.clone()),
            col!(|r| r.speaker_role.clone()),
            col!(|r| r.speaker_entity_type.clone()),
            col!(|r| r.speaker_entity_id.clone()),
            col!(|r| r.text.clone()),
            col!(|r| r.language.clone()),
            col!(|r| r.block_start.clone()),
            col!(|r| r.block_end.clone()),
            col!(|r| r.source_section.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.speaker_person_id.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}
