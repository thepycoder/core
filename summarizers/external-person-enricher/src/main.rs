use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use chrono::Utc;
use crawl::paths::data_dir;
use mistral_client::{
    RateLimiter, create_websearch_agent, hash_text, mistral_websearch_conversation,
    strip_json_fences,
};
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

const MODEL: &str = "mistral-large-latest";
const AGENT_NAME: &str = "partijgedrag-external-person-enricher";
const ENRICHMENT_PROMPT_VERSION: &str = "external-actor-v2";
const SAVE_EVERY: usize = 3;

#[derive(Debug, Clone, Serialize, Default)]
struct BioJson {
    identified_as: String,
    classification: String,
    role: String,
    affiliation: String,
    holder_name: String,
    holder_note: String,
    confidence: String,
    evidence_urls: Vec<String>,
}

impl<'de> Deserialize<'de> for BioJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            identified_as: Option<String>,
            #[serde(default)]
            classification: Option<String>,
            #[serde(default)]
            role: Option<String>,
            #[serde(default)]
            affiliation: Option<String>,
            #[serde(default)]
            holder_name: Option<String>,
            #[serde(default)]
            holder_note: Option<String>,
            #[serde(default)]
            confidence: Option<String>,
            #[serde(default)]
            evidence_urls: Option<Vec<String>>,
        }

        let raw = Raw::deserialize(deserializer)?;
        Ok(BioJson {
            identified_as: raw.identified_as.unwrap_or_default(),
            classification: raw.classification.unwrap_or_default(),
            role: raw.role.unwrap_or_default(),
            affiliation: raw.affiliation.unwrap_or_default(),
            holder_name: raw.holder_name.unwrap_or_default(),
            holder_note: raw.holder_note.unwrap_or_default(),
            confidence: raw
                .confidence
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "low".to_string()),
            evidence_urls: raw.evidence_urls.unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone)]
struct PersonRow {
    external_person_id: String,
    display_name: String,
    kind: String,
    source: String,
    first_seen_bucket: String,
    source_url: String,
    cache_path: String,
}

#[derive(Debug, Clone)]
struct ContextRow {
    context_id: String,
    external_person_id: String,
    meeting_id: String,
    meeting_kind: String,
    meeting_date: String,
    question_id: String,
    question_topics_nl: String,
    question_topics_fr: String,
    utterance_excerpt: String,
    source_url: String,
    cache_path: String,
    raw_field: String,
}

#[derive(Debug, Clone)]
struct CachedBio {
    external_person_id: String,
    input_hash: String,
    bio_nl: String,
    bio_json: String,
    model: String,
    search_queries: String,
    created_at: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let api_key = std::env::var("MISTRAL_API_TOKEN").map_err(|_| "MISTRAL_API_TOKEN not set")?;

    let root = data_dir();
    let identity_dir = root.join("identity");
    let persons_path = identity_dir.join("external_persons.parquet");
    let contexts_path = identity_dir.join("external_person_contexts.parquet");
    let output_path = identity_dir.join("external_person_bios.parquet");

    if !persons_path.exists() {
        eprintln!("No external_persons.parquet — run build-identity first");
        return Ok(());
    }

    let mut persons = load_persons(&persons_path)?;
    let contexts = load_contexts(&contexts_path)?;
    let mut by_person: HashMap<String, Vec<ContextRow>> = HashMap::new();
    for ctx in contexts {
        by_person
            .entry(ctx.external_person_id.clone())
            .or_default()
            .push(ctx);
    }

    let mut cache = load_existing_bios(&output_path);
    let client = Client::new();
    let rate_limiter = RateLimiter::new(0.5);
    let mut processed = 0usize;

    let agent_id = match std::env::var("MISTRAL_WEBSEARCH_AGENT_ID") {
        Ok(id) if !id.is_empty() => {
            eprintln!("[enricher] using agent {id}");
            id
        }
        _ => {
            let Some(id) = create_websearch_agent(
                &client,
                &api_key,
                MODEL,
                AGENT_NAME,
                agent_instructions(),
                &rate_limiter,
            )
            .await
            else {
                return Err("failed to create Mistral web_search agent".into());
            };
            eprintln!("[enricher] created web_search agent {id}");
            id
        }
    };

    for person in &mut persons {
        let ctxs = by_person.get(&person.external_person_id);
        let bundle = build_context_bundle(person, ctxs);
        let input_hash = hash_text(&format!("{ENRICHMENT_PROMPT_VERSION}\n{bundle}"));
        let cache_key = format!("{}:{input_hash}", person.external_person_id);
        if let Some(cached) = cache.get(&cache_key) {
            apply_enriched_classification(person, &parse_bio_response(&cached.bio_json));
            continue;
        }

        let search_hints = build_search_hints(person, ctxs);
        let user = user_prompt(person, &bundle, &search_hints);

        let Some(result) =
            mistral_websearch_conversation(&client, &api_key, &agent_id, &user, &rate_limiter)
                .await
        else {
            eprintln!(
                "[enricher] Mistral failed for {}",
                person.external_person_id
            );
            continue;
        };

        let mut bio_json = parse_bio_response(&result.text);
        for url in result.reference_urls {
            if !bio_json.evidence_urls.iter().any(|u| u == &url) {
                bio_json.evidence_urls.push(url);
            }
        }

        let bio_nl = if !bio_json.holder_note.is_empty() {
            bio_json.holder_note.clone()
        } else if !bio_json.identified_as.is_empty() {
            let mut parts = vec![bio_json.identified_as.clone()];
            if !bio_json.role.is_empty() {
                parts.push(bio_json.role.clone());
            }
            if !bio_json.affiliation.is_empty() {
                parts.push(format!("({})", bio_json.affiliation));
            }
            parts.join(" — ")
        } else {
            clip(&result.text, 300)
        };

        let search_queries = if result.web_search_calls > 0 {
            format!(
                "mistral:web_search({}); {}",
                result.web_search_calls,
                search_hints.join("; ")
            )
        } else {
            format!("mistral:no_web_search; {}", search_hints.join("; "))
        };

        cache.insert(
            cache_key,
            CachedBio {
                external_person_id: person.external_person_id.clone(),
                input_hash: input_hash.clone(),
                bio_nl,
                bio_json: serde_json::to_string(&bio_json)?,
                model: MODEL.to_string(),
                search_queries,
                created_at: Utc::now().to_rfc3339(),
            },
        );
        apply_enriched_classification(person, &bio_json);

        processed += 1;
        eprintln!(
            "[enricher] enriched {} ({})",
            person.display_name, person.external_person_id
        );

        if processed % SAVE_EVERY == 0 {
            save_bios(&output_path, &cache)?;
        }
    }

    save_bios(&output_path, &cache)?;
    save_persons(&persons_path, &persons)?;
    eprintln!("[enricher] wrote {} bios ({} new)", cache.len(), processed);
    Ok(())
}

fn agent_instructions() -> &'static str {
    "Je bent een onderzoeksassistent voor het Belgische federale parlement (Kamer van \
Volksvertegenwoordigers). Je identificeert personen en rollen die in parlementaire bronnen \
voorkomen maar geen Kamerlid zijn.\n\n\
Gebruik web_search wanneer je actuele of externe informatie nodig hebt (minister, voorzitter, \
expert, institutionele auteur).\n\n\
Classificeer personen op de datum in de broncontext. Gebruik een huidige functie niet als \
vervanging voor de historische rol op die vergaderdatum.\n\n\
Antwoord ALLEEN met geldig JSON, geen markdown fences, met EXACT deze velden:\n\
{\n  \
\"identified_as\": \"echte naam of org-label\",\n  \
\"classification\": \"minister|state_secretary|expert|other\",\n  \
\"role\": \"minister van … / voorzitter / griffier / expert / …\",\n  \
\"affiliation\": \"federale regering / Kamer / Senaat / …\",\n  \
\"holder_name\": \"alleen voor procedurele rollen zoals Voorzitter: wie was het op dat moment; lege string als niet van toepassing\",\n  \
\"holder_note\": \"1-2 zinnen bio in het Nederlands\",\n  \
\"confidence\": \"high|medium|low\",\n  \
\"evidence_urls\": [\"url1\", \"url2\"]\n\
}\n\n\
Gebruik lege strings in plaats van null. Wees voorzichtig bij lage confidence."
}

fn apply_enriched_classification(person: &mut PersonRow, bio: &BioJson) {
    if !person.external_person_id.starts_with("ext:person:") {
        return;
    }
    match bio.classification.trim().to_lowercase().as_str() {
        "minister" | "state_secretary" | "expert" | "other" => {
            person.kind = bio.classification.trim().to_lowercase();
        }
        _ => {}
    }
}

fn user_prompt(person: &PersonRow, bundle: &str, search_hints: &[String]) -> String {
    format!(
        "Identificeer deze actor in het Belgische federale parlement (wetgevende zitting 56).\n\n\
Acteur:\n- id: {}\n- label in bron: {}\n- soort: {}\n\n\
Lokale context uit onze data:\n{bundle}\n\n\
Zoeksuggesties (gebruik web_search indien nodig): {}\n\n\
Gebruik meeting-datum en vraagonderwerp om Voorzitter of experts te identificeren.",
        person.external_person_id,
        person.display_name,
        person.kind,
        search_hints.join(", "),
    )
}

fn build_context_bundle(person: &PersonRow, ctxs: Option<&Vec<ContextRow>>) -> String {
    let mut lines = vec![
        format!("display_name: {}", person.display_name),
        format!("kind: {}", person.kind),
    ];
    if let Some(ctxs) = ctxs {
        for (i, ctx) in ctxs.iter().take(5).enumerate() {
            lines.push(format!("--- context {i} ---"));
            if !ctx.meeting_kind.is_empty() {
                lines.push(format!(
                    "meeting: {} {} ({})",
                    ctx.meeting_kind, ctx.meeting_id, ctx.meeting_date
                ));
            }
            if !ctx.question_id.is_empty() {
                lines.push(format!("question_id: {}", ctx.question_id));
            }
            if !ctx.question_topics_nl.is_empty() {
                lines.push(format!("topics_nl: {}", ctx.question_topics_nl));
            }
            if !ctx.utterance_excerpt.is_empty() {
                lines.push(format!("excerpt: {}", ctx.utterance_excerpt));
            }
            if !ctx.source_url.is_empty() {
                lines.push(format!("source_url: {}", ctx.source_url));
            }
        }
    }
    lines.join("\n")
}

fn build_search_hints(person: &PersonRow, ctxs: Option<&Vec<ContextRow>>) -> Vec<String> {
    let mut queries = Vec::new();
    if person.kind == "procedural_role" {
        if let Some(ctxs) = ctxs {
            if let Some(ctx) = ctxs.first() {
                queries.push(format!(
                    "Belgian Chamber of Representatives voorzitter meeting {} {}",
                    ctx.meeting_kind, ctx.meeting_id
                ));
            }
        }
        queries
            .push("Belgische Kamer van Volksvertegenwoordigers voorzitter 2024 2025".to_string());
    } else if person.kind == "institutional" {
        queries.push(format!(
            "{} Belgian federal parliament",
            person.display_name
        ));
    } else {
        queries.push(format!(
            "{} Belgian federal minister government Belgium",
            person.display_name
        ));
        if let Some(ctxs) = ctxs {
            if let Some(ctx) = ctxs.first() {
                if !ctx.question_topics_nl.is_empty() {
                    queries.push(format!(
                        "{} {} Belgian parliament",
                        person.display_name,
                        clip(&ctx.question_topics_nl, 80)
                    ));
                }
            }
        }
    }
    queries.truncate(2);
    queries
}

fn parse_bio_response(raw: &str) -> BioJson {
    let json_str = strip_json_fences(raw);
    serde_json::from_str(&json_str).unwrap_or_else(|e| {
        eprintln!("[enricher] JSON parse failed: {e}");
        BioJson {
            holder_note: clip(raw, 300),
            confidence: "low".to_string(),
            ..Default::default()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_null_string_fields() {
        let raw = r#"{
  "identified_as": "Hadja Lahbib",
  "role": "minister van Buitenlandse Zaken",
  "affiliation": "federale regering",
  "holder_name": null,
  "holder_note": "Belgische minister.",
  "confidence": "high",
  "evidence_urls": []
}"#;
        let bio = parse_bio_response(raw);
        assert_eq!(bio.identified_as, "Hadja Lahbib");
        assert_eq!(bio.holder_name, "");
        assert_eq!(bio.confidence, "high");
    }

    #[test]
    fn applies_only_known_person_classifications() {
        let mut person = PersonRow {
            external_person_id: "ext:person:example".to_string(),
            display_name: "Example".to_string(),
            kind: "other".to_string(),
            source: "bootstrap".to_string(),
            first_seen_bucket: "speakers".to_string(),
            source_url: String::new(),
            cache_path: String::new(),
        };
        apply_enriched_classification(
            &mut person,
            &BioJson {
                classification: "expert".to_string(),
                ..Default::default()
            },
        );
        assert_eq!(person.kind, "expert");
        apply_enriched_classification(
            &mut person,
            &BioJson {
                classification: "ministerial".to_string(),
                ..Default::default()
            },
        );
        assert_eq!(person.kind, "expert");
    }
}

fn clip(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

fn load_persons(path: &Path) -> Result<Vec<PersonRow>, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch?;
        let ids = col_str(&batch, "external_person_id");
        let names = col_str(&batch, "display_name");
        let kinds = col_str(&batch, "kind");
        let sources = col_str(&batch, "source");
        let buckets = col_str(&batch, "first_seen_bucket");
        let source_urls = col_str(&batch, "source_url");
        let cache_paths = col_str(&batch, "cache_path");
        for i in 0..batch.num_rows() {
            out.push(PersonRow {
                external_person_id: ids.value(i).to_string(),
                display_name: names.value(i).to_string(),
                kind: kinds.value(i).to_string(),
                source: sources.value(i).to_string(),
                first_seen_bucket: buckets.value(i).to_string(),
                source_url: source_urls.value(i).to_string(),
                cache_path: cache_paths.value(i).to_string(),
            });
        }
    }
    Ok(out)
}

fn save_persons(path: &Path, persons: &[PersonRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        Field::new("external_person_id", DataType::Utf8, false),
        Field::new("display_name", DataType::Utf8, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("source", DataType::Utf8, false),
        Field::new("first_seen_bucket", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("cache_path", DataType::Utf8, false),
    ]);
    let cols: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(
            persons
                .iter()
                .map(|p| p.external_person_id.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            persons
                .iter()
                .map(|p| p.display_name.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            persons.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            persons
                .iter()
                .map(|p| p.source.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            persons
                .iter()
                .map(|p| p.first_seen_bucket.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            persons
                .iter()
                .map(|p| p.source_url.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            persons
                .iter()
                .map(|p| p.cache_path.as_str())
                .collect::<Vec<_>>(),
        )),
    ];
    let batch = RecordBatch::try_new(Arc::new(schema), cols)?;
    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

fn load_contexts(path: &Path) -> Result<Vec<ContextRow>, Box<dyn Error>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch?;
        let context_ids = col_str(&batch, "context_id");
        let ext_ids = col_str(&batch, "external_person_id");
        let meeting_ids = col_str(&batch, "meeting_id");
        let meeting_kinds = col_str(&batch, "meeting_kind");
        let meeting_dates = col_str(&batch, "meeting_date");
        let question_ids = col_str(&batch, "question_id");
        let topics_nl = col_str(&batch, "question_topics_nl");
        let topics_fr = col_str(&batch, "question_topics_fr");
        let excerpts = col_str(&batch, "utterance_excerpt");
        let source_urls = col_str(&batch, "source_url");
        let cache_paths = col_str(&batch, "cache_path");
        let raw_fields = col_str(&batch, "raw_field");
        for i in 0..batch.num_rows() {
            out.push(ContextRow {
                context_id: context_ids.value(i).to_string(),
                external_person_id: ext_ids.value(i).to_string(),
                meeting_id: meeting_ids.value(i).to_string(),
                meeting_kind: meeting_kinds.value(i).to_string(),
                meeting_date: meeting_dates.value(i).to_string(),
                question_id: question_ids.value(i).to_string(),
                question_topics_nl: topics_nl.value(i).to_string(),
                question_topics_fr: topics_fr.value(i).to_string(),
                utterance_excerpt: excerpts.value(i).to_string(),
                source_url: source_urls.value(i).to_string(),
                cache_path: cache_paths.value(i).to_string(),
                raw_field: raw_fields.value(i).to_string(),
            });
        }
    }
    Ok(out)
}

fn load_existing_bios(path: &Path) -> HashMap<String, CachedBio> {
    let mut map = HashMap::new();
    if !path.exists() {
        return map;
    }
    let file = File::open(path).unwrap();
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .unwrap()
        .build()
        .unwrap();
    for batch in reader {
        let batch = batch.unwrap();
        let ext_ids = col_str(&batch, "external_person_id");
        let hashes = col_str(&batch, "input_hash");
        let bios = col_str(&batch, "bio_nl");
        let jsons = col_str(&batch, "bio_json");
        let models = col_str(&batch, "model");
        let queries = col_str(&batch, "search_queries");
        let created = col_str(&batch, "created_at");
        for i in 0..batch.num_rows() {
            let key = format!("{}:{}", ext_ids.value(i), hashes.value(i));
            map.insert(
                key,
                CachedBio {
                    external_person_id: ext_ids.value(i).to_string(),
                    input_hash: hashes.value(i).to_string(),
                    bio_nl: bios.value(i).to_string(),
                    bio_json: jsons.value(i).to_string(),
                    model: models.value(i).to_string(),
                    search_queries: queries.value(i).to_string(),
                    created_at: created.value(i).to_string(),
                },
            );
        }
    }
    map
}

fn save_bios(path: &Path, cache: &HashMap<String, CachedBio>) -> Result<(), Box<dyn Error>> {
    let mut rows: Vec<&CachedBio> = cache.values().collect();
    rows.sort_by(|a, b| a.external_person_id.cmp(&b.external_person_id));

    let schema = Schema::new(vec![
        Field::new("external_person_id", DataType::Utf8, false),
        Field::new("input_hash", DataType::Utf8, false),
        Field::new("bio_nl", DataType::Utf8, false),
        Field::new("bio_json", DataType::Utf8, false),
        Field::new("model", DataType::Utf8, false),
        Field::new("search_queries", DataType::Utf8, false),
        Field::new("created_at", DataType::Utf8, false),
    ]);

    let cols: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.external_person_id.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.input_hash.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter().map(|r| r.bio_nl.as_str()).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter().map(|r| r.bio_json.as_str()).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter().map(|r| r.model.as_str()).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.search_queries.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| r.created_at.as_str())
                .collect::<Vec<_>>(),
        )),
    ];

    let batch = RecordBatch::try_new(Arc::new(schema), cols)?;
    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

fn col_str<'a>(batch: &'a RecordBatch, name: &str) -> &'a StringArray {
    batch
        .column_by_name(name)
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap()
}
