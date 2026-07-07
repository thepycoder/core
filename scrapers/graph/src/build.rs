use crate::provenance::register_artifact;
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::Schema;
use crawl::utils::ensure_question_id;
use identity::parquet_io::{read_all_rows, read_string_column, utf8_field, write_parquet};
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
    pub confidence: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactRow {
    pub source_artifact_id: String,
    pub source_url: String,
    pub cache_path: String,
    pub parser_version: String,
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
    let mut artifact_registry: HashMap<String, (String, String)> = HashMap::new();
    let mut node_seen: HashMap<(String, String), ()> = HashMap::new();

    let mut add_node = |node_type: &str, node_id: &str, label: &str, source_url: &str, cache_path: &str| {
        if node_id.is_empty() {
            return;
        }
        if node_seen.insert((node_type.to_string(), node_id.to_string()), ()).is_some() {
            return;
        }
        nodes.push(NodeRow {
            node_type: node_type.to_string(),
            node_id: node_id.to_string(),
            label: label.to_string(),
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
                        confidence: &str| {
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
            confidence: confidence.to_string(),
        });
    };

    load_identity_nodes(data_dir, &mut add_node)?;
    load_staging_nodes(data_dir, &mut add_node)?;
    load_normalized_nodes(data_dir, &mut add_node)?;

    load_membership_edges(data_dir, &mut add_edge)?;
    load_vote_cast_edges(data_dir, &mut add_edge)?;
    load_voted_on_edges(data_dir, &mut add_edge)?;
    load_submitted_edges(data_dir, &mut add_edge)?;
    load_tagged_with_edges(data_dir, &mut add_node, &mut add_edge)?;
    load_authored_edges(data_dir, &mut add_edge)?;
    load_asked_edges(data_dir, &mut add_edge)?;
    load_holds_role_edges(data_dir, &mut add_edge)?;
    load_spoke_and_part_of_edges(data_dir, &mut add_edge)?;

    nodes.sort_by(|a, b| a.node_type.cmp(&b.node_type).then(a.node_id.cmp(&b.node_id)));
    edges.sort_by(|a, b| {
        a.edge_type
            .cmp(&b.edge_type)
            .then(a.from_id.cmp(&b.from_id))
            .then(a.to_id.cmp(&b.to_id))
            .then(a.role.cmp(&b.role))
    });

    let parser_version = env!("CARGO_PKG_VERSION").to_string();
    let mut artifacts: Vec<ArtifactRow> = artifact_registry
        .into_iter()
        .map(|(id, (source_url, cache_path))| ArtifactRow {
            source_artifact_id: id,
            source_url,
            cache_path,
            parser_version: parser_version.clone(),
            scraped_at: String::new(),
        })
        .collect();
    artifacts.sort_by(|a, b| a.source_artifact_id.cmp(&b.source_artifact_id));

    Ok(GraphBuild {
        nodes,
        edges,
        artifacts,
    })
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
            add_node("Commission", &ids[i], &names[i], &source_urls[i], &cache_paths[i]);
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
                add_node("Meeting", &node_id, &node_id, &source_urls[i], &cache_paths[i]);
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

    for batch in read_all_rows(&session.join("plenary/votes.parquet"))? {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let titles = read_string_column(&batch, "title_nl")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let label = titles[i].chars().take(120).collect::<String>();
            add_node("Vote", &vote_ids[i], &label, &source_urls[i], &cache_paths[i]);
        }
    }

    for batch in read_all_rows(&session.join("dossiers.parquet"))? {
        let session_ids = read_string_column(&batch, "session_id")?;
        let ids = read_string_column(&batch, "id")?;
        let titles = read_string_column(&batch, "title")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let node_id = format!("{}/{}", session_ids[i], ids[i]);
            let label: String = titles[i].chars().take(120).collect();
            add_node("Dossier", &node_id, &label, &source_urls[i], &cache_paths[i]);
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
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, &str),
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
                &confidences[i],
            );
        }
    }
    Ok(())
}

fn load_vote_cast_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, &str),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/vote_casts.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let person_ids = read_string_column(&batch, "person_id")?;
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_string_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            add_edge(
                "CAST",
                "Person",
                &person_ids[i],
                "Vote",
                &vote_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                &confidences[i],
            );
        }
    }
    Ok(())
}

fn load_voted_on_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, &str),
) -> Result<(), Box<dyn Error>> {
    for batch in read_all_rows(&data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet")))? {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let session_ids = read_string_column(&batch, "session_id")?;
        let dossier_ids = read_string_column(&batch, "dossier_id")?;
        let document_ids = read_string_column(&batch, "document_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
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
                    "exact",
                );
            }
            if !document_ids[i].is_empty() {
                add_edge(
                    "VOTED_ON",
                    "Vote",
                    &vote_ids[i],
                    "Document",
                    &document_ids[i],
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    "exact",
                );
            }
        }
    }
    Ok(())
}

fn load_submitted_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, &str),
) -> Result<(), Box<dyn Error>> {
    for batch in read_all_rows(&data_dir.join(format!("sessions/{SESSION_ID}/subdocuments.parquet")))? {
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
                "exact",
            );
        }
    }
    Ok(())
}

fn load_tagged_with_edges(
    data_dir: &Path,
    add_node: &mut impl FnMut(&str, &str, &str, &str, &str),
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, &str),
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
                add_node("Topic", main_topic, main_topic, &source_urls[i], &cache_paths[i]);
                add_edge(
                    "TAGGED_WITH",
                    "Dossier",
                    &dossier_node,
                    "Topic",
                    main_topic,
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    "exact",
                );
            }
            for topic in descriptors[i].split(',').map(str::trim).filter(|t| !t.is_empty()) {
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
                    "exact",
                );
            }
        }
    }
    Ok(())
}

fn load_authored_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, &str),
) -> Result<(), Box<dyn Error>> {
    let path = data_dir.join("normalized/authored.parquet");
    if !path.exists() {
        return Ok(());
    }
    for batch in read_all_rows(&path)? {
        let person_ids = read_string_column(&batch, "person_id")?;
        let target_types = read_string_column(&batch, "target_type")?;
        let target_ids = read_string_column(&batch, "target_id")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        let confidences = read_string_column(&batch, "confidence")?;
        for i in 0..batch.num_rows() {
            let to_type = match target_types[i].as_str() {
                "dossier" => "Dossier",
                "document" => "Document",
                other => other,
            };
            add_edge(
                "AUTHORED",
                "Person",
                &person_ids[i],
                to_type,
                &target_ids[i],
                "",
                &source_urls[i],
                &cache_paths[i],
                &confidences[i],
            );
        }
    }
    Ok(())
}

fn load_asked_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, &str),
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
        let confidences = read_string_column(&batch, "confidence")?;
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
                &confidences[i],
            );
        }
    }
    Ok(())
}

fn load_holds_role_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, &str),
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
        let confidences = read_string_column(&batch, "confidence")?;
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
                &confidences[i],
            );
        }
    }
    Ok(())
}

fn load_spoke_and_part_of_edges(
    data_dir: &Path,
    add_edge: &mut impl FnMut(&str, &str, &str, &str, &str, &str, &str, &str, &str),
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
        let speaker_ids = read_string_column(&batch, "speaker_person_id")?;
        let confidences = read_string_column(&batch, "confidence")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            let meeting_node_id = format!(
                "{}_{}_{}",
                meeting_kinds[i], session_ids[i], meeting_ids[i]
            );
            add_edge(
                "PART_OF",
                "Utterance",
                &utterance_ids[i],
                "Meeting",
                &meeting_node_id,
                "",
                &source_urls[i],
                &cache_paths[i],
                "exact",
            );
            if item_kinds[i] == "question" && !item_ids[i].is_empty() {
                add_edge(
                    "PART_OF",
                    "Utterance",
                    &utterance_ids[i],
                    "Question",
                    &item_ids[i],
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    "exact",
                );
            }
            if !speaker_ids[i].is_empty() {
                add_edge(
                    "SPOKE",
                    "Person",
                    &speaker_ids[i],
                    "Utterance",
                    &utterance_ids[i],
                    "",
                    &source_urls[i],
                    &cache_paths[i],
                    &confidences[i],
                );
            }
        }
    }
    Ok(())
}

pub fn write_nodes(path: &Path, rows: &[NodeRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("node_type", false),
        utf8_field("node_id", false),
        utf8_field("label", false),
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
            col!(|r| r.edge_type.clone()),
            col!(|r| r.from_type.clone()),
            col!(|r| r.from_id.clone()),
            col!(|r| r.to_type.clone()),
            col!(|r| r.to_id.clone()),
            col!(|r| r.role.clone()),
            col!(|r| r.source_artifact_id.clone()),
            col!(|r| r.source_url.clone()),
            col!(|r| r.cache_path.clone()),
            col!(|r| r.confidence.clone()),
        ],
    )
}

pub fn write_artifacts(path: &Path, rows: &[ArtifactRow]) -> Result<(), Box<dyn Error>> {
    let schema = Schema::new(vec![
        utf8_field("source_artifact_id", false),
        utf8_field("source_url", false),
        utf8_field("cache_path", false),
        utf8_field("parser_version", false),
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
            col!(|r| r.parser_version.clone()),
            col!(|r| r.scraped_at.clone()),
        ],
    )
}
