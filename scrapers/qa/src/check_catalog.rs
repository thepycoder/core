//! Human-readable descriptions for QA checks (used in summary.md).

#[derive(Debug, Clone, Copy)]
pub struct CheckDoc {
    /// One-line: what problem this check surfaces.
    pub what: &'static str,
    /// How the check decides — inputs compared, thresholds, etc.
    pub measures: &'static str,
}

pub fn check_doc(check_id: &str) -> CheckDoc {
    match check_id {
        "vote.compact_total_vs_member_names" => CheckDoc {
            what: "Headline yes/no/abstain totals on a vote row do not match the number of named members in the appendix buckets.",
            measures: "Reads `normalized/vote_reconciliation.parquet`; flags rows where `reconciled` is false (compact totals vs `members_*_count`).",
        },
        "vote.appendix_bucket_vs_collected_names" => CheckDoc {
            what: "The count of comma-separated names in `members_yes` disagrees with the headline `yes` total for that vote.",
            measures: "Splits `members_yes` CSV on staging vote rows and compares length to parsed headline `yes`.",
        },
        "vote.compact_tables_vs_appendix_headers" => CheckDoc {
            what: "A vote number appears in the meeting-report appendix but not in the compact vote tables at the top of the page.",
            measures: "Re-parses cached vote HTML (`vote_inventory`) and compares appendix vs compact vote-number sets per meeting.",
        },
        "vote.source_inventory_vs_parquet" => CheckDoc {
            what: "The number of votes found in source HTML does not match the number of vote rows stored for that meeting.",
            measures: "Counts vote numbers in cached HTML inventory vs rows in `sessions/56/plenary/votes.parquet` per `meeting_id`.",
        },
        "vote.duplicate_person_across_buckets" => CheckDoc {
            what: "The same MP appears in more than one position bucket (yes/no/abstain) for a single vote.",
            measures: "Groups `normalized/vote_casts.parquet` by vote and person; flags persons with multiple positions.",
        },
        "vote.cast_count_vs_headline" => CheckDoc {
            what: "Resolved CAST edges per bucket do not add up to the headline yes/no/abstain totals.",
            measures: "Counts distinct `person_id` per position in `vote_casts.parquet` vs headline fields on the staging vote row.",
        },
        "vote.number_sequence" => CheckDoc {
            what: "Vote numbers in a meeting report skip a value in the expected 1..N sequence.",
            measures: "Lists vote numbers from HTML inventory and reports gaps in the numeric sequence.",
        },
        "vote.total_plausibility" => CheckDoc {
            what: "Headline yes + no + abstain exceeds a plausible chamber size (>150).",
            measures: "Sums parsed headline totals on staging vote rows; warns when total > 150.",
        },
        "vote.motion_id_when_referenced" => CheckDoc {
            what: "Vote title text references a motion but `motion_id` is empty on the staging row.",
            measures: "Matches `title_nl` against motie/motion keywords; requires non-empty `motion_id`.",
        },
        "graph.edge_endpoints_exist" => CheckDoc {
            what: "A graph edge points to a `from` or `to` node id that does not exist in `graph/nodes.parquet`.",
            measures: "Loads all edges and verifies both endpoints exist in the node key set (orphan `VOTED_ON` targets use a separate check).",
        },
        "graph.voted_on_orphan_targets" => CheckDoc {
            what: "A `VOTED_ON` edge targets a vote node id that is missing from the graph.",
            measures: "Finds `VOTED_ON` edges whose `to_id` is not present among graph Vote nodes.",
        },
        "graph.utterance_spoke_resolved" => CheckDoc {
            what: "An Utterance node has no incoming `SPOKE` edge from a resolved Person.",
            measures: "Collects all Utterance node ids and subtracts those referenced as `SPOKE` targets.",
        },
        "graph.external_on_mp_only_edges" => CheckDoc {
            what: "An ExternalPerson is the source of an edge type reserved for chamber MPs (MEMBER_OF, CAST, ASKED, HOLDS_ROLE).",
            measures: "Scans graph edges where `from_type` is ExternalPerson on MP-only edge types.",
        },
        "graph.orphan_external_person" => CheckDoc {
            what: "An ExternalPerson appears on an edge but has no corresponding ExternalPerson node.",
            measures: "Compares ExternalPerson ids on edges against ExternalPerson node ids.",
        },
        "utterance.unique_ids" => CheckDoc {
            what: "The same `utterance_id` appears more than once in `normalized/utterances.parquet`.",
            measures: "Counts rows per `utterance_id`; flags duplicates.",
        },
        "utterance.source_markers_vs_normalized" => CheckDoc {
            what: "Speaker-turn markers in cached meeting HTML do not align with extracted utterance row counts.",
            measures: "Runs S6 marker crosscheck per meeting: `count_source_markers` vs `extract_utterances_from_document` (allows bilingual dedup).",
        },
        "utterance.speech_char_coverage" => CheckDoc {
            what: "Persisted meeting text volume is far below the whole cached report — signals dropped content or parser regressions.",
            measures: "Per meeting: ratio of saved word count (all staging text: utterances, questions, votes, propositions, notices, commission chair) vs whole-document word count from cached HTML (all h1/h2/p/table blocks). Warns on kind p5 outlier or >15% drop vs committed `speech_coverage_baseline.parquet`.",
        },
        "utterance.roundtrip_discussion" => CheckDoc {
            what: "Normalized utterances exist for a question but its staging `discussion` JSON is empty.",
            measures: "Counts utterances per question id in normalized layer vs `discussion` field on staging question rows.",
        },
        "speaker.cleaned_re_resolves" => CheckDoc {
            what: "A name left in `unresolved_persons` would resolve to a Person after standard name cleaning.",
            measures: "Re-runs `resolve_detail` on cleaned names from speakers/questioners buckets in unresolved rows.",
        },
        "speaker.utterance_id_duplicates" => CheckDoc {
            what: "Duplicate or colliding utterance ids detected during speaker QA scan.",
            measures: "Checks normalized utterance rows for id collisions relevant to speaker resolution.",
        },
        "speaker.digit_prefix_resolvable" => CheckDoc {
            what: "A speaker raw name has a leading intervention index that, once stripped, resolves to a known actor.",
            measures: "Detects digit-prefix patterns in `raw_speaker` and tests actor resolution on the stripped name.",
        },
        "artifact.scraped_at_populated" => CheckDoc {
            what: "A source artifact row in the graph layer has an empty `scraped_at` timestamp.",
            measures: "Scans `graph/source_artifacts.parquet` for blank `scraped_at` values.",
        },
        "commission.meeting_gaps" => CheckDoc {
            what: "A commission meeting was discovered in the index but could not be fully scraped or parsed.",
            measures: "Surfaces rows from `sessions/56/commission/meeting_gaps.parquet` (reason + detail from crawl).",
        },
        "normalize.unresolved_persons_by_bucket" => CheckDoc {
            what: "Unresolved person names remain after identity resolution, grouped by source bucket and reason.",
            measures: "Rolls up `normalized/unresolved_persons.parquet` counts per (`source_bucket`, `reason`).",
        },
        "source.cache_exists" => CheckDoc {
            what: "A staging row references a `cache_path` that is not present on disk under `SCRAPER_CACHE_DIR`.",
            measures: "Joins `cache_path` from votes, questions, and source_artifacts against the local cache root.",
        },
        "agenda.entity_count_vs_parquet" => CheckDoc {
            what: "Cached meeting HTML has question/agenda headings but no matching question rows were written for that meeting.",
            measures: "Counts agenda question headings in cache HTML vs question rows per `meeting_id` in staging parquet.",
        },
        "agenda.hearing_not_extracted" => CheckDoc {
            what: "Commission meeting HTML contains a formal hearing heading but no matching rows in hearings.parquet.",
            measures: "Counts formal `hoorzitting met` / `audition de` h2 headings vs hearings.parquet rows per meeting.",
        },
        "agenda.interpellation_not_extracted" => CheckDoc {
            what: "Plenary meeting HTML contains an interpellation heading but no matching rows in interpellations.parquet.",
            measures: "Counts `Interpellatie van` / `Interpellation de` h2 headings vs interpellations.parquet rows per meeting.",
        },
        "question.grouped_internal_ids_complete" => CheckDoc {
            what: "A commission or plenary question row is missing `internal_ids` (site-native sub-question keys).",
            measures: "Requires non-empty `internal_ids` on staging question parquet rows.",
        },
        "written.duplicate_docname" => CheckDoc {
            what: "The same QRVA DOCNAME appears more than once in written questions staging.",
            measures: "Counts rows per `docname` in `sessions/56/written/questions.parquet`.",
        },
        "written.route_missing_question" => CheckDoc {
            what: "A QRVA route row references a written question id that does not exist.",
            measures: "Foreign-key check from `written/routes.parquet` to `written/questions.parquet`.",
        },
        "written.ambiguous_oral_reference" => CheckDoc {
            what: "A written question cites oral refs that do not resolve to exactly one oral Question.",
            measures: "Reads `normalized/oral_written_links.parquet` for `status=ambiguous`.",
        },
        "written.missing_department_role" => CheckDoc {
            what: "A QRVA route department code has no matching `ext:role:dept:{DEPTNUM}` ExternalPerson.",
            measures: "Compares route `deptnum` values against `identity/external_persons.parquet`.",
        },
        "dossier.ref_exists" => CheckDoc {
            what: "A vote references a `dossier_id` that is not present in `sessions/56/dossiers.parquet`.",
            measures: "Checks vote `dossier_id` foreign keys against the dossier id set.",
        },
        "meeting.chair_source_vs_parquet" => CheckDoc {
            what: "The chair name stored on a commission meeting row cannot be found in the cached report text.",
            measures: "Searches meeting HTML for the parquet `chair` value (or chair-presiding keywords).",
        },
        "meeting.date_source_vs_parquet" => CheckDoc {
            what: "The meeting date on a commission row does not appear verbatim in the cached HTML.",
            measures: "Substring search for parquet `date` in the meeting report cache file.",
        },
        "meeting.times_source_vs_parquet" => CheckDoc {
            what: "The meeting start time on a commission row does not appear in the cached HTML.",
            measures: "Searches cache HTML for `start_time` (colon/dot variants).",
        },
        "fk.questions_votes_to_meetings" => CheckDoc {
            what: "A question or vote row references a `meeting_id` that does not exist in meetings parquet.",
            measures: "Builds meeting id set from plenary + commission meetings; validates FK on questions and votes.",
        },
        "source.encoding_bytes" => CheckDoc {
            what: "Cached meeting HTML contains `0xFF` bytes, suggesting a legacy encoding or corrupt download.",
            measures: "Reads raw cache bytes for plenary meetings and flags files containing `0xFF`.",
        },
        "person.speaker_without_vote" => CheckDoc {
            what: "An MP spoke in a plenary meeting (resolved utterance) but has no CAST row in any plenary vote.",
            measures: "Compares plenary utterance `speaker_person_id` set against all `person_id` in `vote_casts.parquet`.",
        },
        "schema.table_loaded" => CheckDoc {
            what: "An expected staging parquet table is missing from `data/`.",
            measures: "Verifies each table in the Stage-0 schema inventory exists on disk.",
        },
        "schema.row_count_delta" => CheckDoc {
            what: "A staging table row count changed sharply since the last QA run.",
            measures: "Compares current row counts to `data/qa/row_counts.json`; warns when delta >25% and >10 rows.",
        },
        "schema.required_non_empty" => CheckDoc {
            what: "A required staging column is missing or contains empty values.",
            measures: "Checks required columns per table spec; counts blank string values per column.",
        },
        "schema.unique_keys" => CheckDoc {
            what: "Duplicate natural keys exist within a staging table.",
            measures: "Tracks composite keys from configured id columns (e.g. `vote_id`, `meeting_id`) per table batch.",
        },
        "schema.internal_ids_present" => CheckDoc {
            what: "Commission questions still use the legacy `dossier_ids` column instead of `internal_ids`.",
            measures: "Fails if `dossier_ids` column is present on commission questions parquet.",
        },
        "qa.summary_vs_detail" => CheckDoc {
            what: "Summary row counts in `checks.parquet` drifted from detail rows (meta-check S8).",
            measures: "After aggregation, verifies each check's summary `count` equals the number of detail rows for that `check_id`.",
        },
        _ => CheckDoc {
            what: "Undocumented check — add an entry in `check_catalog.rs`.",
            measures: "",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registered_check_ids;

    #[test]
    fn every_registered_check_has_documentation() {
        for id in registered_check_ids() {
            let doc = check_doc(id);
            assert!(
                !doc.what.starts_with("Undocumented"),
                "missing check_doc for {id}"
            );
        }
    }
}
