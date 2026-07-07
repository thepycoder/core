pub mod aggregate;
pub mod agenda_checks;
pub mod baseline;
pub mod check_catalog;
pub mod graph;
pub mod infrastructure;
pub mod io;
pub mod remaining;
pub mod schema;
pub mod speakers;
pub mod speech;
pub mod types;
pub mod vote_source;
pub mod votes;

use aggregate::{aggregate_details, write_summary_md};
use crawl::paths::data_dir;
use identity::actor_resolver::ActorResolver;
use identity::resolver::Resolver;
use io::{write_alias_candidates, write_check_details, write_check_summaries};
use normalize::common::UnresolvedRow;
use normalize::normalize_utterances;
use std::error::Error;
use std::path::Path;
use types::{AliasCandidate, CheckDetail, CheckSummary};

pub use types::{CheckDetail as QaCheckDetail, CheckSummary as QaCheckSummary};

pub const SESSION_ID: &str = "56";

pub struct QaRunOptions {
    pub strict: bool,
    pub update_baseline: bool,
    pub tier_filter: Option<String>,
    pub check_filter: Option<String>,
}

pub struct QaRunResult {
    pub details: Vec<CheckDetail>,
    pub summaries: Vec<CheckSummary>,
    pub alias_candidates: Vec<AliasCandidate>,
    pub strict_failed: bool,
    pub regressions: Vec<String>,
}

pub fn run_qa(opts: &QaRunOptions) -> Result<QaRunResult, Box<dyn Error>> {
    let data_root = data_dir();
    let qa_dir = data_root.join("qa");
    std::fs::create_dir_all(&qa_dir)?;

    let mut details = Vec::new();
    let mut alias_candidates = Vec::new();

    // Speaker checks need resolvers + utterance/unresolved data
    let resolver = Resolver::load(&data_root)?;
    let actor_resolver = ActorResolver::load(&data_root)?;
    let utterances_out = normalize_utterances(&data_root, &actor_resolver)?;
    let unresolved = load_unresolved(&data_root)?;
    let speaker_out = speakers::run_speaker_checks(
        &data_root,
        &utterances_out.rows,
        &unresolved,
        &actor_resolver,
        &resolver,
    )?;
    details.extend(speaker_out.details);
    alias_candidates.extend(speaker_out.alias_candidates);

    details.extend(votes::run_vote_reconciliation_checks(&data_root)?);
    details.extend(graph::run_graph_checks(&data_root)?);
    details.extend(infrastructure::run_infrastructure_checks(&data_root)?);
    details.extend(vote_source::run_vote_source_checks(&data_root)?);
    details.extend(agenda_checks::run_agenda_checks(&data_root)?);
    details.extend(speech::run_speech_checks(&data_root)?);
    details.extend(remaining::run_remaining_checks(&data_root)?);
    details.extend(schema::run_schema_checks(&data_root, &qa_dir)?);

    if let Some(tier) = &opts.tier_filter {
        details.retain(|d| d.check_id.starts_with(tier));
    }
    if let Some(check) = &opts.check_filter {
        details.retain(|d| d.check_id == *check);
    }

    let summaries = aggregate_details(&details)?;

    if opts.update_baseline {
        baseline::update_baseline(&summaries, &qa_dir.join("checks_baseline.parquet"))?;
        eprintln!("[qa] baseline updated at {}", qa_dir.join("checks_baseline.parquet").display());
    }

    write_check_details(
        &qa_dir.join("meeting_report_check_details.parquet"),
        &details,
    )?;
    write_check_summaries(&qa_dir.join("checks.parquet"), &summaries)?;
    write_alias_candidates(&qa_dir.join("alias_candidates.parquet"), &alias_candidates)?;
    write_summary_md(&qa_dir.join("summary.md"), &summaries)?;

    let regressions = if qa_dir.join("checks_baseline.parquet").exists() {
        baseline::compare_to_baseline(&summaries, &qa_dir.join("checks_baseline.parquet"))?
            .regressions
    } else {
        Vec::new()
    };

    let strict_failed = opts.strict && !regressions.is_empty();

    print_summary(&summaries, details.len(), &regressions);

    Ok(QaRunResult {
        details,
        summaries,
        alias_candidates,
        strict_failed,
        regressions,
    })
}

fn load_unresolved(data_dir: &Path) -> Result<Vec<UnresolvedRow>, Box<dyn Error>> {
    let path = data_dir.join("normalized/unresolved_persons.parquet");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for batch in identity::parquet_io::read_all_rows(&path)? {
        let raw_names = identity::parquet_io::read_string_column(&batch, "raw_name")?;
        let typo = identity::parquet_io::read_string_column(&batch, "typo_corrected")?;
        let norm_primary = identity::parquet_io::read_string_column(&batch, "norm_primary")?;
        let norm_reordered = identity::parquet_io::read_string_column(&batch, "norm_reordered")?;
        let reasons = identity::parquet_io::read_string_column(&batch, "reason")?;
        let buckets = identity::parquet_io::read_string_column(&batch, "source_bucket")?;
        let roles = identity::parquet_io::read_string_column(&batch, "role")?;
        let context_ids = identity::parquet_io::read_string_column(&batch, "context_id")?;
        let context_labels = identity::parquet_io::read_string_column(&batch, "context_label")?;
        let raw_fields = identity::parquet_io::read_string_column(&batch, "raw_field")?;
        let source_urls = identity::parquet_io::read_string_column(&batch, "source_url")?;
        let cache_paths = identity::parquet_io::read_string_column(&batch, "cache_path")?;
        for i in 0..batch.num_rows() {
            rows.push(UnresolvedRow {
                raw_name: raw_names[i].clone(),
                typo_corrected: typo[i].clone(),
                norm_primary: norm_primary[i].clone(),
                norm_reordered: norm_reordered[i].clone(),
                reason: reasons[i].clone(),
                source_bucket: buckets[i].clone(),
                role: roles[i].clone(),
                context_id: context_ids[i].clone(),
                context_label: context_labels[i].clone(),
                raw_field: raw_fields[i].clone(),
                source_url: source_urls[i].clone(),
                cache_path: cache_paths[i].clone(),
            });
        }
    }
    Ok(rows)
}

fn print_summary(summaries: &[CheckSummary], detail_count: usize, regressions: &[String]) {
    let issues: Vec<_> = summaries
        .iter()
        .filter(|s| s.status != "pass" && s.check != "qa.summary_vs_detail")
        .collect();
    let passes: Vec<_> = summaries.iter().filter(|s| s.status == "pass").collect();
    eprintln!(
        "[qa] {} detail rows, {} checks ({} issues, {} passes)",
        detail_count,
        summaries.len(),
        issues.len(),
        passes.len()
    );
    for s in issues.iter().take(20) {
        eprintln!("[qa] {} {}: {} ({})", s.table, s.check, s.status, s.detail);
    }
    if !regressions.is_empty() {
        eprintln!("[qa] {} baseline regression(s):", regressions.len());
        for r in regressions {
            eprintln!("[qa]   - {r}");
        }
    }
}

pub fn registered_check_ids() -> Vec<&'static str> {
    vec![
        "vote.compact_total_vs_member_names",
        "vote.appendix_bucket_vs_collected_names",
        "vote.compact_tables_vs_appendix_headers",
        "vote.source_inventory_vs_parquet",
        "vote.duplicate_person_across_buckets",
        "vote.cast_count_vs_headline",
        "vote.number_sequence",
        "graph.edge_endpoints_exist",
        "graph.voted_on_orphan_targets",
        "graph.utterance_spoke_resolved",
        "graph.external_on_mp_only_edges",
        "graph.orphan_external_person",
        "utterance.unique_ids",
        "utterance.source_markers_vs_normalized",
        "utterance.roundtrip_discussion",
        "speaker.cleaned_re_resolves",
        "speaker.utterance_id_duplicates",
        "speaker.digit_prefix_resolvable",
        "artifact.scraped_at_populated",
        "commission.meeting_gaps",
        "normalize.unresolved_persons_by_bucket",
        "source.cache_exists",
        "agenda.entity_count_vs_parquet",
        "agenda.hearing_not_extracted",
        "question.grouped_internal_ids_complete",
        "dossier.ref_exists",
        "meeting.chair_source_vs_parquet",
        "meeting.date_source_vs_parquet",
        "meeting.times_source_vs_parquet",
        "vote.total_plausibility",
        "vote.motion_id_when_referenced",
        "fk.questions_votes_to_meetings",
        "source.encoding_bytes",
        "person.speaker_without_vote",
        "schema.table_loaded",
        "schema.row_count_delta",
        "schema.required_non_empty",
        "schema.unique_keys",
        "schema.internal_ids_present",
        "qa.summary_vs_detail",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_check_ids_snapshot() {
        let ids = registered_check_ids();
        assert!(ids.contains(&"vote.compact_total_vs_member_names"));
        assert!(ids.contains(&"qa.summary_vs_detail"));
        assert!(ids.len() >= 30);
    }
}
