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
            "symbol": "extract_votes",
            "note": "dossier_id / motion_id parsed from vote titles.",
        },
    ],
    "vote.cast_count_vs_headline": [
        {
            "path": "scrapers/normalize/src/votes.rs",
            "symbol": "vote_casts normalization",
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
            "note": "Compares cast counts to headline totals.",
        },
    ],
    "vote.compact_total_vs_member_names": [
        {
            "path": "scrapers/plenary-meetings/src/main.rs",
            "symbol": "vote appendix parsing",
            "note": "Headline totals vs members_* CSV from HTML.",
        },
        {
            "path": "scrapers/normalize/src/vote_reconciliation.rs",
            "symbol": "reconciliation",
            "note": "Derived vote_reconciliation.parquet.",
        },
    ],
    "vote.appendix_bucket_vs_collected_names": [
        {
            "path": "scrapers/plenary-meetings/src/main.rs",
            "symbol": "extract_voter_names",
            "note": "Sibling-walk parser for appendix name paragraphs.",
        },
    ],
    "vote.source_inventory_vs_parquet": [
        {
            "path": "scrapers/crawl/src/vote_inventory.rs",
            "symbol": "parse_vote_inventory",
            "note": "Independent HTML vote number inventory.",
        },
        {
            "path": "scrapers/plenary-meetings/src/main.rs",
            "symbol": "extract_votes",
            "note": "Staging vote row writer.",
        },
    ],
    "utterance.speech_char_coverage": [
        {
            "path": "scrapers/crawl/src/agenda_timeline.rs",
            "symbol": "build_agenda_timeline",
            "note": "Section boundaries control what text is extracted.",
        },
        {
            "path": "scrapers/qa/src/speech.rs",
            "symbol": "check_speech_char_coverage",
            "note": "Coverage ratio check vs baseline.",
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
            "symbol": "extract_questions",
            "note": "Plenary question extraction.",
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
