from __future__ import annotations

import re
from collections import defaultdict
from typing import Callable

from qa_triage.models import Cluster, DetailRow

EDGE_ENTITY_RE = re.compile(
    r"^(?P<from_type>\w+):(?P<from_id>.+)->(?P<to_type>\w+):(?P<to_id>.+)$"
)

RECONCILIATION_CHECKS = {
    "vote.compact_total_vs_member_names",
    "vote.appendix_bucket_vs_collected_names",
}


def cluster_rows(rows: list[DetailRow]) -> list[Cluster]:
    by_check: dict[str, list[DetailRow]] = defaultdict(list)
    for row in rows:
        by_check[row.check_id].append(row)

    clusters: list[Cluster] = []
    handlers: dict[str, Callable[[list[DetailRow]], list[Cluster]]] = {
        "graph.edge_endpoints_exist": _cluster_edge_endpoints,
        "graph.voted_on_orphan_targets": _cluster_voted_on_orphans,
        "vote.source_inventory_vs_parquet": _cluster_vote_inventory,
        "utterance.speech_char_coverage": _cluster_speech_coverage,
        "agenda.entity_count_vs_parquet": _cluster_agenda_entity,
        "schema.unique_keys": _cluster_schema_unique,
    }

    reconciliation_vote_ids = {
        r.entity_id
        for check_id, check_rows in by_check.items()
        if check_id in RECONCILIATION_CHECKS
        for r in check_rows
    }

    for check_id, check_rows in sorted(by_check.items()):
        if check_id == "vote.appendix_bucket_vs_collected_names":
            continue
        if check_id == "vote.cast_count_vs_headline":
            clusters.extend(
                _cluster_cast_count(check_rows, reconciliation_vote_ids)
            )
        elif check_id == "vote.compact_total_vs_member_names":
            clusters.extend(_cluster_vote_reconciliation(check_rows))
        else:
            handler = handlers.get(check_id)
            if handler is None:
                clusters.extend(_cluster_fallback(check_id, check_rows))
            else:
                clusters.extend(handler(check_rows))

    appendix_rows = by_check.get("vote.appendix_bucket_vs_collected_names", [])
    for row in appendix_rows:
        vote_id = row.entity_id
        existing = next(
            (c for c in clusters if vote_id in {r.entity_id for r in c.rows}),
            None,
        )
        if existing:
            if row.check_id not in existing.check_ids:
                existing.check_ids.append(row.check_id)
            existing.rows.append(row)
            existing.row_count = len(existing.rows)
        else:
            clusters.extend(_cluster_vote_reconciliation([row]))

    _ensure_appendix_parent(clusters)
    clusters.sort(
        key=lambda c: (-_severity_rank(c.severity), -c.row_count, c.root_cause_id)
    )
    return clusters


def _severity_rank(severity: str) -> int:
    return {"fail": 4, "error": 4, "warn": 3, "info": 2}.get(severity, 0)


def _worst_severity(rows: list[DetailRow]) -> str:
    order = {"fail": 4, "error": 4, "warn": 3, "info": 2, "pass": 1}
    return max(rows, key=lambda r: order.get(r.severity, 0)).severity


def _cluster_edge_endpoints(rows: list[DetailRow]) -> list[Cluster]:
    groups: dict[tuple[str, str], list[DetailRow]] = defaultdict(list)
    for row in rows:
        m = EDGE_ENTITY_RE.match(row.entity_id)
        if not m:
            groups[("unknown", "unknown")].append(row)
            continue
        to_type = m.group("to_type")
        to_id = m.group("to_id")
        pattern = _question_id_pattern(to_type, to_id)
        groups[(to_type, pattern)].append(row)

    clusters: list[Cluster] = []
    for (to_type, pattern), group_rows in groups.items():
        if to_type == "Question" and pattern == "seq_off_by_one":
            root_id = "question_id_mismatch_utterance_part_of"
            title = "Utterance PART_OF targets question seq N but nodes use seq N-1"
        elif to_type == "Interpellation":
            root_id = "interpellation_id_mismatch_utterance_part_of"
            title = "Utterance PART_OF targets interpellation ids missing from graph nodes"
        else:
            root_id = f"edge_orphan_{to_type.lower()}_{pattern}"
            title = f"Graph edge missing {to_type} node ({pattern})"

        clusters.append(
            Cluster(
                root_cause_id=root_id,
                title=title,
                check_ids=["graph.edge_endpoints_exist"],
                severity=_worst_severity(group_rows),
                row_count=len(group_rows),
                cluster_key=f"{to_type}:{pattern}",
                rows=group_rows,
            )
        )
    return clusters


def _question_id_pattern(to_type: str, to_id: str) -> str:
    if to_type != "Question":
        return "generic"
    parts = to_id.rsplit("_", 1)
    if len(parts) == 2 and parts[1].isdigit() and int(parts[1]) >= 1:
        return "seq_off_by_one"
    return "generic"


def _cluster_voted_on_orphans(rows: list[DetailRow]) -> list[Cluster]:
    groups: dict[str, list[DetailRow]] = defaultdict(list)
    for row in rows:
        target = row.entity_id
        if re.match(r"^\d", target):
            groups["document_dossier_ref"].append(row)
        else:
            groups["other"].append(row)

    clusters: list[Cluster] = []
    if groups["document_dossier_ref"]:
        r = groups["document_dossier_ref"]
        clusters.append(
            Cluster(
                root_cause_id="voted_on_orphan_document_refs",
                title="VOTED_ON targets dossier-style Document ids not in graph",
                check_ids=["graph.voted_on_orphan_targets"],
                severity=_worst_severity(r),
                row_count=len(r),
                cluster_key="document_dossier_ref",
                rows=r,
            )
        )
    if groups["other"]:
        r = groups["other"]
        clusters.append(
            Cluster(
                root_cause_id="voted_on_orphan_other",
                title="VOTED_ON orphan targets (misc)",
                check_ids=["graph.voted_on_orphan_targets"],
                severity=_worst_severity(r),
                row_count=len(r),
                cluster_key="other",
                rows=r,
            )
        )
    return clusters


def _cluster_cast_count(
    rows: list[DetailRow],
    reconciliation_vote_ids: set[str],
) -> list[Cluster]:
    identity_rows: list[DetailRow] = []
    recon_rows: list[DetailRow] = []

    for row in rows:
        if row.entity_id in reconciliation_vote_ids:
            recon_rows.append(row)
        else:
            identity_rows.append(row)

    clusters: list[Cluster] = []
    if identity_rows:
        clusters.append(
            Cluster(
                root_cause_id="vote_identity_unresolved_names",
                title="CAST counts below headline due to unresolved vote appendix names",
                check_ids=["vote.cast_count_vs_headline"],
                severity=_worst_severity(identity_rows),
                row_count=len(identity_rows),
                cluster_key="identity_gap",
                rows=identity_rows,
                related_cluster_ids=["vote_appendix_parser_fragility"],
            )
        )

    by_vote: dict[str, list[DetailRow]] = defaultdict(list)
    for row in recon_rows:
        by_vote[row.entity_id].append(row)
    for vote_id, vote_rows in sorted(by_vote.items()):
        clusters.append(
            Cluster(
                root_cause_id=f"vote_cast_reconciliation_{vote_id.replace('/', '_')}",
                title=f"Vote {vote_id}: cast count and appendix reconciliation failures",
                check_ids=["vote.cast_count_vs_headline"],
                severity=_worst_severity(vote_rows),
                row_count=len(vote_rows),
                cluster_key=f"reconciliation:{vote_id}",
                rows=vote_rows,
                related_cluster_ids=["vote_appendix_parser_fragility"],
            )
        )
    return clusters


def _cluster_vote_reconciliation(rows: list[DetailRow]) -> list[Cluster]:
    by_vote: dict[str, list[DetailRow]] = defaultdict(list)
    for row in rows:
        by_vote[row.entity_id].append(row)

    clusters: list[Cluster] = []
    for vote_id, vote_rows in sorted(by_vote.items()):
        check_ids = sorted({r.check_id for r in vote_rows})
        clusters.append(
            Cluster(
                root_cause_id=f"vote_appendix_{vote_id.replace('/', '_')}",
                title=f"Vote {vote_id}: headline vs appendix name count mismatch",
                check_ids=check_ids,
                severity=_worst_severity(vote_rows),
                row_count=len(vote_rows),
                cluster_key=f"vote:{vote_id}",
                rows=vote_rows,
                related_cluster_ids=["vote_appendix_parser_fragility"],
            )
        )
    return clusters


def _cluster_vote_inventory(rows: list[DetailRow]) -> list[Cluster]:
    clusters: list[Cluster] = []
    for row in rows:
        meeting_id = row.meeting_id or row.entity_id.split("|")[0].strip()
        gap = row.actual or row.message
        clusters.append(
            Cluster(
                root_cause_id=f"vote_inventory_meeting_{meeting_id}",
                title=f"Plenary meeting {meeting_id}: source vote count vs parquet",
                check_ids=["vote.source_inventory_vs_parquet"],
                severity=row.severity,
                row_count=1,
                cluster_key=f"meeting:{meeting_id}:{gap}",
                rows=[row],
            )
        )
    return clusters


def _cluster_speech_coverage(rows: list[DetailRow]) -> list[Cluster]:
    clusters: list[Cluster] = []
    for row in rows:
        kind = row.meeting_kind or "unknown"
        mid = row.meeting_id or row.entity_id
        clusters.append(
            Cluster(
                root_cause_id=f"speech_coverage_{kind}_{mid}",
                title=f"{kind.title()} meeting {mid}: low speech text coverage",
                check_ids=["utterance.speech_char_coverage"],
                severity=row.severity,
                row_count=1,
                cluster_key=f"{kind}:{mid}",
                rows=[row],
            )
        )
    return clusters


def _cluster_agenda_entity(rows: list[DetailRow]) -> list[Cluster]:
    clusters: list[Cluster] = []
    for row in rows:
        kind = row.meeting_kind or "plenary"
        mid = row.meeting_id or "unknown"
        clusters.append(
            Cluster(
                root_cause_id=f"agenda_questions_missing_{kind}_{mid}",
                title=f"{kind.title()} meeting {mid}: agenda headings but no question rows",
                check_ids=["agenda.entity_count_vs_parquet"],
                severity=row.severity,
                row_count=1,
                cluster_key=f"{kind}:{mid}",
                rows=[row],
            )
        )
    return clusters


def _cluster_schema_unique(rows: list[DetailRow]) -> list[Cluster]:
    return [
        Cluster(
            root_cause_id="lobby_duplicate_keys",
            title="Lobby staging table has duplicate natural keys",
            check_ids=["schema.unique_keys"],
            severity=_worst_severity(rows),
            row_count=len(rows),
            cluster_key="lobby",
            rows=rows,
        )
    ]


def _cluster_fallback(check_id: str, rows: list[DetailRow]) -> list[Cluster]:
    slug = check_id.replace(".", "_")
    return [
        Cluster(
            root_cause_id=slug,
            title=f"QA cluster for {check_id}",
            check_ids=[check_id],
            severity=_worst_severity(rows),
            row_count=len(rows),
            cluster_key=check_id,
            rows=rows,
        )
    ]


def _ensure_appendix_parent(clusters: list[Cluster]) -> None:
    appendix_children = [
        c for c in clusters if c.root_cause_id.startswith("vote_appendix_56_")
    ]
    if len(appendix_children) < 2:
        return
    parent_id = "vote_appendix_parser_fragility"
    if any(c.root_cause_id == parent_id for c in clusters):
        return
    total = sum(c.row_count for c in appendix_children)
    clusters.append(
        Cluster(
            root_cause_id=parent_id,
            title="Vote appendix parser fragility (meetings 129, 133, 135)",
            check_ids=sorted({cid for c in appendix_children for cid in c.check_ids}),
            severity="warn",
            row_count=total,
            cluster_key="vote_appendix_family",
            rows=[],
            related_cluster_ids=sorted(c.root_cause_id for c in appendix_children),
        )
    )
