use crate::types::CheckDetail;
use identity::parquet_io::{read_all_rows, read_string_column};
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;

const WARNING_GRAPH_TARGET_CHECK: &str = "qa.warning_graph_target";

/// Validate nonempty graph-node / source-artifact targets on finalized details.
pub fn run_warning_target_checks(
    data_dir: &Path,
    details: &[CheckDetail],
) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let node_keys = load_graph_node_keys(data_dir)?;
    let artifact_ids = load_source_artifact_ids(data_dir)?;
    let mut out = Vec::new();

    for d in details {
        if !d.graph_node_type.is_empty() || !d.graph_node_id.is_empty() {
            if d.graph_node_type.is_empty() || d.graph_node_id.is_empty() {
                out.push(target_failure(
                    d,
                    "graph_node_type and graph_node_id must both be set",
                    format!("type={} id={}", d.graph_node_type, d.graph_node_id),
                ));
            } else if looks_like_source_local_id(&d.graph_node_id) {
                out.push(target_failure(
                    d,
                    "graph_node_id must not be a source-local id",
                    &d.graph_node_id,
                ));
            } else {
                let key = (d.graph_node_type.clone(), d.graph_node_id.clone());
                if !node_keys.is_empty() && !node_keys.contains(&key) {
                    out.push(target_failure(
                        d,
                        "graph target missing from graph/nodes.parquet",
                        format!("{}/{}", d.graph_node_type, d.graph_node_id),
                    ));
                }
            }
        }

        if !d.source_artifact_id.is_empty()
            && !d.graph_node_type.is_empty()
            && !artifact_ids.is_empty()
            && !artifact_ids.contains(&d.source_artifact_id)
        {
            out.push(target_failure(
                d,
                "source_artifact_id missing from graph/source_artifacts.parquet",
                &d.source_artifact_id,
            ));
        }
    }

    Ok(out)
}

fn target_failure(
    subject: &CheckDetail,
    expected: impl Into<String>,
    actual: impl Into<String>,
) -> CheckDetail {
    let expected = expected.into();
    let actual = actual.into();
    CheckDetail::new(
        WARNING_GRAPH_TARGET_CHECK,
        "error",
        "fail",
        format!("invalid warning target on {}: {expected}", subject.check_id),
    )
    .with_entity(&subject.entity_type, &subject.entity_id)
    .with_values(&expected, &actual)
    .with_source(&subject.source_url, &subject.cache_path)
    .with_source_block(&subject.source_block)
    .with_warning_kind("integrity")
    .finalize()
}

fn looks_like_source_local_id(id: &str) -> bool {
    // Source-local appendix occurrence keys look like `16#1`.
    id.contains('#')
}

fn load_graph_node_keys(data_dir: &Path) -> Result<HashSet<(String, String)>, Box<dyn Error>> {
    let path = data_dir.join("graph/nodes.parquet");
    let mut keys = HashSet::new();
    if !path.exists() {
        return Ok(keys);
    }
    for batch in read_all_rows(&path)? {
        let types = read_string_column(&batch, "node_type")?;
        let ids = read_string_column(&batch, "node_id")?;
        for i in 0..batch.num_rows() {
            keys.insert((types[i].clone(), ids[i].clone()));
        }
    }
    Ok(keys)
}

fn load_source_artifact_ids(data_dir: &Path) -> Result<HashSet<String>, Box<dyn Error>> {
    let path = data_dir.join("graph/source_artifacts.parquet");
    let mut ids = HashSet::new();
    if !path.exists() {
        return Ok(ids);
    }
    for batch in read_all_rows(&path)? {
        let col = "source_artifact_id";
        if batch.schema().index_of(col).is_err() {
            continue;
        }
        let values = read_string_column(&batch, col)?;
        for v in values {
            if !v.is_empty() {
                ids.insert(v);
            }
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_local_graph_id_fails() {
        let subject = CheckDetail::new("vote.appendix_bucket_counts", "warn", "warn", "x")
            .with_graph_node("VoteResult", "1#1")
            .finalize();
        let failures =
            run_warning_target_checks(Path::new("/tmp/nonexistent-qa-data"), &[subject]).unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].check_id, WARNING_GRAPH_TARGET_CHECK);
    }

    #[test]
    fn valid_empty_targets_pass() {
        let subject = CheckDetail::new("vote.appendix_bucket_counts", "warn", "warn", "x")
            .with_entity("vote_result", "1#1")
            .finalize();
        let failures =
            run_warning_target_checks(Path::new("/tmp/nonexistent-qa-data"), &[subject]).unwrap();
        assert!(failures.is_empty());
    }
}
