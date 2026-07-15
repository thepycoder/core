"""Mirror of scrapers/qa/src/check_catalog.rs entries for warn/fail checks."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class CheckDoc:
    what: str
    measures: str


CHECK_DOCS: dict[str, CheckDoc] = {
    "vote.compact_total_vs_member_names": CheckDoc(
        what="Headline yes/no/abstain totals on a vote row do not match the number of named members in the appendix buckets.",
        measures="Reads `normalized/vote_reconciliation.parquet`; flags rows where `reconciled` is false.",
    ),
    "vote.appendix_bucket_counts": CheckDoc(
        what="An ordered appendix bucket's declared count differs from independently counted names.",
        measures="Parses each source-number occurrence and compares its declared bucket count to voter-name paragraphs.",
    ),
    "vote.source_inventory_vs_parquet": CheckDoc(
        what="The number of votes found in source HTML does not match the number of vote rows stored for that meeting.",
        measures="Counts vote numbers in cached HTML inventory vs rows in plenary votes.parquet per meeting_id.",
    ),
    "vote.compact_tables_vs_appendix_headers": CheckDoc(
        what="An appendix source number has no compact formal result table.",
        measures="Compares independent compact and appendix inventories.",
    ),
    "vote.duplicate_person_across_buckets": CheckDoc(
        what="One person appears in multiple result position buckets.",
        measures="Groups normalized casts by result_id and person_id.",
    ),
    "vote.number_sequence": CheckDoc(
        what="A meeting's source vote-number sequence has a gap.",
        measures="Checks the independent union of formal and appendix source numbers.",
    ),
    "vote.no_quorum_invariants": CheckDoc(
        what="A no-quorum result violates tally, cast, or participation rules.",
        measures="Requires participation and forbids all position rows, including explicit zero rows.",
    ),
    "vote.sitting_standing_invariants": CheckDoc(
        what="A sitting/standing result lacks an outcome or retains counts/casts.",
        measures="Requires a formal outcome and forbids position tallies and casts.",
    ),
    "vote.secret_ballot_invariants": CheckDoc(
        what="A secret ballot violates aggregate-statistic or cast rules.",
        measures="Requires voters/valid statistics, validates blank-inclusive totals, and forbids casts.",
    ),
    "vote.language_group_sums": CheckDoc(
        what="Language-group tallies are missing or do not sum to overall.",
        measures="Checks NL + FR = overall for every option, including zeros.",
    ),
    "vote.cast_count_vs_headline": CheckDoc(
        what="Resolved CAST edges per bucket do not add up to the headline yes/no/abstain totals.",
        measures="Counts distinct person_id per position in vote_casts.parquet vs headline fields on the staging vote row.",
    ),
    "vote.decision_evidence": CheckDoc(
        what="A decision lacks valid title or linked-result evidence.",
        measures="Joins vote and result ids to valid source spans.",
    ),
    "vote.result_evidence_roles": CheckDoc(
        what="A result lacks evidence required by its method and retained fields.",
        measures="Checks method-specific result, appendix, candidate, threshold, and proclamation roles.",
    ),
    "vote.unresolved_events": CheckDoc(
        what="The parser retained a formal vote event it could not safely assemble.",
        measures="Reads vote_unresolved_events.parquet with reasons and block evidence.",
    ),
    "vote.cast_method_rules": CheckDoc(
        what="A non-named vote method has CAST rows.",
        measures="Forbids casts for secret, no-quorum, and sitting/standing results.",
    ),
    "vote.standard_roll_call_invariants": CheckDoc(
        what="A standard roll call is missing explicit overall tally rows.",
        measures="Requires yes/no/abstain rows, including explicit zero values.",
    ),
    "source.span.typed_schema": CheckDoc(
        what="Source-span bounds or confidence use the wrong Arrow type.",
        measures="Requires UInt32 bounds/ids and Float64 confidence.",
    ),
    "source.span.block_range": CheckDoc(
        what="A valid source span has invalid half-open block bounds.",
        measures="Checks start < end <= artifact block count.",
    ),
    "source.span.validation_status": CheckDoc(
        what="A source span has an inconsistent validation status and reason.",
        measures="Valid spans have no reason; unresolved spans have a reason.",
    ),
    "source.span.artifact_id": CheckDoc(
        what="A source span references no report-block artifact.",
        measures="Joins artifact_id to report_blocks.",
    ),
    "source.span.graph_artifact": CheckDoc(
        what="A source span artifact is absent from graph provenance.",
        measures="Joins artifact_id to graph/source_artifacts.",
    ),
    "source.span.source_content_stale": CheckDoc(
        what="A valid span's source-content hash is stale.",
        measures="Compares hashes across spans, report blocks, and graph artifacts.",
    ),
    "source.span.block_parser_stale": CheckDoc(
        what="A valid span's block-parser version is stale.",
        measures="Compares parser versions across provenance layers and current code.",
    ),
    "source.span.extractor_version": CheckDoc(
        what="A source span has a missing or unexpected extractor version.",
        measures="Validates vote and meeting extractor/version pairs.",
    ),
    "source.span.entity_reference": CheckDoc(
        what="A source span references a missing typed entity.",
        measures="Checks every supported entity type against its staging table.",
    ),
    "source.span.extraction_fields": CheckDoc(
        what="A span has invalid extraction field names or scope fields.",
        measures="Validates coverage kind and the canonical field-name catalog.",
    ),
    "source.span.allowed_role": CheckDoc(
        what="A source span uses an unknown semantic role.",
        measures="Validates all vote and meeting source-span roles.",
    ),
    "source.span.overlap": CheckDoc(
        what="Same-entity same-role extraction spans overlap unexpectedly.",
        measures="Allows scope/cross-entity overlaps while rejecting duplicate or conflicting extraction spans.",
    ),
    "graph.edge_endpoints_exist": CheckDoc(
        what="A graph edge points to a from or to node id that does not exist in graph/nodes.parquet.",
        measures="Loads all edges and verifies both endpoints exist in the node key set.",
    ),
    "graph.voted_on_orphan_targets": CheckDoc(
        what="A VOTED_ON edge targets a vote node id that is missing from the graph.",
        measures="Finds VOTED_ON edges whose to_id is not present among graph Vote nodes.",
    ),
    "utterance.speech_char_coverage": CheckDoc(
        what="Persisted meeting text volume is far below the whole cached report.",
        measures="Per meeting: ratio of saved word count vs whole-document word count from cached HTML. Constitutive whole-report classes emit info with policy reference; mixed reports stay on warn.",
    ),
    "agenda.entity_count_vs_parquet": CheckDoc(
        what="Cached meeting HTML has question/agenda headings but no matching question rows were written.",
        measures="Counts agenda question headings in cache HTML vs question rows per meeting_id.",
    ),
    "written.published_answer_text_present": CheckDoc(
        what="A published QRVA answer has neither a Dutch nor French answer body.",
        measures="Reads written answers staging and flags QRVA written answers in publicated/published states where both language text fields are blank.",
    ),
    "remuneration.amount_valid": CheckDoc(
        what="A remuneration row has a non-numeric, non-finite, negative, or reversed min/max EUR range.",
        measures="Reads remunerations.parquet and requires finite nonnegative remuneration_min <= remuneration_max.",
    ),
    "remuneration.amount_scale": CheckDoc(
        what="A remuneration maximum exceeds a conservative annual EUR threshold.",
        measures="Warns when remuneration_max is greater than 1,000,000 EUR.",
    ),
    "remuneration.duplicate_mandate": CheckDoc(
        what="The same person/year/mandate/institute/period occurrence appears on multiple remuneration rows.",
        measures="Groups remunerations.parquet by person, year, mandate, institute, period_start, and period_end; emits one warning per exact duplicate group. Distinct Begin/Einde date segments are not duplicates.",
    ),
    "fk.utterance_interpellation": CheckDoc(
        what="An interpellation utterance does not resolve to exactly one canonical interpellation in its session, kind, and meeting.",
        measures="Checks direct canonical item_id first; missing or ambiguous site-native targets fail.",
    ),
    "utterance.interpellation_item_id_canonical": CheckDoc(
        what="An interpellation utterance site ref resolves uniquely, but the stored item_id is noncanonical.",
        measures="Warns when question_ids uniquely identify a canonical interpellation that differs from the utterance item_id.",
    ),
    "lobby.url_placement": CheckDoc(
        what="A lobby register URL or domain token appears outside the url column.",
        measures="Reads lobby.parquet and flags URL tokens in contacts or interests.",
    ),
    "lobby.column_bleed": CheckDoc(
        what="A truncated URL fragment in contacts or interests matches the canonical url field.",
        measures="Detects partial domain bleed from fixed-column PDF parsing.",
    ),
    "commission.chair_subchair_overlap": CheckDoc(
        what="A person appears in both the chair and subchair lists for the same commission.",
        measures="Reads commissions.parquet, splits/trims/case-folds chairs and subchairs, and fails once per overlapping person.",
    ),
    "dossier.date_chronology": CheckDoc(
        what="A dossier or subdocument source date is chronologically impossible.",
        measures="Compares ISO dates on dossiers/subdocuments; targets canonical Dossier nodes without correcting source values.",
    ),
    "qa.warning_graph_target": CheckDoc(
        what="An entity-level warning points at a missing graph node, source-local id, or missing source artifact.",
        measures="Validates nonempty graph_node targets and entity-warning source_artifact_id joins.",
    ),
    "meeting.gaps": CheckDoc(
        what="A plenary or commission meeting ID in the discovery range is an accepted remote gap rather than a parsed row.",
        measures="Surfaces rows from both meeting_gaps.parquet files; fails on disallowed reasons such as legacy parse_failed.",
    ),
    "source.manifest_complete": CheckDoc(
        what="A source inventory manifest is missing, has duplicate keys, or uses an unknown status.",
        measures="Validates data/source_manifests meeting manifests for status vocabulary and uniqueness.",
    ),
    "source.cache_metadata": CheckDoc(
        what="A parsed or unsupported-format manifest row has inconsistent cache existence, hash, or timestamp ordering.",
        measures="Joins manifest cache_path/content_hash/timestamps to on-disk files and optional .meta.json sidecars.",
    ),
    "source.freshness": CheckDoc(
        what="A mutable source manifest has not been checked within its documented refresh interval.",
        measures="Compares oldest checked_at in each source_manifests parquet against FRESHNESS_POLICIES max_age_days.",
    ),
    "normalize.provenance_columns": CheckDoc(
        what="A source-derived normalized table lacks the required transform-time provenance schema.",
        measures="Checks catalogued normalized parquet tables for the seven provenance columns.",
    ),
    "normalize.provenance_complete": CheckDoc(
        what="A source-derived normalized row is missing required transform-time provenance values.",
        measures="Requires nonempty artifact id, content hash, and extractor_version when URL/cache are set.",
    ),
    "normalize.provenance_artifact_id": CheckDoc(
        what="A normalized row source_artifact_id is not canonical or missing from graph source artifacts.",
        measures="Compares to crawl artifact_id and joins graph/source_artifacts when present.",
    ),
    "normalize.confidence_typed": CheckDoc(
        what="A normalized confidence column is not FLOAT64 in [0,1].",
        measures="Validates Arrow type and finite numeric range.",
    ),
    "schema.unique_keys": CheckDoc(
        what="Duplicate natural keys exist within a staging table.",
        measures="Tracks composite keys from configured id columns per table batch.",
    ),
}


def check_doc(check_id: str) -> CheckDoc:
    return CHECK_DOCS.get(
        check_id,
        CheckDoc(what=f"Undocumented check {check_id}", measures=""),
    )
