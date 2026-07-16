use crate::{SESSION_ID, types::CheckDetail};
use crawl::utils::is_flwb_document_id;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::{MissingSpokeKind, classify_missing_spoke};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::Path;

struct EdgeRow {
    edge_type: String,
    from_type: String,
    from_id: String,
    to_type: String,
    to_id: String,
}

struct UtteranceSpeakerMeta {
    raw_speaker: String,
    speaker_role: String,
    speaker_person_id: String,
    speaker_entity_id: String,
}

pub fn run_graph_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let nodes_path = data_dir.join("graph/nodes.parquet");
    let edges_path = data_dir.join("graph/edges.parquet");
    if !nodes_path.exists() || !edges_path.exists() {
        return Ok(details);
    }

    let node_keys = load_node_keys(&nodes_path)?;
    let edges = load_edges(&edges_path)?;
    details.extend(check_native_document_ids(data_dir)?);
    details.extend(check_vote_result_edges_match_staging(data_dir, &edges)?);
    let utterance_ids: HashSet<String> = node_keys
        .iter()
        .filter(|(t, _)| t == "Utterance")
        .map(|(_, id)| id.clone())
        .collect();
    let external_ids: HashSet<String> = node_keys
        .iter()
        .filter(|(t, _)| t == "ExternalPerson")
        .map(|(_, id)| id.clone())
        .collect();

    let mut spoke_targets: HashSet<String> = HashSet::new();
    for e in &edges {
        if e.edge_type == "SPOKE" && e.to_type == "Utterance" {
            spoke_targets.insert(e.to_id.clone());
        }

        if !node_keys.contains(&(e.from_type.clone(), e.from_id.clone())) {
            details.push(
                CheckDetail::new(
                    "graph.edge_endpoints_exist",
                    "error",
                    "fail",
                    format!(
                        "edge {} missing from node {}:{}",
                        e.edge_type, e.from_type, e.from_id
                    ),
                )
                .with_entity(
                    "edge",
                    &format!("{}:{}->{}:{}", e.from_type, e.from_id, e.to_type, e.to_id),
                ),
            );
        }

        if !node_keys.contains(&(e.to_type.clone(), e.to_id.clone())) {
            if e.edge_type == "VOTED_ON" {
                details.push(
                    CheckDetail::new(
                        "graph.voted_on_orphan_targets",
                        "warn",
                        "warn",
                        format!("VOTED_ON target {}:{} not in nodes", e.to_type, e.to_id),
                    )
                    .with_entity(&e.to_type.to_lowercase(), &e.to_id),
                );
            } else {
                details.push(
                    CheckDetail::new(
                        "graph.edge_endpoints_exist",
                        "error",
                        "fail",
                        format!(
                            "edge {} missing to node {}:{}",
                            e.edge_type, e.to_type, e.to_id
                        ),
                    )
                    .with_entity(
                        "edge",
                        &format!("{}:{}->{}:{}", e.from_type, e.from_id, e.to_type, e.to_id),
                    ),
                );
            }
        }

        if e.from_type == "ExternalPerson"
            && matches!(
                e.edge_type.as_str(),
                "MEMBER_OF" | "CAST" | "ASKED" | "HOLDS_ROLE"
            )
        {
            details.push(
                CheckDetail::new(
                    "graph.external_on_mp_only_edges",
                    "error",
                    "fail",
                    format!(
                        "ExternalPerson {} on MP-only edge {}",
                        e.from_id, e.edge_type
                    ),
                )
                .with_entity("ExternalPerson", &e.from_id),
            );
        }

        if e.from_type == "ExternalPerson" && !external_ids.contains(&e.from_id) {
            details.push(
                CheckDetail::new(
                    "graph.orphan_external_person",
                    "error",
                    "fail",
                    format!("ExternalPerson {} missing from nodes", e.from_id),
                )
                .with_entity("ExternalPerson", &e.from_id),
            );
        }
    }

    let utterance_meta = load_utterance_speaker_meta(data_dir)?;

    for uid in &utterance_ids {
        if !spoke_targets.contains(uid) {
            let kind = match utterance_meta.get(uid) {
                Some(meta) => classify_missing_spoke(
                    Some(&meta.raw_speaker),
                    Some(&meta.speaker_role),
                    &meta.speaker_person_id,
                    &meta.speaker_entity_id,
                ),
                None => MissingSpokeKind::MissingNormalizedRow,
            };
            let (severity, status) = if kind == MissingSpokeKind::ResolvedWithoutSpoke {
                ("warn", "warn")
            } else {
                ("info", "info")
            };
            let raw = utterance_meta
                .get(uid)
                .map(|m| m.raw_speaker.as_str())
                .unwrap_or("");
            details.push(
                CheckDetail::new(
                    "graph.utterance_spoke_resolved",
                    severity,
                    status,
                    format!("Utterance {uid} has no SPOKE edge ({})", kind.label()),
                )
                .with_entity("utterance", uid)
                .with_values(kind.label(), raw),
            );
        }
    }

    // B11 duplicate utterance ids in normalized layer
    let utterances_path = data_dir.join("normalized/utterances.parquet");
    if utterances_path.exists() {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for batch in read_all_rows(&utterances_path)? {
            let ids = read_string_column(&batch, "utterance_id")?;
            for id in ids {
                *counts.entry(id).or_default() += 1;
            }
        }
        for (id, count) in counts {
            if count > 1 {
                details.push(
                    CheckDetail::new(
                        "utterance.unique_ids",
                        "warn",
                        "warn",
                        format!("utterance_id {id} appears {count} times"),
                    )
                    .with_entity("utterance", &id)
                    .with_values("1", count.to_string()),
                );
            }
        }
    }

    Ok(details)
}

fn check_native_document_ids(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/subdocuments.parquet"));
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let ids = read_string_column(&batch, "id")?;
        for id in ids {
            if !is_flwb_document_id(&id) {
                details.push(
                    CheckDetail::new(
                        "graph.document_id_native",
                        "error",
                        "fail",
                        format!("subdocument id {id} is not a native FLWB document id"),
                    )
                    .with_entity("Document", &id),
                );
            }
        }
    }
    Ok(details)
}

fn check_vote_result_edges_match_staging(
    data_dir: &Path,
    edges: &[EdgeRow],
) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join(format!("sessions/{SESSION_ID}/plenary/votes.parquet"));
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut graph_targets: HashMap<String, Vec<String>> = HashMap::new();
    for edge in edges {
        if edge.edge_type == "HAS_RESULT"
            && edge.from_type == "Vote"
            && edge.to_type == "VoteResult"
        {
            graph_targets
                .entry(edge.from_id.clone())
                .or_default()
                .push(edge.to_id.clone());
        }
    }
    for targets in graph_targets.values_mut() {
        targets.sort();
        targets.dedup();
    }

    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let vote_ids = read_string_column(&batch, "vote_id")?;
        let result_ids = read_string_column(&batch, "result_id")?;
        for i in 0..batch.num_rows() {
            let actual = graph_targets.get(&vote_ids[i]).cloned().unwrap_or_default();
            if actual.len() != 1 || actual[0] != result_ids[i] {
                details.push(
                    CheckDetail::new(
                        "graph.vote_result_edges_match_staging",
                        "error",
                        "fail",
                        format!(
                            "Vote {} stages result {} but graph HAS_RESULT targets [{}]",
                            vote_ids[i],
                            result_ids[i],
                            actual.join(", ")
                        ),
                    )
                    .with_entity("Vote", &vote_ids[i])
                    .with_values(&result_ids[i], actual.join(", ")),
                );
            }
        }
    }
    Ok(details)
}

fn load_node_keys(path: &Path) -> Result<HashSet<(String, String)>, Box<dyn Error>> {
    let mut keys = HashSet::new();
    for batch in read_all_rows(path)? {
        let types = read_string_column(&batch, "node_type")?;
        let ids = read_string_column(&batch, "node_id")?;
        for i in 0..batch.num_rows() {
            keys.insert((types[i].clone(), ids[i].clone()));
        }
    }
    Ok(keys)
}

fn load_edges(path: &Path) -> Result<Vec<EdgeRow>, Box<dyn Error>> {
    let mut edges = Vec::new();
    for batch in read_all_rows(path)? {
        let edge_types = read_string_column(&batch, "edge_type")?;
        let from_types = read_string_column(&batch, "from_type")?;
        let from_ids = read_string_column(&batch, "from_id")?;
        let to_types = read_string_column(&batch, "to_type")?;
        let to_ids = read_string_column(&batch, "to_id")?;
        for i in 0..batch.num_rows() {
            edges.push(EdgeRow {
                edge_type: edge_types[i].clone(),
                from_type: from_types[i].clone(),
                from_id: from_ids[i].clone(),
                to_type: to_types[i].clone(),
                to_id: to_ids[i].clone(),
            });
        }
    }
    Ok(edges)
}

fn load_utterance_speaker_meta(
    data_dir: &Path,
) -> Result<HashMap<String, UtteranceSpeakerMeta>, Box<dyn Error>> {
    let path = data_dir.join("normalized/utterances.parquet");
    let mut out = HashMap::new();
    if !path.exists() {
        return Ok(out);
    }
    for batch in read_all_rows(&path)? {
        let ids = read_string_column(&batch, "utterance_id")?;
        let raw_speakers = read_string_column(&batch, "raw_speaker")?;
        let speaker_roles = read_string_column(&batch, "speaker_role")?;
        let speaker_person_ids = read_string_column(&batch, "speaker_person_id")?;
        let speaker_entity_ids = read_string_column(&batch, "speaker_entity_id")?;
        for i in 0..batch.num_rows() {
            out.insert(
                ids[i].clone(),
                UtteranceSpeakerMeta {
                    raw_speaker: raw_speakers[i].clone(),
                    speaker_role: speaker_roles[i].clone(),
                    speaker_person_id: speaker_person_ids[i].clone(),
                    speaker_entity_id: speaker_entity_ids[i].clone(),
                },
            );
        }
    }
    Ok(out)
}
