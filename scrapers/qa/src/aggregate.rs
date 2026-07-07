use crate::check_catalog::check_doc;
use crate::types::{table_for_check_id, worst_status, CheckDetail, CheckSummary};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;

const META_CHECK_ID: &str = "qa.summary_vs_detail";
const MAX_EXAMPLES: usize = 3;

pub fn aggregate_details(details: &[CheckDetail]) -> Result<Vec<CheckSummary>, Box<dyn Error>> {
    let mut by_check: HashMap<String, Vec<&CheckDetail>> = HashMap::new();
    for d in details {
        by_check.entry(d.check_id.clone()).or_default().push(d);
    }

    let mut summaries: Vec<CheckSummary> = by_check
        .iter()
        .map(|(check_id, rows)| {
            let count = rows.len();
            let status = if count == 0 {
                "pass"
            } else {
                worst_status(rows.iter().map(|r| r.status.as_str()))
            };
            let examples: Vec<String> = rows
                .iter()
                .take(MAX_EXAMPLES)
                .map(|r| {
                    format!(
                        "{} | {} | {}",
                        r.entity_id,
                        r.expected,
                        r.actual
                    )
                })
                .collect();
            CheckSummary {
                table: table_for_check_id(check_id),
                check: check_id.clone(),
                status: status.to_string(),
                count,
                detail: if count == 0 {
                    "no issues".to_string()
                } else {
                    format!("{count} issue(s)")
                },
                examples: examples.join("; "),
            }
        })
        .collect();

    // Checks with zero detail rows still deserve a pass row when registered.
    summaries.sort_by(|a, b| a.check.cmp(&b.check));

    // S8 meta-check: summary count must match detail count per check_id.
    let mut meta_failures = Vec::new();
    for summary in &summaries {
        let detail_count = by_check.get(&summary.check).map(|v| v.len()).unwrap_or(0);
        if summary.count != detail_count {
            meta_failures.push(format!(
                "{}: summary count {} != detail count {}",
                summary.check, summary.count, detail_count
            ));
        }
    }

    if meta_failures.is_empty() {
        summaries.push(CheckSummary {
            table: "qa".to_string(),
            check: META_CHECK_ID.to_string(),
            status: "pass".to_string(),
            count: 0,
            detail: "summary counts match detail rows".to_string(),
            examples: String::new(),
        });
    } else {
        summaries.push(CheckSummary {
            table: "qa".to_string(),
            check: META_CHECK_ID.to_string(),
            status: "fail".to_string(),
            count: meta_failures.len(),
            detail: meta_failures.join(" | "),
            examples: meta_failures
                .iter()
                .take(MAX_EXAMPLES)
                .cloned()
                .collect::<Vec<_>>()
                .join("; "),
        });
    }

    Ok(summaries)
}

pub fn write_summary_md(path: &Path, summaries: &[CheckSummary]) -> Result<(), Box<dyn Error>> {
    let issues: Vec<_> = summaries
        .iter()
        .filter(|s| s.status != "pass" && s.check != META_CHECK_ID)
        .collect();
    let passes: Vec<_> = summaries
        .iter()
        .filter(|s| s.status == "pass")
        .collect();

    let mut out = String::new();
    out.push_str("# QA summary\n\n");
    out.push_str(&format!(
        "Generated: {}\n\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    ));
    out.push_str(&format!("- **Issues:** {}\n", issues.len()));
    out.push_str(&format!("- **Passes:** {}\n\n", passes.len()));

    if !issues.is_empty() {
        out.push_str("## Issues\n\n");
        for s in issues {
            let doc = check_doc(&s.check);
            out.push_str(&format!(
                "- **{} / {}** (`{}`, count={}): {}\n",
                s.table, s.check, s.status, s.count, s.detail
            ));
            out.push_str(&format!("  - **What:** {}\n", doc.what));
            if !doc.measures.is_empty() {
                out.push_str(&format!("  - **Measures:** {}\n", doc.measures));
            }
            if !s.examples.is_empty() {
                out.push_str("  - **Examples:**\n");
                for ex in s.examples.split("; ") {
                    if !ex.is_empty() {
                        out.push_str(&format!("    - `{ex}`\n"));
                    }
                }
            }
            out.push('\n');
        }
    }

    let passing: Vec<_> = passes
        .iter()
        .filter(|s| s.check != META_CHECK_ID)
        .collect();
    if !passing.is_empty() {
        out.push_str("## Passing checks\n\n");
        for s in passing {
            let doc = check_doc(&s.check);
            out.push_str(&format!(
                "- **{} / {}** — {}\n",
                s.table, s.check, doc.what
            ));
        }
        out.push('\n');
    }

    let meta = summaries.iter().find(|s| s.check == META_CHECK_ID);
    if let Some(m) = meta {
        let doc = check_doc(&m.check);
        out.push_str(&format!("## Meta ({})\n\n", m.check));
        out.push_str(&format!("- **What:** {}\n", doc.what));
        if !doc.measures.is_empty() {
            out.push_str(&format!("- **Measures:** {}\n", doc.measures));
        }
        out.push_str(&format!("- **Result:** {}\n", m.detail));
    }

    fs::write(path, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CheckDetail;

    #[test]
    fn summary_count_matches_detail_rows() {
        let details = vec![
            CheckDetail::new(
                "vote.compact_total_vs_member_names",
                "warn",
                "warn",
                "mismatch",
            )
            .with_entity("vote", "56_129_4"),
            CheckDetail::new(
                "vote.compact_total_vs_member_names",
                "warn",
                "warn",
                "mismatch 2",
            )
            .with_entity("vote", "56_133_18"),
        ];
        let summaries = aggregate_details(&details).unwrap();
        let vote = summaries
            .iter()
            .find(|s| s.check == "vote.compact_total_vs_member_names")
            .unwrap();
        assert_eq!(vote.count, 2);
        let meta = summaries.iter().find(|s| s.check == META_CHECK_ID).unwrap();
        assert_eq!(meta.status, "pass");
    }
}
