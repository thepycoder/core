use crate::io::{read_check_summaries, write_check_summaries};
use crate::types::{CheckSummary, status_rank};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

#[derive(Debug)]
pub struct BaselineDiff {
    pub regressions: Vec<String>,
}

pub fn compare_to_baseline(
    current: &[CheckSummary],
    baseline_path: &Path,
) -> Result<BaselineDiff, Box<dyn Error>> {
    let baseline = read_check_summaries(baseline_path)?;
    let baseline_by_check: HashMap<String, &CheckSummary> =
        baseline.iter().map(|s| (s.check.clone(), s)).collect();

    let mut regressions = Vec::new();

    for cur in current {
        if cur.check == "qa.summary_vs_detail" {
            if cur.status == "fail" {
                regressions.push(format!("meta-check failed: {}", cur.detail));
            }
            continue;
        }

        let Some(base) = baseline_by_check.get(&cur.check) else {
            if cur.status != "pass" && cur.count > 0 {
                regressions.push(format!(
                    "new check {}: status={} count={}",
                    cur.check, cur.status, cur.count
                ));
            }
            continue;
        };

        if status_rank(&cur.status) > status_rank(&base.status) {
            regressions.push(format!(
                "{} status regressed: {} -> {}",
                cur.check, base.status, cur.status
            ));
        }

        if cur.count > base.count && matches!(cur.status.as_str(), "fail" | "error" | "warn") {
            regressions.push(format!(
                "{} count increased: {} -> {}",
                cur.check, base.count, cur.count
            ));
        }
    }

    Ok(BaselineDiff { regressions })
}

pub fn update_baseline(
    current: &[CheckSummary],
    baseline_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let filtered: Vec<CheckSummary> = current
        .iter()
        .filter(|s| s.check != "qa.summary_vs_detail")
        .cloned()
        .collect();
    write_check_summaries(baseline_path, &filtered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CheckSummary;

    #[test]
    fn detects_status_regression() {
        let baseline = vec![CheckSummary {
            table: "vote".to_string(),
            check: "vote.compact_total_vs_member_names".to_string(),
            status: "warn".to_string(),
            count: 6,
            detail: String::new(),
            examples: String::new(),
        }];
        let current = vec![CheckSummary {
            table: "vote".to_string(),
            check: "vote.compact_total_vs_member_names".to_string(),
            status: "fail".to_string(),
            count: 8,
            detail: String::new(),
            examples: String::new(),
        }];

        let tmp = std::env::temp_dir().join("qa_baseline_test.parquet");
        write_check_summaries(&tmp, &baseline).unwrap();
        let diff = compare_to_baseline(&current, &tmp).unwrap();
        assert!(!diff.regressions.is_empty());
        let _ = std::fs::remove_file(tmp);
    }
}
