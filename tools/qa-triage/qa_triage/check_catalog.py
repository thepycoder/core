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
    "vote.appendix_bucket_vs_collected_names": CheckDoc(
        what="The count of comma-separated names in `members_yes` disagrees with the headline `yes` total for that vote.",
        measures="Splits `members_yes` CSV on staging vote rows and compares length to parsed headline `yes`.",
    ),
    "vote.source_inventory_vs_parquet": CheckDoc(
        what="The number of votes found in source HTML does not match the number of vote rows stored for that meeting.",
        measures="Counts vote numbers in cached HTML inventory vs rows in plenary votes.parquet per meeting_id.",
    ),
    "vote.cast_count_vs_headline": CheckDoc(
        what="Resolved CAST edges per bucket do not add up to the headline yes/no/abstain totals.",
        measures="Counts distinct person_id per position in vote_casts.parquet vs headline fields on the staging vote row.",
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
        measures="Per meeting: ratio of saved word count vs whole-document word count from cached HTML.",
    ),
    "agenda.entity_count_vs_parquet": CheckDoc(
        what="Cached meeting HTML has question/agenda headings but no matching question rows were written.",
        measures="Counts agenda question headings in cache HTML vs question rows per meeting_id.",
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
