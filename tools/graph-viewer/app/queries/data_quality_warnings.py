from __future__ import annotations

from app.models import DataQualityWarning
from app.queries.vote_helpers import resolve_result_id


def fetch_data_quality_warnings(
    conn, node_type: str, node_id: str
) -> list[DataQualityWarning]:
    """Return actionable entity warnings for a graph node.

    Vote nodes also inherit warnings attached to their linked VoteResult.
    Missing QA tables yield an empty list.
    """
    targets: list[tuple[str, str]] = [(node_type, node_id)]
    if node_type == "Vote":
        result_id = resolve_result_id(conn, node_type, node_id)
        if result_id:
            targets.append(("VoteResult", result_id))

    rows: list[DataQualityWarning] = []
    seen: set[str] = set()
    for graph_type, graph_id in targets:
        rows.extend(_query_warnings_for_target(conn, graph_type, graph_id, seen))
    return rows


def _query_warnings_for_target(
    conn, graph_type: str, graph_id: str, seen: set[str]
) -> list[DataQualityWarning]:
    try:
        result = conn.execute(
            """
            SELECT
                warning_id,
                warning_kind,
                check_id,
                severity,
                status,
                message,
                expected,
                actual,
                graph_node_type,
                graph_node_id,
                source_url,
                cache_path,
                source_block,
                source_artifact_id
            FROM qa_details
            WHERE graph_node_type = ?
              AND graph_node_id = ?
              AND status IN ('warn', 'fail', 'error')
              AND coalesce(warning_id, '') != ''
            ORDER BY severity DESC, check_id, warning_id
            """,
            [graph_type, graph_id],
        ).fetchall()
    except Exception:
        # Missing/unreadable QA view must not break node detail.
        return []

    out: list[DataQualityWarning] = []
    for row in result:
        warning_id = row[0] or ""
        graph_node_id = row[9] or ""
        if not warning_id or warning_id in seen:
            continue
        if looks_like_source_local_id(graph_node_id):
            continue
        seen.add(warning_id)
        out.append(
            DataQualityWarning(
                warning_id=warning_id,
                warning_kind=row[1] or "",
                check_id=row[2] or "",
                severity=row[3] or "",
                status=row[4] or "",
                message=row[5] or "",
                expected=row[6] or "",
                actual=row[7] or "",
                graph_node_type=row[8] or "",
                graph_node_id=graph_node_id,
                source_url=row[10] or "",
                cache_path=row[11] or "",
                source_block=row[12] or "",
                source_artifact_id=row[13] or "",
            )
        )
    return out


def looks_like_source_local_id(node_id: str) -> bool:
    return "#" in node_id
