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
        "vote.compact_tables_vs_appendix_headers" => CheckDoc {
            what: "A vote number appears in the meeting-report appendix but not in the compact vote tables at the top of the page.",
            measures: "Re-parses cached vote HTML (`vote_inventory`) and compares appendix vs compact vote-number sets per meeting.",
        },
        "vote.source_inventory_vs_parquet" => CheckDoc {
            what: "Independent HTML inventory of formal vote-result occurrences does not match staged `vote_results` per source roll-call number.",
            measures: "Re-parses cached HTML for compact/reuse-aware result occurrences and compares ordered counts per `source_roll_call_number` to `vote_results.parquet` (reuse decisions excluded).",
        },
        "vote.appendix_bucket_counts" => CheckDoc {
            what: "An independently parsed appendix bucket count differs from the number of names retained in that ordered appendix occurrence.",
            measures: "Parses declared yes/no/abstain counts and voter-name paragraphs after each `(source_number, occurrence)` appendix header.",
        },
        "vote.decision_evidence" => CheckDoc {
            what: "A vote decision lacks valid title evidence or points to a result with no valid evidence.",
            measures: "Joins every `votes.parquet` row to valid Vote `decision_title` and VoteResult source spans.",
        },
        "vote.result_evidence_roles" => CheckDoc {
            what: "A formal result lacks evidence required by its method or retained candidate/proclamation data.",
            measures: "Requires method-specific valid source-span roles for standard, language-group, secret, no-quorum, sitting/standing, appendix, candidate, and proclamation data.",
        },
        "vote.unresolved_events" => CheckDoc {
            what: "The vote parser retained an event it could not safely assemble.",
            measures: "Surfaces every row in `vote_unresolved_events.parquet`, including the typed block bounds, reason, and source evidence.",
        },
        "vote.duplicate_person_across_buckets" => CheckDoc {
            what: "The same MP appears in more than one position bucket (yes/no/abstain) for a single vote.",
            measures: "Groups `normalized/vote_casts.parquet` by vote and person; flags persons with multiple positions.",
        },
        "vote.cast_count_vs_headline" => CheckDoc {
            what: "Resolved CAST edges per bucket do not add up to the headline yes/no/abstain totals.",
            measures: "Counts distinct `person_id` per position in `vote_casts.parquet` vs `vote_tallies.parquet` overall rows for named `roll_call` and `language_group_roll_call` results.",
        },
        "vote.cast_method_rules" => CheckDoc {
            what: "A vote method that cannot expose named choices has normalized casts.",
            measures: "Forbids CAST rows on secret, no-quorum, and sitting/standing results.",
        },
        "vote.standard_roll_call_invariants" => CheckDoc {
            what: "A standard roll call is missing an explicit overall yes, no, or abstain tally.",
            measures: "Requires all three overall position rows, retaining zero as a present value rather than treating it as missing.",
        },
        "vote.no_quorum_invariants" => CheckDoc {
            what: "A no-quorum result has forbidden yes/no/abstain tallies or casts, or lacks participation evidence.",
            measures: "For `status=no_quorum`: requires participation tallies; forbids position tallies and normalized casts.",
        },
        "vote.sitting_standing_invariants" => CheckDoc {
            what: "A sitting/standing result has counts/casts or is missing a formal outcome.",
            measures: "For `method=sitting_standing`: requires non-empty outcome; forbids yes/no/abstain tallies and casts.",
        },
        "vote.secret_ballot_invariants" => CheckDoc {
            what: "Secret ballot statistics or casts violate method semantics.",
            measures: "For `method=secret_ballot`: forbids named casts; when voter/valid/(blank) tallies exist, checks `voters = valid + blank`.",
        },
        "vote.language_group_sums" => CheckDoc {
            what: "Language-group roll call NL and FR counts do not sum to the overall total per option.",
            measures: "For `method=language_group_roll_call`: checks `nl_group + fr_group = overall` for yes/no/abstain tallies.",
        },
        "vote.number_sequence" => CheckDoc {
            what: "Vote numbers in a meeting report skip a value in the expected 1..N sequence.",
            measures: "Lists vote numbers from HTML inventory and reports gaps in the numeric sequence.",
        },
        "vote.total_plausibility" => CheckDoc {
            what: "Headline yes + no + abstain exceeds a plausible chamber size (>150).",
            measures: "Sums `vote_tallies.parquet` overall yes/no/abstain counts per `result_id`; warns when total > 150.",
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
        "graph.document_id_native" => CheckDoc {
            what: "A subdocument uses a local sequence number instead of its native FLWB document id.",
            measures: "Requires every `subdocuments.parquet.id` to match the native `NNKddddddd` FLWB identifier format.",
        },
        "graph.vote_result_edges_match_staging" => CheckDoc {
            what: "A graph `HAS_RESULT` edge does not match the result id recorded for that vote in staging.",
            measures: "Compares each `plenary/votes.parquet` (`vote_id`, `result_id`) pair with exactly one Vote→VoteResult `HAS_RESULT` graph edge.",
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
        "utterance.turn_number_sequence" => CheckDoc {
            what: "Speaker turn numbers within an agenda item skip a value in the expected 1..N sequence.",
            measures: "Scans cached HTML for `DD.MM` turn markers (ignoring optional intervention digits) and reports gaps per agenda item.",
        },
        "utterance.speech_char_coverage" => CheckDoc {
            what: "Persisted meeting text volume is far below the whole cached report — signals dropped content or parser regressions.",
            measures: "Per meeting: ratio of covered word count (union of extraction `source_spans` block `word_count` from `report_blocks`, with legacy saved-column fallback) vs whole-document word count from cached HTML. Warns on kind p5 outlier (≥10 meetings per kind). Constitutive whole-report classes (`corpus_policy.rs`) emit `info` with policy reference instead of `warn` and are excluded from the p5 pool. Mixed reports (e.g. plenary 24) remain fully evaluated.",
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
        "agenda.number_sequence" => CheckDoc {
            what: "Agenda item numbers in a meeting report skip a value in the expected 1..N sequence.",
            measures: "Builds the agenda timeline from cached HTML and reports missing agenda numbers between 1 and the highest seen.",
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
        "question.questioner_resolved" => CheckDoc {
            what: "A commission question has a nonempty staging questioner field but no resolved ASKED relation.",
            measures: "Joins commission question rows to normalized `asked.parquet` by canonical question id; warns separately when the staging questioner field is empty.",
        },
        "fk.utterance_interpellation" => CheckDoc {
            what: "An interpellation utterance does not resolve to exactly one canonical interpellation in its session, kind, and meeting.",
            measures: "Checks direct canonical item_id first, then uses site-native question_ids only to diagnose a unique noncanonical target or missing/ambiguous target.",
        },
        "written.published_answer_text_present" => CheckDoc {
            what: "A published QRVA answer has neither a Dutch nor French answer body.",
            measures: "Reads written answers staging and flags `source_kind=qrva`, written answers in publicated/published states where both language text fields are blank.",
        },
        "remuneration.amount_valid" => CheckDoc {
            what: "A remuneration row has a non-numeric, non-finite, negative, or reversed min/max EUR range.",
            measures: "Reads `remunerations.parquet` and requires finite nonnegative `remuneration_min <= remuneration_max`.",
        },
        "remuneration.amount_scale" => CheckDoc {
            what: "A remuneration maximum exceeds a conservative annual EUR threshold.",
            measures: "Warns when `remuneration_max` is greater than 1,000,000 EUR.",
        },
        "remuneration.duplicate_mandate" => CheckDoc {
            what: "The same person/year/mandate/institute appears on multiple remuneration rows.",
            measures: "Groups `remunerations.parquet` by person, year, mandate, and institute; emits one warning per group listing all amount ranges.",
        },
        "lobby.url_placement" => CheckDoc {
            what: "A lobby register URL or domain token appears outside the url column.",
            measures: "Reads `lobby.parquet` and flags `www.`/`http` tokens in contacts or interests, or excessive distinct URL tokens in url.",
        },
        "lobby.column_bleed" => CheckDoc {
            what: "A truncated URL fragment in contacts or interests matches a prefix of the canonical url field.",
            measures: "Detects partial domain tokens in non-url columns that align with the row's canonical url value.",
        },
        "commission.chair_subchair_overlap" => CheckDoc {
            what: "A person appears in both the chair and subchair lists for the same commission.",
            measures: "Reads `commissions.parquet`, splits/trims/case-folds `chairs` and `subchairs`, and fails once per overlapping person name.",
        },
        "dossier.date_chronology" => CheckDoc {
            what: "A dossier or subdocument source date is chronologically impossible (submission after vote/end, or future subdocument date).",
            measures: "Compares ISO dates on `dossiers.parquet` and `subdocuments.parquet`; targets canonical `Dossier:{session}/{id}` without correcting source values.",
        },
        "qa.warning_graph_target" => CheckDoc {
            what: "An entity-level warning points at a missing graph node, a source-local id, or a missing source artifact.",
            measures: "Validates nonempty `graph_node_type`/`graph_node_id` against `graph/nodes.parquet` and entity-warning `source_artifact_id` against `graph/source_artifacts.parquet`.",
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
        "source.span.block_range" => CheckDoc {
            what: "A provenance span references block indices outside the derived `report_blocks` stream or uses an invalid half-open range.",
            measures: "Validates `block_start < block_end` and `block_end` ≤ artifact block count in `derived/.../report_blocks.parquet`.",
        },
        "source.span.typed_schema" => CheckDoc {
            what: "A source-span bound, identity number, or confidence column lost its canonical Arrow type.",
            measures: "Requires UInt32 session/meeting/bounds and Float64 confidence columns.",
        },
        "source.span.validation_status" => CheckDoc {
            what: "A source span has an invalid status/reason combination.",
            measures: "Requires `valid` with no unresolved reason or `unresolved` with a non-empty reason.",
        },
        "source.span.artifact_id" => CheckDoc {
            what: "A source span references an `artifact_id` with no matching report blocks.",
            measures: "Joins `source_spans.artifact_id` to distinct `report_blocks.artifact_id`.",
        },
        "source.span.graph_artifact" => CheckDoc {
            what: "A source span's artifact is absent from graph provenance.",
            measures: "Joins span `artifact_id` to `graph/source_artifacts.parquet.source_artifact_id`.",
        },
        "source.span.source_content_stale" => CheckDoc {
            what: "A valid span was extracted from different source content than its report blocks or graph artifact.",
            measures: "Compares `source_content_hash` across spans, report blocks, and graph source artifacts.",
        },
        "source.span.block_parser_stale" => CheckDoc {
            what: "A valid span uses an outdated report-block parser version.",
            measures: "Compares span parser version to report blocks, graph artifacts, and the current parser constant.",
        },
        "source.span.extractor_version" => CheckDoc {
            what: "A source span lacks or uses an unexpected extractor version.",
            measures: "Validates extractor/version pairs for vote assembly and unified meeting parsing.",
        },
        "source.span.entity_reference" => CheckDoc {
            what: "A provenance span points at a Vote, VoteResult, or Meeting entity id missing from staging.",
            measures: "Checks `entity_type`/`entity_id` against `votes.parquet`, `vote_results.parquet`, and `plenary_{session}_{meeting}`.",
        },
        "source.span.extraction_fields" => CheckDoc {
            what: "An extraction span has empty `field_names`.",
            measures: "Requires non-empty `field_names` when `coverage_kind=extraction`.",
        },
        "source.span.allowed_role" => CheckDoc {
            what: "A span uses an unknown `span_role`.",
            measures: "Validates `span_role` against the catalogued vote/meeting provenance roles.",
        },
        "source.span.overlap" => CheckDoc {
            what: "Duplicate or same-entity same-role extraction spans overlap unexpectedly.",
            measures: "Allows scope and cross-entity overlap while rejecting duplicate span ids and overlapping extraction ranges for the same entity role.",
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
