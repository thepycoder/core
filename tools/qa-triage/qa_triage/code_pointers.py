"""Static code pointers per check_id for fix agents."""

from __future__ import annotations

CODE_POINTERS: dict[str, list[dict[str, str]]] = {
    "graph.edge_endpoints_exist": [
        {
            "path": "scrapers/graph/src/build.rs",
            "symbol": "load_spoke_and_part_of_edges",
            "note": "Utterance PART_OF edges use item_id from normalized utterances without ensure_question_id.",
        },
        {
            "path": "scrapers/crawl/src/agenda_timeline.rs",
            "symbol": "push_question_item",
            "note": "Agenda timeline assigns item_id via composite_scoped_id for utterance linkage.",
        },
        {
            "path": "scrapers/crawl/src/utils.rs",
            "symbol": "composite_scoped_id, ensure_question_id",
            "note": "Question node ids are upgraded at graph build; utterance item_ids may lag.",
        },
    ],
    "graph.voted_on_orphan_targets": [
        {
            "path": "scrapers/graph/src/build.rs",
            "symbol": "VOTED_ON edge emission",
            "note": "Vote targets dossier/document ids that may not exist as graph nodes.",
        },
        {
            "path": "scrapers/plenary-meetings/src/main.rs",
            "symbol": "parse_plenary_meeting_report",
            "note": "dossier_id / motion_id parsed from vote titles.",
        },
    ],
    "vote.cast_count_vs_headline": [
        {
            "path": "scrapers/normalize/src/vote_casts.rs",
            "symbol": "normalize_vote_casts",
            "note": "Name lists resolved to person_id; unresolved names drop CAST edges.",
        },
        {
            "path": "scrapers/identity/src/normalize.rs",
            "symbol": "resolve_person / typo_corrections",
            "note": "Vote appendix names often need alias/typo handling.",
        },
        {
            "path": "scrapers/qa/src/vote_source.rs",
            "symbol": "run_vote_source_checks",
            "note": "Compares cast counts to headline totals for roll_call and language_group_roll_call.",
        },
    ],
    "vote.compact_total_vs_member_names": [
        {
            "path": "scrapers/crawl/src/vote_assembly.rs",
            "symbol": "assemble_votes_from_blocks",
            "note": "Headline totals vs appendix member buckets from HTML.",
        },
        {
            "path": "scrapers/normalize/src/vote_casts.rs",
            "symbol": "normalize_vote_casts",
            "note": "Derived vote_reconciliation.parquet.",
        },
    ],
    "vote.source_inventory_vs_parquet": [
        {
            "path": "scrapers/crawl/src/vote_inventory.rs",
            "symbol": "parse_vote_inventory",
            "note": "Independent HTML vote number inventory.",
        },
        {
            "path": "scrapers/qa/src/vote_source.rs",
            "symbol": "inventory_result_occurrences",
            "note": "Per-result occurrence crosscheck vs vote_results.parquet.",
        },
        {
            "path": "scrapers/crawl/src/vote_assembly.rs",
            "symbol": "assemble_votes_from_blocks",
            "note": "Staging vote result writer.",
        },
    ],
    "vote.appendix_bucket_counts": [
        {
            "path": "scrapers/crawl/src/vote_inventory.rs",
            "symbol": "parse_appendix_buckets",
            "note": "Independent ordered appendix count/name inventory.",
        },
        {
            "path": "scrapers/crawl/src/vote_assembly.rs",
            "symbol": "appendix occurrence matching",
            "note": "Production parser maps each result to the same ordered appendix occurrence.",
        },
    ],
    "vote.decision_evidence": [
        {
            "path": "scrapers/qa/src/vote_source.rs",
            "symbol": "check_vote_evidence",
            "note": "Joins Vote and VoteResult rows to valid source-span roles.",
        },
        {
            "path": "scrapers/crawl/src/vote_assembly.rs",
            "symbol": "push_title_spans",
            "note": "Emits per-decision title evidence.",
        },
    ],
    "vote.result_evidence_roles": [
        {
            "path": "scrapers/qa/src/vote_source.rs",
            "symbol": "check_vote_evidence",
            "note": "Defines method-aware required result evidence.",
        },
        {
            "path": "scrapers/crawl/src/vote_assembly.rs",
            "symbol": "assemble_votes_from_blocks",
            "note": "Emits result, appendix, candidate, threshold, and proclamation evidence.",
        },
    ],
    "vote.unresolved_events": [
        {
            "path": "scrapers/crawl/src/vote_assembly.rs",
            "symbol": "push_unresolved",
            "note": "Retains formal events that cannot be assembled safely.",
        },
        {
            "path": "scrapers/qa/src/vote_source.rs",
            "symbol": "check_unresolved_vote_events",
            "note": "Surfaces unresolved event rows without suppression.",
        },
    ],
    "vote.no_quorum_invariants": [
        {
            "path": "scrapers/crawl/src/vote_assembly.rs",
            "symbol": "quorum failure handling",
            "note": "Participation tallies without yes/no/abstain.",
        },
        {
            "path": "scrapers/qa/src/vote_source.rs",
            "symbol": "check_method_invariants",
            "note": "QA guardrails for no_quorum results.",
        },
    ],
    "vote.standard_roll_call_invariants": [
        {
            "path": "scrapers/crawl/src/vote_events.rs",
            "symbol": "parse_roll_call_table",
            "note": "Retains optional and explicit-zero standard tally values.",
        },
        {
            "path": "scrapers/qa/src/vote_source.rs",
            "symbol": "check_method_invariants",
            "note": "Requires all standard overall tally rows.",
        },
    ],
    "vote.secret_ballot_invariants": [
        {
            "path": "scrapers/crawl/src/vote_events.rs",
            "symbol": "parse_secret_ballot_table",
            "note": "Parses voters, valid, blank/invalid, and threshold statistics.",
        },
        {
            "path": "scrapers/qa/src/vote_source.rs",
            "symbol": "check_method_invariants",
            "note": "Checks secret aggregate equations and cast prohibition.",
        },
    ],
    "vote.sitting_standing_invariants": [
        {
            "path": "scrapers/crawl/src/vote_assembly.rs",
            "symbol": "sitting_standing_context",
            "note": "Assembles formal prose outcomes without tallies.",
        },
        {
            "path": "scrapers/qa/src/vote_source.rs",
            "symbol": "check_method_invariants",
            "note": "Requires outcome and forbids tallies/casts.",
        },
    ],
    "vote.cast_method_rules": [
        {
            "path": "scrapers/normalize/src/vote_casts.rs",
            "symbol": "normalize_vote_casts",
            "note": "Only named roll-call methods may emit casts.",
        },
    ],
    "vote.language_group_sums": [
        {
            "path": "scrapers/crawl/src/vote_events.rs",
            "symbol": "parse_roll_call_table",
            "note": "Language-group N/Tot/F table parsing.",
        },
        {
            "path": "scrapers/qa/src/vote_source.rs",
            "symbol": "check_method_invariants",
            "note": "NL+FR sum validation per option.",
        },
    ],
    "source.span.block_range": [
        {
            "path": "scrapers/qa/src/source_spans.rs",
            "symbol": "run_source_span_checks",
            "note": "Half-open range validation against report_blocks.",
        },
        {
            "path": "scrapers/crawl/src/meeting_parse.rs",
            "symbol": "assembly_spans",
            "note": "Span emission from vote assembly evidence.",
        },
    ],
    "source.span.entity_reference": [
        {
            "path": "scrapers/qa/src/source_spans.rs",
            "symbol": "run_source_span_checks",
            "note": "Vote/VoteResult/Meeting FK validation.",
        },
    ],
    "source.span.typed_schema": [
        {
            "path": "scrapers/crawl/src/source_spans.rs",
            "symbol": "write_source_spans_parquet",
            "note": "Canonical typed source-span schema.",
        },
        {
            "path": "scrapers/qa/src/source_spans.rs",
            "symbol": "run_source_span_checks",
            "note": "Validates typed bounds and confidence.",
        },
    ],
    "source.span.validation_status": [
        {
            "path": "scrapers/crawl/src/source_spans.rs",
            "symbol": "validate_source_spans",
            "note": "Assigns valid/unresolved status and reasons.",
        },
    ],
    "source.span.graph_artifact": [
        {
            "path": "scrapers/graph/src/build.rs",
            "symbol": "ArtifactRow",
            "note": "Materializes graph source-artifact provenance.",
        },
        {
            "path": "scrapers/qa/src/source_spans.rs",
            "symbol": "load_graph_artifacts",
            "note": "Checks every span artifact against graph artifacts.",
        },
    ],
    "source.span.source_content_stale": [
        {
            "path": "scrapers/qa/src/source_spans.rs",
            "symbol": "run_source_span_checks",
            "note": "Compares source hashes across all provenance layers.",
        },
    ],
    "source.span.block_parser_stale": [
        {
            "path": "scrapers/crawl/src/artifact_id.rs",
            "symbol": "BLOCK_PARSER_VERSION",
            "note": "Current canonical parser version.",
        },
    ],
    "source.span.extractor_version": [
        {
            "path": "scrapers/crawl/src/artifact_id.rs",
            "symbol": "VOTE_EXTRACTOR_VERSION, MEETING_SCOPE_EXTRACTOR_VERSION",
            "note": "Current canonical extractor versions.",
        },
    ],
    "source.span.extraction_fields": [
        {
            "path": "scrapers/qa/src/source_spans.rs",
            "symbol": "field_allowed_for_entity",
            "note": "Canonical entity-specific field-name rules.",
        },
    ],
    "source.span.allowed_role": [
        {
            "path": "scrapers/qa/src/source_spans.rs",
            "symbol": "ALLOWED_ROLES",
            "note": "Canonical semantic role catalog.",
        },
    ],
    "source.span.overlap": [
        {
            "path": "scrapers/qa/src/source_spans.rs",
            "symbol": "check_overlaps",
            "note": "Allows scope overlap but rejects conflicting extraction spans.",
        },
    ],
    "utterance.speech_char_coverage": [
        {
            "path": "scrapers/crawl/src/corpus_policy.rs",
            "symbol": "classify_meeting",
            "note": "Explicit corpus classes; constitutive meetings downgrade coverage to info.",
        },
        {
            "path": "scrapers/crawl/src/agenda_timeline.rs",
            "symbol": "build_agenda_timeline",
            "note": "Section boundaries control what text is extracted.",
        },
        {
            "path": "scrapers/qa/src/speech.rs",
            "symbol": "load_extraction_span_words",
            "note": "Union extraction span block word counts; legacy fallback when spans missing.",
        },
    ],
    "agenda.entity_count_vs_parquet": [
        {
            "path": "scrapers/qa/src/agenda_checks.rs",
            "symbol": "run_agenda_checks",
            "note": "Heading count vs parquet row count.",
        },
        {
            "path": "scrapers/plenary-meetings/src/main.rs",
            "symbol": "parse_plenary_meeting_report",
            "note": "Plenary question extraction.",
        },
    ],
    "written.published_answer_text_present": [
        {
            "path": "scrapers/qrva/src/xml.rs",
            "symbol": "parse_qrva_xml",
            "note": "Accumulates answer text across nested inline XML elements.",
        },
        {
            "path": "scrapers/qa/src/written.rs",
            "symbol": "check_published_answer_text",
            "note": "Flags published QRVA staging answers with blank NL and FR bodies.",
        },
    ],
    "remuneration.amount_valid": [
        {
            "path": "scrapers/remunerations/src/parse.rs",
            "symbol": "parse_remuneration_text",
            "note": "Parses European-locale EUR amounts into canonical decimal strings.",
        },
        {
            "path": "scrapers/qa/src/remunerations.rs",
            "symbol": "check_amount_valid",
            "note": "Requires finite nonnegative remuneration_min <= remuneration_max.",
        },
    ],
    "remuneration.amount_scale": [
        {
            "path": "scrapers/qa/src/remunerations.rs",
            "symbol": "check_amount_scale",
            "note": "Warns when remuneration_max exceeds 1,000,000 EUR.",
        },
    ],
    "remuneration.duplicate_mandate": [
        {
            "path": "scrapers/qa/src/remunerations.rs",
            "symbol": "duplicate_mandate_detail",
            "note": "Groups duplicate person/year/mandate/institute rows.",
        },
    ],
    "fk.utterance_interpellation": [
        {
            "path": "scrapers/normalize/src/utterances.rs",
            "symbol": "load_interpellation_targets",
            "note": "Canonicalizes uniquely site-reference-resolvable interpellation utterance IDs before normalized output.",
        },
        {
            "path": "scrapers/qa/src/agenda_checks.rs",
            "symbol": "check_utterance_interpellation_fk",
            "note": "Groups missing, ambiguous, and uniquely noncanonical interpellation targets by bad reference.",
        },
        {
            "path": "scrapers/graph/src/build.rs",
            "symbol": "resolve_proceeding_target_id",
            "note": "Does not use same-meeting single-candidate guesses for Interpellation nodes.",
        },
    ],
    "lobby.url_placement": [
        {
            "path": "scrapers/lobby/src/lib.rs",
            "symbol": "extract_lobby_from_layout",
            "note": "Character-position column slicing for pdftotext -layout rows.",
        },
        {
            "path": "scrapers/qa/src/lobby.rs",
            "symbol": "check_url_placement",
            "note": "Flags URL tokens outside the url column.",
        },
    ],
    "lobby.column_bleed": [
        {
            "path": "scrapers/lobby/src/lib.rs",
            "symbol": "slice_columns",
            "note": "Column boundaries for organisation/contact/interest/url fields.",
        },
        {
            "path": "scrapers/qa/src/lobby.rs",
            "symbol": "check_column_bleed",
            "note": "Detects truncated URL fragments in wrong columns.",
        },
    ],
    "commission.chair_subchair_overlap": [
        {
            "path": "scrapers/commissions/src/lib.rs",
            "symbol": "normalize_role_label",
            "note": "Canonical role tokens so Ondervoorzitters does not match Voorzitter.",
        },
        {
            "path": "scrapers/qa/src/commissions.rs",
            "symbol": "check_chair_subchair_overlap",
            "note": "Fails when the same person appears in both chairs and subchairs.",
        },
    ],
    "dossier.date_chronology": [
        {
            "path": "scrapers/qa/src/dossiers.rs",
            "symbol": "run_dossier_chronology_checks",
            "note": "Submission-after-vote/end and future subdocument dates.",
        },
    ],
    "qa.warning_graph_target": [
        {
            "path": "scrapers/qa/src/warnings.rs",
            "symbol": "run_warning_target_checks",
            "note": "Validates graph_node and source_artifact targets on entity warnings.",
        },
        {
            "path": "scrapers/qa/src/types.rs",
            "symbol": "CheckDetail::finalize",
            "note": "Deterministic warning_id and crawl::artifact_id provenance.",
        },
    ],
    "meeting.gaps": [
        {
            "path": "scrapers/crawl/src/meeting_gaps.rs",
            "symbol": "reconcile_meeting_coverage / discover_last_from_probes",
            "note": "Shared gap schema; trailing discovery 404s are not gaps; parse_failed is not publishable.",
        },
        {
            "path": "scrapers/qa/src/infrastructure.rs",
            "symbol": "check_meeting_gaps",
            "note": "Surfaces plenary and commission gap rows; fails disallowed reasons.",
        },
    ],
    "source.manifest_complete": [
        {
            "path": "scrapers/crawl/src/source_manifest.rs",
            "symbol": "validate_manifest_rows / write_source_manifest",
            "note": "Closed status vocabulary and unique native item keys.",
        },
        {
            "path": "scrapers/qa/src/infrastructure.rs",
            "symbol": "check_source_manifest_complete",
            "note": "QA over data/source_manifests/*.parquet.",
        },
    ],
    "source.cache_metadata": [
        {
            "path": "scrapers/crawl/src/cache_meta.rs",
            "symbol": "write_cache_artifact / CacheMetadata",
            "note": "Sibling .meta.json with hash and fetched/checked timestamps.",
        },
        {
            "path": "scrapers/qa/src/infrastructure.rs",
            "symbol": "check_source_cache_metadata",
            "note": "Joins manifest rows to cache files and sidecars.",
        },
    ],
    "source.freshness": [
        {
            "path": "scrapers/crawl/src/freshness.rs",
            "symbol": "FRESHNESS_POLICIES",
            "note": "Documented max_age_days per mutable source.",
        },
        {
            "path": "scrapers/qa/src/infrastructure.rs",
            "symbol": "check_source_freshness",
            "note": "Warns when oldest checked_at exceeds policy.",
        },
    ],
    "schema.unique_keys": [
        {
            "path": "scrapers/lobby/src/main.rs",
            "symbol": "lobby scraper",
            "note": "Lobby org natural key / slug generation.",
        },
    ],
}


def pointers_for_checks(check_ids: list[str]) -> list[dict[str, str]]:
    seen: set[tuple[str, str]] = set()
    out: list[dict[str, str]] = []
    for check_id in check_ids:
        for entry in CODE_POINTERS.get(check_id, []):
            key = (entry["path"], entry.get("symbol", ""))
            if key in seen:
                continue
            seen.add(key)
            out.append(entry)
    return out
