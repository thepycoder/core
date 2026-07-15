use crate::types::CheckDetail;
use identity::parquet_io::{read_all_rows, read_string_column};
use normalize::SESSION_ID;
use std::error::Error;
use std::path::Path;

const CHECK_ID: &str = "vote.compact_total_vs_member_names";

pub fn run_vote_reconciliation_checks(data_dir: &Path) -> Result<Vec<CheckDetail>, Box<dyn Error>> {
    let path = data_dir.join("normalized/vote_reconciliation.parquet");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut details = Vec::new();
    for batch in read_all_rows(&path)? {
        let vote_ids = read_string_column(&batch, "result_id")?;
        let session_ids = read_string_column(&batch, "session_id")?;
        let meeting_ids = read_string_column(&batch, "meeting_id")?;
        let yes = read_string_column(&batch, "yes")?;
        let no = read_string_column(&batch, "no")?;
        let abstain = read_string_column(&batch, "abstain")?;
        let members_yes = read_string_column(&batch, "members_yes_count")?;
        let members_no = read_string_column(&batch, "members_no_count")?;
        let members_abstain = read_string_column(&batch, "members_abstain_count")?;
        let reconciled = read_string_column(&batch, "reconciled")?;
        let source_urls = read_string_column(&batch, "source_url")?;
        let cache_paths = read_string_column(&batch, "cache_path")?;

        for i in 0..batch.num_rows() {
            if reconciled[i] == "true" {
                continue;
            }
            details.push(
                CheckDetail::new(
                    CHECK_ID,
                    "warn",
                    "warn",
                    format!(
                        "result {} headline yes={}/no={}/abstain={} vs members {}/{}/{}",
                        vote_ids[i],
                        yes[i],
                        no[i],
                        abstain[i],
                        members_yes[i],
                        members_no[i],
                        members_abstain[i]
                    ),
                )
                .with_session(&session_ids[i])
                .with_meeting("plenary", &meeting_ids[i])
                .with_entity("vote_result", &vote_ids[i])
                .with_graph_node("VoteResult", &vote_ids[i])
                .with_warning_kind("source_conflict")
                .with_values(
                    format!("yes={} no={} abstain={}", yes[i], no[i], abstain[i]),
                    format!(
                        "members_yes={} members_no={} members_abstain={}",
                        members_yes[i], members_no[i], members_abstain[i]
                    ),
                )
                .with_source(&source_urls[i], &cache_paths[i]),
            );
        }
    }

    if details.is_empty() && path.exists() {
        // Emit nothing — pass is implicit via zero detail rows.
        let _ = SESSION_ID;
    }

    Ok(details)
}
