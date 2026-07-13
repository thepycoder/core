use crate::types::CheckDetail;
use identity::parquet_io::{read_all_rows, read_string_column};
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

pub fn run_graph_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let mut details = Vec::new();
    let nodes_path = data_dir.join("graph/nodes.parquet");
    let edges_path = data_dir.join("graph/edges.parquet");
    if !nodes_path.exists() || !edges_path.exists() {
        return Ok(details);
    }

    let node_keys = load_node_keys(&nodes_path)?;
    let edges = load_edges(&edges_path)?;
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

    for uid in &utterance_ids {
        if !spoke_targets.contains(uid) {
            details.push(
                CheckDetail::new(
                    "graph.utterance_spoke_resolved",
                    "info",
                    "info",
                    format!("Utterance {uid} has no SPOKE edge"),
                )
                .with_entity("utterance", uid),
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
