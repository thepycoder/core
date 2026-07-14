//! Supplemental distribution stats for QA summary output.

use crate::types::{CheckDetail, MeetingCoverageSnapshot};
use crawl::corpus_policy;
use crawl::corpus_policy::POLICY_DOC;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub struct QaStatsContext<'a> {
    pub coverage_snapshots: &'a [MeetingCoverageSnapshot],
    pub row_counts_path: Option<&'a Path>,
}

pub fn stats_for_check(
    check_id: &str,
    details: &[CheckDetail],
    ctx: &QaStatsContext<'_>,
) -> Option<String> {
    match check_id {
        "utterance.speech_char_coverage" => coverage_distribution_stats(ctx.coverage_snapshots),
        "schema.row_count_delta" => row_count_delta_stats(ctx.row_counts_path, details),
        "vote.source_inventory_vs_parquet" => inventory_gap_stats(details),
        "agenda.entity_count_vs_parquet" => entity_count_gap_stats(details),
        "graph.edge_endpoints_exist" => entity_type_breakdown_stats(details),
        "normalize.unresolved_persons_by_bucket" => bucket_breakdown_stats(details),
        _ => generic_meeting_kind_stats(check_id, details),
    }
}

pub fn format_all_issue_stats(
    summaries: &[crate::types::CheckSummary],
    details: &[CheckDetail],
    ctx: &QaStatsContext<'_>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for summary in summaries {
        if summary.status == "pass" || summary.check == "qa.summary_vs_detail" {
            continue;
        }
        if let Some(stats) = stats_for_check(&summary.check, details, ctx) {
            out.insert(summary.check.clone(), stats);
        }
    }
    out
}

/// Always-on corpus overview (printed even when coverage check passes).
pub fn corpus_overview(ctx: &QaStatsContext<'_>) -> Option<String> {
    let mut sections = Vec::new();
    if let Some(block) = coverage_distribution_stats(ctx.coverage_snapshots) {
        sections.push(block);
    }
    if let Some(block) = row_counts_overview(ctx.row_counts_path) {
        sections.push(block);
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

fn coverage_distribution_stats(snapshots: &[MeetingCoverageSnapshot]) -> Option<String> {
    if snapshots.is_empty() {
        return None;
    }

    let mut lines = vec![
        "**Document word coverage** (`saved_words / source_words`):".to_string(),
        String::new(),
        "| kind | n | min | p5 | median | p95 | max |".to_string(),
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: |".to_string(),
    ];

    for kind in ["commission", "plenary"] {
        let ratios: Vec<f64> = snapshots
            .iter()
            .filter(|s| s.meeting_kind == kind)
            .map(|s| s.ratio)
            .collect();
        if ratios.is_empty() {
            continue;
        }
        let dist = distribution(&ratios);
        lines.push(format!(
            "| {kind} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} |",
            dist.n, dist.min, dist.p5, dist.median, dist.p95, dist.max
        ));
    }

    let mut lowest: Vec<_> = snapshots.iter().collect();
    lowest.sort_by(|a, b| {
        a.ratio
            .partial_cmp(&b.ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    lines.push(String::new());
    lines.push("**Lowest ratios:**".to_string());
    for row in lowest.iter().take(8) {
        let annotation = corpus_class_annotation(&row.meeting_kind, &row.meeting_id);
        lines.push(format!(
            "- {} {}: {:.3} ({} / {} words){}",
            row.meeting_kind,
            row.meeting_id,
            row.ratio,
            row.saved_words,
            row.source_words,
            annotation
        ));
    }

    let policy_rows: Vec<String> = snapshots
        .iter()
        .filter_map(|row| {
            let session_id = 56;
            let kind = crawl::agenda_timeline::MeetingKind::parse(&row.meeting_kind);
            let meeting_id = row.meeting_id.parse().ok()?;
            let class = corpus_policy::classify_meeting(session_id, kind, meeting_id)?;
            Some(format!(
                "- {} {}: `{}` — {}",
                row.meeting_kind, row.meeting_id, class.class.as_str(), class.note
            ))
        })
        .collect();
    if !policy_rows.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "**Corpus policy classifications** (see `{POLICY_DOC}`):"
        ));
        lines.extend(policy_rows);
    }

    Some(lines.join("\n"))
}

/// Compact lines for stderr (one per kind + lowest outliers).
pub fn coverage_console_lines(snapshots: &[MeetingCoverageSnapshot]) -> Vec<String> {
    if snapshots.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for kind in ["plenary", "commission"] {
        let ratios: Vec<f64> = snapshots
            .iter()
            .filter(|s| s.meeting_kind == kind)
            .map(|s| s.ratio)
            .collect();
        if ratios.is_empty() {
            continue;
        }
        let dist = distribution(&ratios);
        lines.push(format!(
            "[qa] coverage {kind}: n={} min={:.3} p5={:.3} median={:.3} max={:.3}",
            dist.n, dist.min, dist.p5, dist.median, dist.max
        ));
    }
    let mut lowest: Vec<_> = snapshots.iter().collect();
    lowest.sort_by(|a, b| {
        a.ratio
            .partial_cmp(&b.ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let worst: Vec<String> = lowest
        .iter()
        .take(5)
        .map(|r| {
            format!(
                "{} {} ({:.3}){}",
                r.meeting_kind,
                r.meeting_id,
                r.ratio,
                corpus_class_annotation(&r.meeting_kind, &r.meeting_id)
            )
        })
        .collect();
    if !worst.is_empty() {
        lines.push(format!("[qa] coverage lowest: {}", worst.join(", ")));
    }
    lines
}

fn corpus_class_annotation(meeting_kind: &str, meeting_id: &str) -> String {
    let kind = crawl::agenda_timeline::MeetingKind::parse(meeting_kind);
    let id = meeting_id.parse().unwrap_or(0);
    corpus_policy::classify_meeting(56, kind, id)
        .map(|c| format!(" [{}]", c.class.as_str()))
        .unwrap_or_default()
}

struct Distribution {
    n: usize,
    min: f64,
    p5: f64,
    median: f64,
    p95: f64,
    max: f64,
}

fn distribution(values: &[f64]) -> Distribution {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    Distribution {
        n,
        min: sorted[0],
        p5: percentile_at(&sorted, 0.05),
        median: percentile_at(&sorted, 0.5),
        p95: percentile_at(&sorted, 0.95),
        max: sorted[n - 1],
    }
}

fn percentile_at(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64) * pct).floor() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn row_counts_overview(path: Option<&Path>) -> Option<String> {
    let path = path?;
    let raw = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let obj = value.as_object()?;
    if obj.is_empty() {
        return None;
    }

    let mut rows: Vec<_> = obj
        .iter()
        .map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0)))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));

    let mut lines = vec![
        "**Staging table row counts:**".to_string(),
        String::new(),
        "| table | rows |".to_string(),
        "| --- | ---: |".to_string(),
    ];
    for (table, count) in rows {
        lines.push(format!("| {table} | {count} |"));
    }
    Some(lines.join("\n"))
}

fn row_count_delta_stats(path: Option<&Path>, details: &[CheckDetail]) -> Option<String> {
    let check_details: Vec<_> = details
        .iter()
        .filter(|d| d.check_id == "schema.row_count_delta")
        .collect();
    if check_details.is_empty() {
        return row_counts_overview(path);
    }

    let mut lines = vec!["**Flagged row-count deltas (>25% and >10 rows):**".to_string()];
    for d in check_details {
        lines.push(format!(
            "- {}: {} → {} ({})",
            d.entity_id, d.expected, d.actual, d.message
        ));
    }
    Some(lines.join("\n"))
}

fn inventory_gap_stats(details: &[CheckDetail]) -> Option<String> {
    let rows: Vec<_> = details
        .iter()
        .filter(|d| d.check_id == "vote.source_inventory_vs_parquet")
        .collect();
    if rows.is_empty() {
        return None;
    }

    let count = rows.len();
    let mut total_gap = 0i64;
    let mut gaps = Vec::new();
    for d in &rows {
        let source: i64 = d.expected.parse().unwrap_or(0);
        let parquet: i64 = d.actual.parse().unwrap_or(0);
        let gap = source - parquet;
        total_gap += gap;
        gaps.push((d.meeting_id.clone(), gap, source, parquet));
    }
    gaps.sort_by(|a, b| b.1.cmp(&a.1));

    let mut lines = vec![format!(
        "**Vote inventory gaps:** {total_gap} total votes missing across {count} meetings",
    )];
    for (meeting_id, gap, source, parquet) in gaps.iter().take(8) {
        lines.push(format!(
            "- meeting {meeting_id}: source {source} vs parquet {parquet} (gap {gap})"
        ));
    }
    Some(lines.join("\n"))
}

fn entity_count_gap_stats(details: &[CheckDetail]) -> Option<String> {
    let rows: Vec<_> = details
        .iter()
        .filter(|d| d.check_id == "agenda.entity_count_vs_parquet")
        .collect();
    if rows.is_empty() {
        return None;
    }

    let mut lines = vec![format!(
        "**Agenda entity count mismatches:** {} meetings",
        rows.len()
    )];
    for d in rows.iter().take(8) {
        lines.push(format!(
            "- {} {}: source {} vs parquet {} ({})",
            d.meeting_kind, d.meeting_id, d.expected, d.actual, d.message
        ));
    }
    Some(lines.join("\n"))
}

fn entity_type_breakdown_stats(details: &[CheckDetail]) -> Option<String> {
    let rows: Vec<_> = details
        .iter()
        .filter(|d| d.check_id == "graph.edge_endpoints_exist")
        .collect();
    if rows.is_empty() {
        return None;
    }

    let count = rows.len();
    let mut by_type: HashMap<String, usize> = HashMap::new();
    for d in &rows {
        *by_type.entry(d.entity_type.clone()).or_default() += 1;
    }
    let mut pairs: Vec<_> = by_type.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1));

    let mut lines = vec![format!(
        "**Orphan edge endpoints:** {count} issues by node type",
    )];
    for (entity_type, count) in pairs {
        lines.push(format!("- {entity_type}: {count}"));
    }
    Some(lines.join("\n"))
}

fn bucket_breakdown_stats(details: &[CheckDetail]) -> Option<String> {
    let rows: Vec<_> = details
        .iter()
        .filter(|d| d.check_id == "normalize.unresolved_persons_by_bucket")
        .collect();
    if rows.is_empty() {
        return None;
    }

    let count = rows.len();
    let mut by_bucket: HashMap<String, usize> = HashMap::new();
    for d in &rows {
        let bucket = if d.entity_type.is_empty() {
            "unknown"
        } else {
            d.entity_type.as_str()
        };
        *by_bucket.entry(bucket.to_string()).or_default() += 1;
    }
    let mut pairs: Vec<_> = by_bucket.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1));

    let mut lines = vec![format!("**Unresolved persons:** {count} rows by bucket")];
    for (bucket, count) in pairs {
        lines.push(format!("- {bucket}: {count}"));
    }
    Some(lines.join("\n"))
}

fn generic_meeting_kind_stats(check_id: &str, details: &[CheckDetail]) -> Option<String> {
    let rows: Vec<_> = details
        .iter()
        .filter(|d| d.check_id == check_id && !d.meeting_kind.is_empty())
        .collect();
    if rows.len() < 3 {
        return None;
    }

    let mut by_kind: HashMap<String, usize> = HashMap::new();
    for d in rows {
        *by_kind.entry(d.meeting_kind.clone()).or_default() += 1;
    }
    let mut pairs: Vec<_> = by_kind.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1));

    let mut lines = vec!["**Issues by meeting kind:**".to_string()];
    for (kind, count) in pairs {
        lines.push(format!("- {kind}: {count}"));
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MeetingCoverageSnapshot;

    #[test]
    fn coverage_distribution_table() {
        let snapshots = vec![
            MeetingCoverageSnapshot {
                meeting_kind: "plenary".into(),
                meeting_id: "1".into(),
                source_words: 1000,
                saved_words: 900,
                ratio: 0.9,
                updated_at: String::new(),
            },
            MeetingCoverageSnapshot {
                meeting_kind: "plenary".into(),
                meeting_id: "2".into(),
                source_words: 1000,
                saved_words: 500,
                ratio: 0.5,
                updated_at: String::new(),
            },
            MeetingCoverageSnapshot {
                meeting_kind: "commission".into(),
                meeting_id: "3".into(),
                source_words: 1000,
                saved_words: 950,
                ratio: 0.95,
                updated_at: String::new(),
            },
        ];
        let stats = coverage_distribution_stats(&snapshots).unwrap();
        assert!(stats.contains("plenary"));
        assert!(stats.contains("p5"));
        assert!(stats.contains("Lowest ratios"));
    }

    #[test]
    fn inventory_gap_stats_parses_expected_actual() {
        let details = vec![
            CheckDetail::new("vote.source_inventory_vs_parquet", "warn", "warn", "gap")
                .with_meeting("plenary", "129")
                .with_values("12", "10"),
        ];
        let stats = inventory_gap_stats(&details).unwrap();
        assert!(stats.contains("gap 2"));
        assert!(stats.contains("meeting 129"));
    }
}
