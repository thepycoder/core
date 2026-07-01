use crawl::paths::data_dir;
use graph::{build_graph, write_artifacts, write_edges, write_nodes};
use std::collections::HashMap;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let root = data_dir();
    let out_dir = root.join("graph");
    std::fs::create_dir_all(&out_dir)?;

    let build = build_graph(&root)?;

    write_nodes(&out_dir.join("nodes.parquet"), &build.nodes)?;
    write_edges(&out_dir.join("edges.parquet"), &build.edges)?;
    write_artifacts(
        &out_dir.join("source_artifacts.parquet"),
        &build.artifacts,
    )?;

    let mut edge_counts: HashMap<String, usize> = HashMap::new();
    let mut node_counts: HashMap<String, usize> = HashMap::new();
    for edge in &build.edges {
        *edge_counts.entry(edge.edge_type.clone()).or_default() += 1;
    }
    for node in &build.nodes {
        *node_counts.entry(node.node_type.clone()).or_default() += 1;
    }

    println!("[graph] nodes: {}", build.nodes.len());
    for (kind, count) in node_counts.iter() {
        println!("[graph]   {kind}: {count}");
    }
    println!("[graph] edges: {}", build.edges.len());
    for (kind, count) in edge_counts.iter() {
        println!("[graph]   {kind}: {count}");
    }
    println!("[graph] source artifacts: {}", build.artifacts.len());
    println!("[graph] output dir: {}", out_dir.display());

    Ok(())
}
