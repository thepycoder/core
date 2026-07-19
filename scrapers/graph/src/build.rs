use crate::provenance::{ArtifactEntry, register_artifact, register_artifact_with_hash};
use crate::written_qa::{load_answer_nodes, load_written_qa_edges, load_written_question_nodes};
use arrow::array::{ArrayRef, Float64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use crawl::paths::cache_dir;
use crawl::report_blocks::read_report_html;
use crawl::utils::{ensure_question_id, is_flwb_document_id, normalize_site_ref};
use crawl::{
    BLOCK_PARSER_VERSION, VOTE_EXTRACTOR_VERSION, content_hash, content_hash_bytes,
    read_cache_metadata,
};
use identity::parquet_io::{
    read_all_rows, read_f64_column, read_string_column, utf8_field, write_parquet,
};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;

pub const SESSION_ID: &str = "56";

#[derive(Debug, Clone)]
pub struct NodeRow {
    pub node_type: String,
    pub node_id: String,
    pub label: String,
    pub source_artifact_id: String,
    pub source_url: String,
    pub cache_path: String,
}

#[derive(Debug, Clone)]
pub struct EdgeRow {
    pub edge_type: String,
    pub from_type: String,
    pub from_id: String,
    pub to_type: String,
    pub to_id: String,
    pub role: String,
    pub source_artifact_id: String,
    pub source_url: String,
    pub cache_path: String,
    pub confidence: f64,
    pub properties_json: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactRow {
    pub source_artifact_id: String,
    pub source_url: String,
    pub cache_path: String,
    pub source_content_hash: String,
    pub block_parser_version: String,
    pub extractor_version: String,
    pub scraped_at: String,
}

pub struct GraphBuild {
    pub nodes: Vec<NodeRow>,
    pub edges: Vec<EdgeRow>,
    pub artifacts: Vec<ArtifactRow>,
}

pub fn build_graph(data_dir: &Path) -> Result<GraphBuild, Box<dyn Error>> {
    let mut nodes: Vec<NodeRow> = Vec::new();
    let mut edges: Vec<EdgeRow> = Vec::new();
    let mut artifact_registry: HashMap<String, ArtifactEntry> = HashMap::new();
    let mut node_seen: HashMap<(String, String), ()> = HashMap::new();

    let mut add_node =
        |node_type: &str, node_id: &str, label: &str, source_url: &str, cache_path: &str| {
            if node_id.is_empty() {
                return;
            }
            if node_seen
                .insert((node_type.to_string(), node_id.to_string()), ())
                .is_some()
            {
                return;
            }
            nodes.push(NodeRow {
                node_type: node_type.to_string(),
                node_id: node_id.to_string(),
                label: label.to_string(),
                source_artifact_id: crate::provenance::artifact_id(source_url, cache_path),
                source_url: source_url.to_string(),
                cache_path: cache_path.to_string(),
            });
        };

    let mut add_edge = |edge_type: &str,
                        from_type: &str,
                        from_id: &str,
                        to_type: &str,
                        to_id: &str,
                        role: &str,
                        source_url: &str,
                        cache_path: &str,
                        confidence: f64| {
        if from_id.is_empty() || to_id.is_empty() {
            return;
        }
        let artifact = register_artifact(&mut artifact_registry, source_url, cache_path);
        edges.push(EdgeRow {
            edge_type: edge_type.to_string(),
            from_type: from_type.to_string(),
            from_id: from_id.to_string(),
            to_type: to_type.to_string(),
            to_id: to_id.to_string(),
            role: role.to_string(),
            source_artifact_id: artifact,
            source_url: source_url.to_string(),
            cache_path: cache_path.to_string(),
            confidence,
            properties_json: "{}".to_string(),
        });
    };

    load_identity_nodes(data_dir, &mut add_node)?;
    load_staging_nodes(data_dir, &mut add_node)?;
    load_written_question_nodes(data_dir, &mut add_node)?;
    load_normalized_nodes(data_dir, &mut add_node)?;
    load_answer_nodes(data_dir, &mut add_node)?;

    load_membership_edges(data_dir, &mut add_edge)?;
    load_has_result_edges(data_dir, &mut add_edge)?;
    load_vote_cast_edges(data_dir, &mut add_edge)?;
    load_voted_on_edges(data_dir, &mut add_edge)?;
    load_submitted_edges(data_dir, &mut add_edge)?;
    load_tagged_with_edges(data_dir, &mut add_node, &mut add_edge)?;
    load_authored_edges(data_dir, &mut add_edge)?;
    load_asked_edges(data_dir, &mut add_edge)?;
    load_answered_edges(data_dir, &mut add_edge)?;
    load_interpellated_edges(data_dir, &mut add_edge)?;
    load_interpellation_responded_edges(data_dir, &mut add_edge)?;
    load_invited_edges(data_dir, &mut add_edge)?;
    load_proceeding_meeting_edges(data_dir, &mut add_edge)?;
    load_agenda_item_edges(data_dir, &mut add_edge)?;
    load_holds_role_edges(data_dir, &mut add_edge)?;
    let site_ref_nodes = build_site_ref_node_lookup(data_dir, &node_seen)?;
    load_spoke_and_part_of_edges(data_dir, &site_ref_nodes, &node_seen, &mut add_edge)?;
    load_written_qa_edges(data_dir, &mut edges, &mut artifact_registry)?;

    for node in &nodes {
        let registered =
            register_artifact(&mut artifact_registry, &node.source_url, &node.cache_path);
        if registered != node.source_artifact_id {
            return Err(format!(
                "node artifact mismatch for {} {}",
                node.node_type, node.node_id
            )
            .into());
        }
    }

    seed_transform_hashes_from_normalized(data_dir, &mut artifact_registry)?;

    nodes.sort_by(|a, b| {
        a.node_type
            .cmp(&b.node_type)
            .then(a.node_id.cmp(&b.node_id))
    });
    edges.sort_by(|a, b| {
        a.edge_type
            .cmp(&b.edge_type)
            .then(a.from_id.cmp(&b.from_id))
            .then(a.to_id.cmp(&b.to_id))
            .then(a.role.cmp(&b.role))
    });

    let extractor_version = format!("graph_{}", env!("CARGO_PKG_VERSION"));
    let scraped_at_index = load_scraped_at_index(data_dir);
    let mut artifacts: Vec<ArtifactRow> = artifact_registry
        .into_iter()
        .map(|(id, entry)| {
            let content_hash = entry
                .transform_content_hash
                .clone()
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| source_content_hash(&entry.cache_path));
            ArtifactRow {
                source_artifact_id: id,
                source_url: entry.source_url,
                source_content_hash: content_hash,
                block_parser_version: if entry.cache_path.contains("/meetings/plenary/")
                    && entry.cache_path.ends_with(".html")
                {
                    BLOCK_PARSER_VERSION.to_string()
                } else {
                    String::new()
                },
                scraped_at: resolve_scraped_at(&entry.cache_path, &scraped_at_index),
                cache_path: entry.cache_path,
                extractor_version: extractor_version.clone(),
            }
        })
        .collect();
    artifacts.sort_by(|a, b| a.source_artifact_id.cmp(&b.source_artifact_id));

    Ok(GraphBuild {
        nodes,
        edges,
        artifacts,
    })
}

pub(crate) fn register_artifact_edge(
    edges: &mut Vec<EdgeRow>,
    artifact_registry: &mut HashMap<String, ArtifactEntry>,
    edge_type: &str,
    from_type: &str,
    from_id: &str,
    to_type: &str,
    to_id: &str,
    role: &str,
    source_url: &str,
    cache_path: &str,
    confidence: f64,
    properties_json: &str,
) {
    register_artifact_edge_with_hash(
        edges,
        artifact_registry,
        edge_type,
        from_type,
        from_id,
        to_type,
        to_id,
        role,
        source_url,
        cache_path,
        None,
        confidence,
        properties_json,
    );
}

pub(crate) fn register_artifact_edge_with_hash(
    edges: &mut Vec<EdgeRow>,
    artifact_registry: &mut HashMap<String, ArtifactEntry>,
    edge_type: &str,
    from_type: &str,
    from_id: &str,
    to_type: &str,
    to_id: &str,
    role: &str,
    source_url: &str,
    cache_path: &str,
    transform_content_hash: Option<&str>,
    confidence: f64,
    properties_json: &str,
) {
    if from_id.is_empty() || to_id.is_empty() {
        return;
    }
    let artifact = register_artifact_with_hash(
        artifact_registry,
        source_url,
        cache_path,
        transform_content_hash,
    );
    edges.push(EdgeRow {
        edge_type: edge_type.to_string(),
        from_type: from_type.to_string(),
        from_id: from_id.to_string(),
        to_type: to_type.to_string(),
        to_id: to_id.to_string(),
        role: role.to_string(),
        source_artifact_id: artifact,
        source_url: source_url.to_string(),
        cache_path: cache_path.to_string(),
        confidence,
        properties_json: if properties_json.is_empty() {
            "{}".to_string()
        } else {
            properties_json.to_string()
        },
    });
}

fn confidence_score(confidence: &str) -> f64 {
    match confidence {
        "exact" => 1.0,
        "parsed" => 0.8,
        "heuristic" => 0.5,
        value => value.parse().unwrap_or(0.0),
    }
}

fn source_content_hash(cache_path: &str) -> String {
    if cache_path.is_empty() {
        return String::new();
    }
    let path = cache_dir().join(cache_path);
    if cache_path.contains("/meetings/") && cache_path.ends_with(".html") {
        return read_report_html(&path)
            .map(|html| content_hash(&html))
            .unwrap_or_default();
    }
    std::fs::read(path)
        .map(|bytes| content_hash_bytes(&bytes))
        .unwrap_or_default()
}

/// Prefer transform-time hashes from normalized rows over recomputing from current cache.
fn seed_transform_hashes_from_normalized(
    data_dir: &Path,
    registry: &mut HashMap<String, ArtifactEntry>,
) -> Result<(), Box<dyn Error>> {
    const TABLES: &[&str] = &[
        "vote_casts",
        "asked",
        "answered",
        "authored",
        "holds_role",
        "invited",
        "interpellated",
        "interpellation_responded",
        "written_asked",
        "addressed_to",
        "answered_by",
        "answers",
        "oral_written_links",
        "utterances",
        "unresolved_persons",
    ];
    for table in TABLES {
        let path = data_dir.join(format!("normalized/{table}.parquet"));
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            if batch.schema().index_of("source_content_hash").is_err() {
                continue;
            }
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            let artifact_ids = read_string_column(&batch, "source_artifact_id")?;
            let content_hashes = read_string_column(&batch, "source_content_hash")?;
            for i in 0..batch.num_rows() {
                apply_transform_provenance(
                    registry,
                    &source_urls[i],
                    &cache_paths[i],
                    &artifact_ids[i],
                    &content_hashes[i],
                    &format!("{table} row {i}"),
                )?;
            }
        }
    }
    Ok(())
}

fn apply_transform_provenance(
    registry: &mut HashMap<String, ArtifactEntry>,
    source_url: &str,
    cache_path: &str,
    source_artifact_id: &str,
    transform_hash: &str,
    context: &str,
) -> Result<(), Box<dyn Error>> {
    validate_transform_provenance(
        source_url,
        cache_path,
        source_artifact_id,
        transform_hash,
        context,
    )?;
    if source_url.is_empty() && cache_path.is_empty() {
        return Ok(());
    }
    register_artifact_with_hash(
        registry,
        source_url,
        cache_path,
        (!transform_hash.is_empty()).then_some(transform_hash),
    );
    Ok(())
}

fn validate_transform_provenance(
    source_url: &str,
    cache_path: &str,
    source_artifact_id: &str,
    transform_hash: &str,
    context: &str,
) -> Result<(), Box<dyn Error>> {
    if source_url.is_empty() && cache_path.is_empty() {
        return Ok(());
    }
    let canonical = crate::provenance::artifact_id(source_url, cache_path);
    if !source_artifact_id.is_empty() && source_artifact_id != canonical {
        return Err(format!(
            "artifact id mismatch for {context}: row={source_artifact_id} canonical={canonical}"
        )
        .into());
    }
    // Reject silently accepting newer cache bytes for a transform-time hash.
    if !transform_hash.is_empty() && !cache_path.is_empty() {
        let current = source_content_hash(cache_path);
        if !current.is_empty() && current != transform_hash {
            return Err(format!(
                "stale source content for {context}: transform-time hash differs from current cache"
            )
            .into());
        }
    }
    Ok(())
}

/// Prefer cache sidecar `fetched_at`, then any source-manifest `fetched_at` for the path.
fn load_scraped_at_index(data_dir: &Path) -> HashMap<String, String> {
    let mut index = HashMap::new();
    let manifest_dir = data_dir.join("source_manifests");
    let Ok(entries) = std::fs::read_dir(&manifest_dir) else {
        return index;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("parquet") {
            continue;
        }
        let Ok(batches) = read_all_rows(&path) else {
            continue;
        };
        for batch in batches {
            let Ok(cache_paths) = read_string_column(&batch, "cache_path") else {
                continue;
            };
            let Ok(fetched) = read_string_column(&batch, "fetched_at") else {
                continue;
            };
            for i in 0..batch.num_rows() {
                let cp = cache_paths[i].trim();
                let ts = fetched[i].trim();
                if cp.is_empty() || ts.is_empty() {
                    continue;
                }
                index
                    .entry(cp.to_string())
                    .and_modify(|existing: &mut String| {
                        if ts > existing.as_str() {
                            *existing = ts.to_string();
                        }
                    })
                    .or_insert_with(|| ts.to_string());
            }
        }
    }
    index
}

fn resolve_scraped_at(cache_path: &str, manifest_index: &HashMap<String, String>) -> String {
    if cache_path.is_empty() {
        return String::new();
    }
    let full = cache_dir().join(cache_path);
    if let Ok(Some(meta)) = read_cache_metadata(&full) {
        if !meta.fetched_at.trim().is_empty() {
            return meta.fetched_at;
        }
        if !meta.checked_at.trim().is_empty() {
            return meta.checked_at;
        }
    }
    manifest_index.get(cache_path).cloned().unwrap_or_default()
}

fn load_identity_nodes(
    data_dir: &Path,
    add_node: &mut impl FnMut(&str, &str, &str, &str, &str),
) -> Result<(), Box<dyn Error>> {
    let identity = data_dir.join("identity");

    for batch in read_all_rows(&identity.join("persons.parquet"))? {
        let ids = read_string_column(&batch, "person_id")?;
        let first = read_string_column(&batch, "first_name")?;
        let last = read_string_column(&batch, "last_name")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            add_node(
                "Person",
                &ids[i],
                &format!("{} {}", first[i], last[i]),
                &source_urls[i],
                &cache_paths[i],
            );
        }
    }

    for batch in read_all_rows(&identity.join("parties.parquet"))? {
        let slugs = read_string_column(&batch, "party_slug")?;
        for i in 0..batch.num_rows() {
            add_node("Party", &slugs[i], &slugs[i], "", "");
        }
    }

    for batch in read_all_rows(&identity.join("commissions.parquet"))? {
        let ids = read_string_column(&batch, "commission_id")?;
        let names = read_string_column(&batch, "name")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            add_node(
                "Commission",
                &ids[i],
                &names[i],
                &source_urls[i],
                &cache_paths[i],
            );
        }
    }

    let external_path = identity.join("external_persons.parquet");
    if external_path.exists() {
        for batch in read_all_rows(&external_path)? {
            let ids = read_string_column(&batch, "external_person_id")?;
            let names = read_string_column(&batch, "display_name")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                add_node(
                    "ExternalPerson",
                    &ids[i],
                    &names[i],
                    &source_urls[i],
                    &cache_paths[i],
                );
            }
        }
    }

    Ok(())
}

fn load_staging_nodes(
    data_dir: &Path,
    add_node: &mut impl FnMut(&str, &str, &str, &str, &str),
) -> Result<(), Box<dyn Error>> {
    let session = data_dir.join(format!("sessions/{SESSION_ID}"));

    for (kind, rel) in [
        ("plenary", "plenary/meetings.parquet"),
        ("commission", "commission/meetings.parquet"),
    ] {
        for batch in read_all_rows(&session.join(rel))? {
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let node_id = format!("{kind}_{}_{}", session_ids[i], meeting_ids[i]);
                add_node(
                    "Meeting",
                    &node_id,
                    &node_id,
                    &source_urls[i],
                    &cache_paths[i],
                );
            }
        }
    }

    for (kind, rel) in [
        ("plenary", "plenary/questions.parquet"),
        ("commission", "commission/questions.parquet"),
    ] {
        for batch in read_all_rows(&session.join(rel))? {
            let question_ids = read_string_column(&batch, "question_id")?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let topics = read_string_column(&batch, "topics_nl")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let question_id = ensure_question_id(&session_ids[i], kind, &question_ids[i]);
                let label = if topics[i].is_empty() {
                    question_id.clone()
                } else {
                    topics[i].chars().take(120).collect()
                };
                add_node(
                    "Question",
                    &question_id,
                    &label,
                    &source_urls[i],
                    &cache_paths[i],
                );
            }
        }
    }

    for (kind, rel, node_type, id_col, title_col) in [
        (
            "plenary",
            "plenary/hearings.parquet",
            "Hearing",
            "hearing_id",
            "title_nl",
        ),
        (
            "commission",
            "commission/hearings.parquet",
            "Hearing",
            "hearing_id",
            "title_nl",
        ),
        (
            "plenary",
            "plenary/interpellations.parquet",
            "Interpellation",
            "interpellation_id",
            "topics_nl",
        ),
        (
            "commission",
            "commission/interpellations.parquet",
            "Interpellation",
            "interpellation_id",
            "topics_nl",
        ),
    ] {
        let path = session.join(rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let entity_ids = read_string_column(&batch, id_col)?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let titles = read_string_column(&batch, title_col)?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let entity_id = ensure_question_id(&session_ids[i], kind, &entity_ids[i]);
                let label = if titles[i].is_empty() {
                    entity_id.clone()
                } else {
                    titles[i].chars().take(120).collect()
                };
                add_node(
                    node_type,
                    &entity_id,
                    &label,
                    &source_urls[i],
                    &cache_paths[i],
                );
            }
        }
    }

    for kind in ["plenary", "commission"] {
        let path = session.join(format!("{kind}/agenda_items.parquet"));
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let agenda_item_ids = read_string_column(&batch, "agenda_item_id")?;
            let titles_nl = read_string_column(&batch, "title_nl")?;
            let titles_fr = read_string_column(&batch, "title_fr")?;
            let agenda_ids = read_string_column(&batch, "agenda_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let label = if !titles_nl[i].is_empty() {
                    titles_nl[i].chars().take(120).collect::<String>()
                } else if !titles_fr[i].is_empty() {
                    titles_fr[i].chars().take(120).collect::<String>()
                } else if !agenda_ids[i].is_empty() {
                    format!("agenda {}", agenda_ids[i])
                } else {
                    agenda_item_ids[i].clone()
                };
                add_node(
                    "AgendaItem",
                    &agenda_item_ids[i],
                    &label,
                    &source_urls[i],
                    &cache_paths[i],
                );
            }
        }
    }

    load_vote_nodes(data_dir, add_node)?;

    for batch in read_all_rows(&session.join("dossiers.parquet"))? {
        let session_ids = read_string_column(&batch, "session_id")?;
        let ids = read_string_column(&batch, "id")?;
        let titles = read_string_column(&batch, "title")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let node_id = format!("{}/{}", session_ids[i], ids[i]);
            let label: String = titles[i].chars().take(120).collect();
            add_node(
                "Dossier",
                &node_id,
                &label,
                &source_urls[i],
                &cache_paths[i],
            );
        }
    }

    for batch in read_all_rows(&session.join("subdocuments.parquet"))? {
        let ids = read_string_column(&batch, "id")?;
        let types = read_string_column(&batch, "type")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            add_node(
                "Document",
                &ids[i],
                &format!("{} ({})", ids[i], types[i]),
                &source_urls[i],
                &cache_paths[i],
            );
        }
    }

    Ok(())
}

fn load_vote_nodes(
    data_dir: &Path,
    add_node: &mut impl FnMut(&str, &str, &str, &str, &str),
) -> Result<(), Box<dyn Error>> {
    let plenary = data_dir.join(format!("sessions/{SESSION_ID}/plenary"));
    let result_provenance = load_vote_result_provenance(data_dir)?;
    for batch in read_all_rows(&plenary.join("votes.parquet"))? {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let result_ids = read_string_column(&batch, "result_id")?;
        let titles = read_string_column(&batch, "title_nl")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            require_shared_result_provenance(
                &result_provenance,
                &result_ids[i],
                &source_urls[i],
                &cache_paths[i],
                &format!("Vote {}", vote_ids[i]),
            )?;
            let label = titles[i].chars().take(120).collect::<String>();
            add_node(
                "Vote",
                &vote_ids[i],
                &label,
                &source_urls[i],
                &cache_paths[i],
            );
        }
    }

    let vote_results_path = plenary.join("vote_results.parquet");
    if vote_results_path.exists() {
        for batch in read_all_rows(&vote_results_path)? {
            let result_ids = read_string_column(&batch, "result_id")?;
            let methods = read_string_column(&batch, "method")?;
            let outcomes = read_string_column(&batch, "outcome")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let label = format!("{} ({})", methods[i], outcomes[i]);
                add_node(
                    "VoteResult",
                    &result_ids[i],
                    &label,
                    &source_urls[i],
                    &cache_paths[i],
                );
            }
        }
    }

    Ok(())
}

fn load_vote_result_provenance(
    data_dir: &Path,
) -> Result<HashMap<String, (String, String)>, Box<dyn Error>> {
    let path = data_dir.join(format!(
        "sessions/{SESSION_ID}/plenary/vote_results.parquet"
    ));
    let mut provenance = HashMap::new();
    if !path.exists() {
        return Ok(provenance);
    }
    for batch in read_all_rows(&path)? {
        let result_ids = read_string_column(&batch, "result_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let value = (source_urls[i].clone(), cache_paths[i].clone());
            if let Some(existing) = provenance.insert(result_ids[i].clone(), value.clone())
                && existing != value
            {
                return Err(format!(
                    "conflicting graph provenance for vote result {}",
                    result_ids[i]
                )
                .into());
            }
        }
    }
    Ok(provenance)
}

fn require_shared_result_provenance(
    results: &HashMap<String, (String, String)>,
    result_id: &str,
    source_url: &str,
    cache_path: &str,
    owner: &str,
) -> Result<(), Box<dyn Error>> {
    let Some((result_url, result_cache)) = results.get(result_id) else {
        return Err(format!("{owner} references unknown vote result {result_id}").into());
    };
    if result_url != source_url || result_cache != cache_path {
        return Err(
            format!("{owner} does not share provenance with vote result {result_id}").into(),
        );
    }
    Ok(())
}

fn load_normalized_nodes(
    data_dir: &Path,
    add_node: &mut impl FnMut(&str, &str, &str, &str, &str),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/utterances.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let ids = read_string_column(&batch, "utterance_id")?;
        let speakers = read_string_column(&batch, "raw_speaker")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            add_node(
                "Utterance",
                &ids[i],
                &speakers[i],
                &source_urls[i],
                &cache_paths[i],
            );
        }
    }
    Ok(())
}

fn load_membership_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    for batch in read_all_rows(&data_dir.join("identity/memberships.parquet"))? {
        let person_ids = read_string_column(&batch, "person_id")?;
        let org_types = read_string_column(&batch, "org_type")?;
        let org_ids = read_string_column(&batch, "org_id")?;
        let roles = read_string_column(&batch, "role")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let confidences = read_string_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            let to_type = match org_types[i].as_str() {
                "party" => "Party",
                "commission" => "Commission",
                other => other,
            };
            add_edge(
                "MEMBER_OF",
                "Person",
                &person_ids[i],
                to_type,
                &org_ids[i],
                &roles[i],
                &source_urls[i],
                "",
                confidence_score(&confidences[i]),
            );
        }
    }
    Ok(())
}

fn load_has_result_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet"));
    if !path.exists() {
        return Ok(());
    }
    let result_provenance = load_vote_result_provenance(data_dir)?;
    for batch in read_all_rows(&path)? {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let result_ids = read_string_column(&batch, "result_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            require_shared_result_provenance(
                &result_provenance,
                &result_ids[i],
                &source_urls[i],
                &cache_paths[i],
                &format!("HAS_RESULT from {}", vote_ids[i]),
            )?;
            add_edge(
                "HAS_RESULT",
                "Vote",
                &vote_ids[i],
                "VoteResult",
                &result_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                1.0,
            );
        }
    }
    Ok(())
}

fn load_vote_cast_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/vote_casts.parquet");
    if !path.exists() {
        return Ok(());
    }
    let result_provenance = load_vote_result_provenance(data_dir)?;
    for batch in read_all_rows(&path)? {
        let person_ids = read_string_column(&batch, "person_id")?;
        let result_ids = read_string_column(&batch, "result_id")?;
        let positions = read_string_column(&batch, "position")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let source_artifact_ids = read_string_column(&batch, "source_artifact_id")?;
        let source_content_hashes = read_string_column(&batch, "source_content_hash")?;
        let block_parser_versions = read_string_column(&batch, "block_parser_version")?;
        let extractor_versions = read_string_column(&batch, "extractor_version")?;
        let confidences = read_f64_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            require_shared_result_provenance(
                &result_provenance,
                &result_ids[i],
                &source_urls[i],
                &cache_paths[i],
                &format!("CAST from {}", person_ids[i]),
            )?;
            let context = format!("vote cast {} -> {}", person_ids[i], result_ids[i]);
            validate_transform_provenance(
                &source_urls[i],
                &cache_paths[i],
                &source_artifact_ids[i],
                &source_content_hashes[i],
                &context,
            )?;
            if block_parser_versions[i] != BLOCK_PARSER_VERSION
                || extractor_versions[i] != VOTE_EXTRACTOR_VERSION
            {
                return Err(format!(
                    "vote cast parser version mismatch for {} -> {}",
                    person_ids[i], result_ids[i]
                )
                .into());
            }
            add_edge(
                "CAST",
                "Person",
                &person_ids[i],
                "VoteResult",
                &result_ids[i],
                &positions[i],
                &source_urls[i],
                &cache_paths[i],
                confidences[i],
            );
        }
    }
    Ok(())
}

fn load_voted_on_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let result_provenance = load_vote_result_provenance(data_dir)?;
    for batch in
        read_all_rows(&data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet")))?
    {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let result_ids = read_string_column(&batch, "result_id")?;
        let session_ids = read_string_column(&batch, "session_id")?;
        let dossier_ids = read_string_column(&batch, "dossier_id")?;
        let document_ids = read_string_column(&batch, "document_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            require_shared_result_provenance(
                &result_provenance,
                &result_ids[i],
                &source_urls[i],
                &cache_paths[i],
                &format!("VOTED_ON from {}", vote_ids[i]),
            )?;
            if !dossier_ids[i].is_empty() {
                let dossier_node = format!("{}/{}", session_ids[i], dossier_ids[i]);
                add_edge(
                    "VOTED_ON",
                    "Vote",
                    &vote_ids[i],
                    "Dossier",
                    &dossier_node,
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    1.0,
                );
            }
            if !document_ids[i].is_empty() && is_flwb_document_id(&document_ids[i]) {
                add_edge(
                    "VOTED_ON",
                    "Vote",
                    &vote_ids[i],
                    "Document",
                    &document_ids[i],
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    1.0,
                );
            }
        }
    }
    Ok(())
}

fn load_submitted_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    for batch in
        read_all_rows(&data_dir.join(format!("sessions/{SESSION_ID}/subdocuments.parquet")))?
    {
        let dossier_ids = read_string_column(&batch, "dossier_id")?;
        let doc_ids = read_string_column(&batch, "id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let dossier_node = format!("{SESSION_ID}/{}", dossier_ids[i]);
            add_edge(
                "SUBMITTED",
                "Document",
                &doc_ids[i],
                "Dossier",
                &dossier_node,
                "",
                &source_urls[i],
                &cache_paths[i],
                1.0,
            );
        }
    }
    Ok(())
}

fn load_tagged_with_edges(
    data_dir: &Path,
    add_node: &mut impl FnMut(&str, &str, &str, &str, &str),
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    for batch in read_all_rows(&data_dir.join(format!("sessions/{SESSION_ID}/dossiers.parquet")))? {
        let session_ids = read_string_column(&batch, "session_id")?;
        let dossier_ids = read_string_column(&batch, "id")?;
        let main = read_string_column(&batch, "eurovoc_main_descriptor")?;
        let descriptors = read_string_column(&batch, "eurovoc_descriptors")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let dossier_node = format!("{}/{}", session_ids[i], dossier_ids[i]);
            let main_topic = main[i].trim();
            if !main_topic.is_empty() {
                add_node(
                    "Topic",
                    main_topic,
                    main_topic,
                    &source_urls[i],
                    &cache_paths[i],
                );
                add_edge(
                    "TAGGED_WITH",
                    "Dossier",
                    &dossier_node,
                    "Topic",
                    main_topic,
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    1.0,
                );
            }
            for topic in descriptors[i]
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                if !main_topic.is_empty() && topic.eq_ignore_ascii_case(main_topic) {
                    continue;
                }
                add_node("Topic", topic, topic, &source_urls[i], &cache_paths[i]);
                add_edge(
                    "TAGGED_WITH",
                    "Dossier",
                    &dossier_node,
                    "Topic",
                    topic,
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    1.0,
                );
            }
        }
    }
    Ok(())
}

fn load_authored_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/authored.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let person_ids = read_string_column(&batch, "person_id")?;
        let entity_types = read_string_column(&batch, "entity_type")?;
        let entity_ids = read_string_column(&batch, "entity_id")?;
        let target_types = read_string_column(&batch, "target_type")?;
        let target_ids = read_string_column(&batch, "target_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_f64_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            let from_type = if !entity_types[i].is_empty() {
                entity_types[i].clone()
            } else {
                "Person".to_string()
            };
            let from_id = if !entity_ids[i].is_empty() {
                entity_ids[i].clone()
            } else {
                person_ids[i].clone()
            };
            if from_id.is_empty() {
                continue;
            }
            let to_type = match target_types[i].as_str() {
                "dossier" => "Dossier",
                "document" => "Document",
                other => other,
            };
            add_edge(
                "AUTHORED",
                &from_type,
                &from_id,
                to_type,
                &target_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                confidences[i],
            );
        }
    }
    Ok(())
}

fn load_asked_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/asked.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let person_ids = read_string_column(&batch, "person_id")?;
        let question_ids = read_string_column(&batch, "question_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_f64_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            add_edge(
                "ASKED",
                "Person",
                &person_ids[i],
                "Question",
                &question_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                confidences[i],
            );
        }
    }
    Ok(())
}

fn load_answered_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/answered.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let entity_types = read_string_column(&batch, "entity_type")?;
        let entity_ids = read_string_column(&batch, "entity_id")?;
        let question_ids = read_string_column(&batch, "question_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_f64_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            if entity_ids[i].is_empty() {
                continue;
            }
            add_edge(
                "ANSWERED",
                &entity_types[i],
                &entity_ids[i],
                "Question",
                &question_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                confidences[i],
            );
        }
    }
    Ok(())
}

/// AgendaItem → Meeting, AgendaItem → Dossier, and typed proceedings → AgendaItem.
fn load_agenda_item_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let session = data_dir.join(format!("sessions/{SESSION_ID}"));
    for kind in ["plenary", "commission"] {
        let path = session.join(format!("{kind}/agenda_items.parquet"));
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let agenda_item_ids = read_string_column(&batch, "agenda_item_id")?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let meeting_kinds = read_string_column(&batch, "meeting_kind")?;
            let item_kinds = read_string_column(&batch, "item_kind")?;
            let item_ids = read_string_column(&batch, "item_id")?;
            let dossier_ids = read_string_column(&batch, "dossier_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let meeting_kind = if meeting_kinds[i].is_empty() {
                    kind
                } else {
                    meeting_kinds[i].as_str()
                };
                let meeting_node_id =
                    format!("{meeting_kind}_{}_{}", session_ids[i], meeting_ids[i]);
                add_edge(
                    "PART_OF",
                    "AgendaItem",
                    &agenda_item_ids[i],
                    "Meeting",
                    &meeting_node_id,
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    1.0,
                );

                let dossier_raw = dossier_ids[i].trim();
                if !dossier_raw.is_empty() {
                    let dossier_node = if dossier_raw.contains('/') {
                        dossier_raw.to_string()
                    } else {
                        format!("{}/{}", session_ids[i], dossier_raw)
                    };
                    add_edge(
                        "REFERENCES",
                        "AgendaItem",
                        &agenda_item_ids[i],
                        "Dossier",
                        &dossier_node,
                        "",
                        &source_urls[i],
                        &cache_paths[i],
                        1.0,
                    );
                }

                if item_ids[i].is_empty() {
                    continue;
                }
                let proceeding_type = match item_kinds[i].as_str() {
                    "question" => Some("Question"),
                    "hearing" => Some("Hearing"),
                    "interpellation" => Some("Interpellation"),
                    _ => None,
                };
                let Some(proceeding_type) = proceeding_type else {
                    continue;
                };
                let proceeding_id =
                    ensure_question_id(&session_ids[i], meeting_kind, &item_ids[i]);
                add_edge(
                    "PART_OF",
                    proceeding_type,
                    &proceeding_id,
                    "AgendaItem",
                    &agenda_item_ids[i],
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    1.0,
                );
            }
        }
    }
    Ok(())
}

fn load_holds_role_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/holds_role.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let person_ids = read_string_column(&batch, "person_id")?;
        let target_ids = read_string_column(&batch, "target_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_f64_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            add_edge(
                "HOLDS_ROLE",
                "Person",
                &person_ids[i],
                "Meeting",
                &target_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                confidences[i],
            );
        }
    }
    Ok(())
}

fn load_interpellated_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/interpellated.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let person_ids = read_string_column(&batch, "person_id")?;
        let interpellation_ids = read_string_column(&batch, "interpellation_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_f64_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            add_edge(
                "INTERPELLED",
                "Person",
                &person_ids[i],
                "Interpellation",
                &interpellation_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                confidences[i],
            );
        }
    }
    Ok(())
}

fn load_interpellation_responded_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/interpellation_responded.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let entity_types = read_string_column(&batch, "entity_type")?;
        let entity_ids = read_string_column(&batch, "entity_id")?;
        let interpellation_ids = read_string_column(&batch, "interpellation_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_f64_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            if entity_ids[i].is_empty() {
                continue;
            }
            add_edge(
                "RESPONDED",
                &entity_types[i],
                &entity_ids[i],
                "Interpellation",
                &interpellation_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                confidences[i],
            );
        }
    }
    Ok(())
}

fn load_invited_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/invited.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let entity_types = read_string_column(&batch, "entity_type")?;
        let entity_ids = read_string_column(&batch, "entity_id")?;
        let hearing_ids = read_string_column(&batch, "hearing_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_f64_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            if entity_ids[i].is_empty() {
                continue;
            }
            add_edge(
                "INVITED",
                &entity_types[i],
                &entity_ids[i],
                "Hearing",
                &hearing_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                confidences[i],
            );
        }
    }
    Ok(())
}

fn load_proceeding_meeting_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let session = data_dir.join(format!("sessions/{SESSION_ID}"));
    for (kind, rel, node_type, id_col) in [
        (
            "plenary",
            "plenary/hearings.parquet",
            "Hearing",
            "hearing_id",
        ),
        (
            "commission",
            "commission/hearings.parquet",
            "Hearing",
            "hearing_id",
        ),
        (
            "plenary",
            "plenary/interpellations.parquet",
            "Interpellation",
            "interpellation_id",
        ),
        (
            "commission",
            "commission/interpellations.parquet",
            "Interpellation",
            "interpellation_id",
        ),
    ] {
        let path = session.join(rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let entity_ids = read_string_column(&batch, id_col)?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let meeting_ids = read_string_column(&batch, "meeting_id")?;
            let source_urls = read_string_column(&batch, "source_url")?;
            let cache_paths = read_string_column(&batch, "cache_path")?;
            for i in 0..batch.num_rows() {
                let entity_id = ensure_question_id(&session_ids[i], kind, &entity_ids[i]);
                let meeting_node_id = format!("{kind}_{}_{}", session_ids[i], meeting_ids[i]);
                add_edge(
                    "PART_OF",
                    node_type,
                    &entity_id,
                    "Meeting",
                    &meeting_node_id,
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    1.0,
                );
            }
        }
    }
    Ok(())
}

fn build_site_ref_node_lookup(
    data_dir: &Path,
    node_seen: &HashMap<(String, String), ()>,
) -> Result<HashMap<String, (String, String)>, Box<dyn Error>> {
    let mut lookup: HashMap<String, (String, String)> = HashMap::new();
    let session = data_dir.join(format!("sessions/{SESSION_ID}"));

    for (kind, rel, node_type, id_col, internal_col) in [
        (
            "plenary",
            "plenary/questions.parquet",
            "Question",
            "question_id",
            "internal_ids",
        ),
        (
            "commission",
            "commission/questions.parquet",
            "Question",
            "question_id",
            "internal_ids",
        ),
        (
            "plenary",
            "plenary/hearings.parquet",
            "Hearing",
            "hearing_id",
            "internal_ids",
        ),
        (
            "commission",
            "commission/hearings.parquet",
            "Hearing",
            "hearing_id",
            "internal_ids",
        ),
        (
            "plenary",
            "plenary/interpellations.parquet",
            "Interpellation",
            "interpellation_id",
            "internal_ids",
        ),
        (
            "commission",
            "commission/interpellations.parquet",
            "Interpellation",
            "interpellation_id",
            "internal_ids",
        ),
    ] {
        let path = session.join(rel);
        if !path.exists() {
            continue;
        }
        for batch in read_all_rows(&path)? {
            let entity_ids = read_string_column(&batch, id_col)?;
            let session_ids = read_string_column(&batch, "session_id")?;
            let internal_ids = read_string_column(&batch, internal_col)?;
            for i in 0..batch.num_rows() {
                let node_id = ensure_question_id(&session_ids[i], kind, &entity_ids[i]);
                if !node_seen.contains_key(&(node_type.to_string(), node_id.clone())) {
                    continue;
                }
                for token in internal_ids[i].split(',') {
                    let key = normalize_site_ref(token);
                    if key.is_empty() {
                        continue;
                    }
                    lookup
                        .entry(key)
                        .or_insert((node_type.to_string(), node_id.clone()));
                }
            }
        }
    }

    Ok(lookup)
}

fn resolve_proceeding_target_id(
    session_id: &str,
    meeting_kind: &str,
    raw_item_id: &str,
    site_refs_csv: &str,
    node_type: &str,
    site_ref_nodes: &HashMap<String, (String, String)>,
    node_seen: &HashMap<(String, String), ()>,
) -> Option<String> {
    let scoped = ensure_question_id(session_id, meeting_kind, raw_item_id);
    if node_seen.contains_key(&(node_type.to_string(), scoped.clone())) {
        return Some(scoped);
    }
    for token in site_refs_csv.split(',') {
        let key = normalize_site_ref(token);
        if key.is_empty() {
            continue;
        }
        if let Some((ntype, node_id)) = site_ref_nodes.get(&key) {
            if ntype == node_type
                && node_seen.contains_key(&(node_type.to_string(), node_id.clone()))
            {
                return Some(node_id.clone());
            }
        }
    }
    if node_type == "Interpellation" {
        return None;
    }
    let meeting_prefix = format!("{session_id}_{meeting_kind}_");
    let rest = raw_item_id
        .strip_prefix(&meeting_prefix)
        .unwrap_or(raw_item_id);
    let meeting_id = rest.split('_').next().unwrap_or("");
    if !meeting_id.is_empty() {
        let node_prefix = format!("{meeting_prefix}{meeting_id}_");
        let mut candidates: Vec<String> = node_seen
            .keys()
            .filter(|(ntype, node_id)| ntype == node_type && node_id.starts_with(&node_prefix))
            .map(|(_, node_id)| node_id.clone())
            .collect();
        candidates.sort();
        if candidates.len() == 1 {
            return Some(candidates[0].clone());
        }
    }
    None
}

fn load_spoke_and_part_of_edges(
    data_dir: &Path,
    site_ref_nodes: &HashMap<String, (String, String)>,
    node_seen: &HashMap<(String, String), ()>,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, f64),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/utterances.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let utterance_ids = read_string_column(&batch, "utterance_id")?;
        let session_ids = read_string_column(&batch, "session_id")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let meeting_kinds = read_string_column(&batch, "meeting_kind")?;
        let item_kinds = read_string_column(&batch, "item_kind")?;
        let item_ids = read_string_column(&batch, "item_id")?;
        let question_ids = read_string_column(&batch, "question_ids")?;
        let agenda_item_ids = if batch.schema().index_of("agenda_item_id").is_ok() {
            Some(read_string_column(&batch, "agenda_item_id")?)
        } else {
            None
        };
        let speaker_ids = read_string_column(&batch, "speaker_person_id")?;
        let entity_types = read_string_column(&batch, "speaker_entity_type")?;
        let entity_ids = read_string_column(&batch, "speaker_entity_id")?;
        let confidences = read_f64_column(&batch, "confidence")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let meeting_node_id =
                format!("{}_{}_{}", meeting_kinds[i], session_ids[i], meeting_ids[i]);
            add_edge(
                "PART_OF",
                "Utterance",
                &utterance_ids[i],
                "Meeting",
                &meeting_node_id,
                "",
                &source_urls[i],
                &cache_paths[i],
                1.0,
            );
            if let Some(ref agenda_ids) = agenda_item_ids {
                if !agenda_ids[i].is_empty() {
                    add_edge(
                        "PART_OF",
                        "Utterance",
                        &utterance_ids[i],
                        "AgendaItem",
                        &agenda_ids[i],
                        "",
                        &source_urls[i],
                        &cache_paths[i],
                        1.0,
                    );
                }
            }
            if item_kinds[i] == "question" && !item_ids[i].is_empty() {
                if let Some(target_id) = resolve_proceeding_target_id(
                    &session_ids[i],
                    &meeting_kinds[i],
                    &item_ids[i],
                    &question_ids[i],
                    "Question",
                    site_ref_nodes,
                    node_seen,
                ) {
                    add_edge(
                        "PART_OF",
                        "Utterance",
                        &utterance_ids[i],
                        "Question",
                        &target_id,
                        "",
                        &source_urls[i],
                        &cache_paths[i],
                        1.0,
                    );
                }
            } else if item_kinds[i] == "hearing" && !item_ids[i].is_empty() {
                if let Some(target_id) = resolve_proceeding_target_id(
                    &session_ids[i],
                    &meeting_kinds[i],
                    &item_ids[i],
                    &question_ids[i],
                    "Hearing",
                    site_ref_nodes,
                    node_seen,
                ) {
                    add_edge(
                        "PART_OF",
                        "Utterance",
                        &utterance_ids[i],
                        "Hearing",
                        &target_id,
                        "",
                        &source_urls[i],
                        &cache_paths[i],
                        1.0,
                    );
                }
            } else if item_kinds[i] == "interpellation" && !item_ids[i].is_empty() {
                if let Some(target_id) = resolve_proceeding_target_id(
                    &session_ids[i],
                    &meeting_kinds[i],
                    &item_ids[i],
                    &question_ids[i],
                    "Interpellation",
                    site_ref_nodes,
                    node_seen,
                ) {
                    add_edge(
                        "PART_OF",
                        "Utterance",
                        &utterance_ids[i],
                        "Interpellation",
                        &target_id,
                        "",
                        &source_urls[i],
                        &cache_paths[i],
                        1.0,
                    );
                }
            }
            let (from_type, from_id) = if !entity_ids[i].is_empty() {
                (entity_types[i].as_str(), entity_ids[i].as_str())
            } else if !speaker_ids[i].is_empty() {
                ("Person", speaker_ids[i].as_str())
            } else {
                continue;
            };
            if from_id.is_empty() {
                continue;
            }
            add_edge(
                "SPOKE",
                from_type,
                from_id,
                "Utterance",
                &utterance_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                confidences[i],
            );
        }
    }
    Ok(())
}

pub fn write_nodes(path: &Path, rows: &[NodeRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("node_type", false),
        utf8_field("node_id", false),
        utf8_field("label", false),
        utf8_field("source_artifact_id", false),
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
            col!(|r| r.node_type.clone()),
            col!(|r| r.node_id.clone()),
            col!(|r| r.label.clone()),
            col!(|r| r.source_artifact_id.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
        ],
    )
}

pub fn write_edges(path: &Path, rows: &[EdgeRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("edge_type", false),
        utf8_field("from_type", false),
        utf8_field("from_id", false),
        utf8_field("to_type", false),
        utf8_field("to_id", false),
        utf8_field("role", false),
        utf8_field("source_artifact_id", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
        Field::new("confidence", DataType::Float64, false),
        utf8_field("properties_json", false),
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
            col!(|r| r.edge_type.clone()),
            col!(|r| r.from_type.clone()),
            col!(|r| r.from_id.clone()),
            col!(|r| r.to_type.clone()),
            col!(|r| r.to_id.clone()),
            col!(|r| r.role.clone()),
            col!(|r| r.source_artifact_id.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            Arc::new(Float64Array::from(
                rows.iter().map(|r| r.confidence).collect::<Vec<_>>(),
            )) as ArrayRef,
            col!(|r| r.properties_json.clone()),
        ],
    )
}

pub fn write_artifacts(path: &Path, rows: &[ArtifactRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("source_artifact_id", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
        utf8_field("source_content_hash", false),
        utf8_field("block_parser_version", false),
        utf8_field("extractor_version", false),
        utf8_field("scraped_at", false),
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
            col!(|r| r.source_artifact_id.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.source_content_hash.clone()),
            col!(|r| r.block_parser_version.clone()),
            col!(|r| r.extractor_version.clone()),
            col!(|r| r.scraped_at.clone()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crawl::artifact_id;
    use crawl::vote_io::{write_vote_results_parquet, write_votes_parquet};
    use crawl::vote_types::{VoteDecisionDraft, VoteResultDraft};
    use normalize::vote_casts::{VoteCastRow, write_vote_casts};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("graph_vote_result_reuse_{nonce}"))
    }

    fn vote(vote_id: &str, seq: u32, reuses_result: bool) -> VoteDecisionDraft {
        VoteDecisionDraft {
            vote_id: vote_id.into(),
            result_id: "56-1-r1".into(),
            session_id: 56,
            meeting_id: 1,
            date: "2026-01-01".into(),
            seq,
            title_nl: format!("Decision {seq}"),
            title_fr: String::new(),
            method: "roll_call".into(),
            status: "complete".into(),
            outcome: "adopted".into(),
            dossier_id: "123".into(),
            document_id: String::new(),
            motion_id: String::new(),
            source_roll_call_number: "1".into(),
            reuses_result,
            source_url: "https://example.test/report".into(),
            cache_path: String::new(),
        }
    }

    #[test]
    fn reused_result_has_one_cast_set_and_two_decision_traversals() {
        let root = test_dir();
        let plenary = root.join("sessions/56/plenary");
        let normalized = root.join("normalized");
        std::fs::create_dir_all(&plenary).unwrap();
        std::fs::create_dir_all(&normalized).unwrap();
        write_votes_parquet(
            &plenary.join("votes.parquet"),
            &[vote("56-1-v1", 1, false), vote("56-1-v2", 2, true)],
        )
        .unwrap();
        write_vote_results_parquet(
            &plenary.join("vote_results.parquet"),
            &[VoteResultDraft {
                result_id: "56-1-r1".into(),
                session_id: 56,
                meeting_id: 1,
                seq: 1,
                method: "roll_call".into(),
                named: true,
                status: "complete".into(),
                outcome: "adopted".into(),
                source_roll_call_number: "1".into(),
                source_url: "https://example.test/report".into(),
                cache_path: String::new(),
            }],
        )
        .unwrap();
        let shared_artifact = artifact_id("https://example.test/report", "");
        write_vote_casts(
            &normalized.join("vote_casts.parquet"),
            &[VoteCastRow {
                vote_cast_id: "56-1-r1_yes_0".into(),
                result_id: "56-1-r1".into(),
                session_id: 56,
                meeting_id: 1,
                person_id: "p1".into(),
                position: "yes".into(),
                raw_name: "Known Member".into(),
                source_url: "https://example.test/report".into(),
                cache_path: String::new(),
                source_artifact_id: shared_artifact.clone(),
                source_content_hash: String::new(),
                block_parser_version: BLOCK_PARSER_VERSION.into(),
                extractor_version: VOTE_EXTRACTOR_VERSION.into(),
                confidence: 1.0,
            }],
        )
        .unwrap();

        let mut nodes = Vec::new();
        load_vote_nodes(
            &root,
            &mut |node_type, node_id, label, source_url, cache_path| {
                nodes.push(NodeRow {
                    node_type: node_type.into(),
                    node_id: node_id.into(),
                    label: label.into(),
                    source_artifact_id: crate::provenance::artifact_id(source_url, cache_path),
                    source_url: source_url.into(),
                    cache_path: cache_path.into(),
                });
            },
        )
        .unwrap();

        let mut edges = Vec::new();
        let mut artifacts = HashMap::new();
        {
            let mut add_edge = |edge_type: &str,
                                from_type: &str,
                                from_id: &str,
                                to_type: &str,
                                to_id: &str,
                                role: &str,
                                source_url: &str,
                                cache_path: &str,
                                confidence: f64| {
                register_artifact_edge(
                    &mut edges,
                    &mut artifacts,
                    edge_type,
                    from_type,
                    from_id,
                    to_type,
                    to_id,
                    role,
                    source_url,
                    cache_path,
                    confidence,
                    "{}",
                );
            };
            load_has_result_edges(&root, &mut add_edge).unwrap();
            load_vote_cast_edges(&root, &mut add_edge).unwrap();
            load_voted_on_edges(&root, &mut add_edge).unwrap();
        }

        assert_eq!(
            nodes.iter().filter(|node| node.node_type == "Vote").count(),
            2
        );
        assert_eq!(
            nodes
                .iter()
                .filter(|node| node.node_type == "VoteResult")
                .count(),
            1
        );
        assert_eq!(
            edges
                .iter()
                .filter(|edge| edge.edge_type == "HAS_RESULT")
                .count(),
            2
        );
        assert_eq!(
            edges
                .iter()
                .filter(|edge| edge.edge_type == "VOTED_ON")
                .count(),
            2
        );
        let casts = edges
            .iter()
            .filter(|edge| edge.edge_type == "CAST")
            .collect::<Vec<_>>();
        assert_eq!(casts.len(), 1);
        assert_eq!(casts[0].from_id, "p1");
        assert_eq!(casts[0].to_id, "56-1-r1");
        assert_eq!(casts[0].role, "yes");

        for vote_id in ["56-1-v1", "56-1-v2"] {
            let result_id = edges
                .iter()
                .find(|edge| edge.edge_type == "HAS_RESULT" && edge.from_id == vote_id)
                .map(|edge| edge.to_id.as_str())
                .unwrap();
            assert!(
                edges
                    .iter()
                    .any(|edge| edge.edge_type == "CAST" && edge.to_id == result_id)
            );
        }
        assert!(
            nodes
                .iter()
                .all(|node| node.source_artifact_id == shared_artifact)
        );
        assert!(
            edges
                .iter()
                .all(|edge| edge.source_artifact_id == shared_artifact)
        );
        assert_eq!(artifacts.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agenda_item_references_dossier_and_utterance_part_of() {
        use crawl::{AgendaItemDraft, MeetingKind, write_agenda_items_parquet};

        let root = test_dir();
        let plenary = root.join("sessions/56/plenary");
        let normalized = root.join("normalized");
        std::fs::create_dir_all(&plenary).unwrap();
        std::fs::create_dir_all(&normalized).unwrap();

        write_agenda_items_parquet(
            &plenary.join("agenda_items.parquet"),
            &[AgendaItemDraft {
                agenda_item_id: "56_plenary_42_agenda_100".into(),
                session_id: 56,
                meeting_id: 42,
                meeting_kind: MeetingKind::Plenary,
                agenda_id: "15".into(),
                item_kind: "proposition".into(),
                item_id: String::new(),
                title_nl: "Wetsontwerp (56/318)".into(),
                title_fr: String::new(),
                dossier_id: "56/318".into(),
                document_id: "1-9".into(),
                internal_ids: String::new(),
                start_block: 100,
                end_block: 200,
                title_blocks: "100".into(),
                source_section: "propositions".into(),
                source_url: "https://example.test/ip042".into(),
                cache_path: "sessions/56/meetings/plenary/56-42.html".into(),
            }],
        )
        .unwrap();

        {
            use identity::parquet_io::{utf8_field, write_parquet};
            use arrow::array::{ArrayRef, Float64Array, StringArray};
            use arrow::datatypes::Schema;
            use std::sync::Arc;
            let schema = Schema::new(vec![
                utf8_field("utterance_id", false),
                utf8_field("session_id", false),
                utf8_field("meeting_id", false),
                utf8_field("meeting_kind", false),
                utf8_field("agenda_id", false),
                utf8_field("agenda_item_id", false),
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
                utf8_field("speaker_person_id", false),
                utf8_field("source_url", false),
                utf8_field("cache_path", false),
                utf8_field("source_artifact_id", false),
                utf8_field("source_content_hash", false),
                utf8_field("block_parser_version", false),
                utf8_field("extractor_version", false),
                Field::new("confidence", DataType::Float64, false),
            ]);
            let s = |v: &str| Arc::new(StringArray::from(vec![v.to_string()])) as ArrayRef;
            write_parquet(
                &normalized.join("utterances.parquet"),
                schema,
                vec![
                    s("56_plenary_42_15_15_01"),
                    s("56"),
                    s("42"),
                    s("plenary"),
                    s("15"),
                    s("56_plenary_42_agenda_100"),
                    s("15.01"),
                    s("1"),
                    s("proposition"),
                    s(""),
                    s(""),
                    s("56/318"),
                    s("1-9"),
                    s(""),
                    s(""),
                    s("Someone"),
                    s("mp"),
                    s("Person"),
                    s("01226"),
                    s("Hello"),
                    s("NL"),
                    s("101"),
                    s("101"),
                    s("propositions"),
                    s("01226"),
                    s("https://example.test/ip042"),
                    s("sessions/56/meetings/plenary/56-42.html"),
                    s("art"),
                    s("hash"),
                    s("v1"),
                    s("v1"),
                    Arc::new(Float64Array::from(vec![1.0])) as ArrayRef,
                ],
            )
            .unwrap();
        }

        let mut edges = Vec::new();
        let mut add_edge =
            |edge_type: &str,
             from_type: &str,
             from_id: &str,
             to_type: &str,
             to_id: &str,
             role: &str,
             source_url: &str,
             cache_path: &str,
             confidence: f64| {
                edges.push(EdgeRow {
                    edge_type: edge_type.into(),
                    from_type: from_type.into(),
                    from_id: from_id.into(),
                    to_type: to_type.into(),
                    to_id: to_id.into(),
                    role: role.into(),
                    source_artifact_id: String::new(),
                    source_url: source_url.into(),
                    cache_path: cache_path.into(),
                    confidence,
                    properties_json: "{}".into(),
                });
            };

        load_agenda_item_edges(&root, &mut add_edge).unwrap();
        load_spoke_and_part_of_edges(&root, &HashMap::new(), &HashMap::new(), &mut add_edge)
            .unwrap();

        assert!(edges.iter().any(|e| {
            e.edge_type == "REFERENCES"
                && e.from_type == "AgendaItem"
                && e.from_id == "56_plenary_42_agenda_100"
                && e.to_type == "Dossier"
                && e.to_id == "56/318"
        }));
        assert!(edges.iter().any(|e| {
            e.edge_type == "PART_OF"
                && e.from_type == "AgendaItem"
                && e.from_id == "56_plenary_42_agenda_100"
                && e.to_type == "Meeting"
                && e.to_id == "plenary_56_42"
        }));
        assert!(edges.iter().any(|e| {
            e.edge_type == "PART_OF"
                && e.from_type == "Utterance"
                && e.from_id == "56_plenary_42_15_15_01"
                && e.to_type == "AgendaItem"
                && e.to_id == "56_plenary_42_agenda_100"
        }));

        std::fs::remove_dir_all(root).unwrap();
    }
}
