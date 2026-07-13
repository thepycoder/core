from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

import duckdb
from bs4 import BeautifulSoup

from qa_triage.check_catalog import check_doc
from qa_triage.code_pointers import pointers_for_checks
from qa_triage.config import Settings
from qa_triage.models import Cluster, DetailRow

EDGE_ENTITY_RE = re.compile(
    r"^(?P<from_type>\w+):(?P<from_id>.+)->(?P<to_type>\w+):(?P<to_id>.+)$"
)


def build_evidence(
    cluster: Cluster,
    settings: Settings,
    conn: duckdb.DuckDBPyConnection,
) -> dict[str, Any]:
    check_docs = {
        cid: {"what": check_doc(cid).what, "measures": check_doc(cid).measures}
        for cid in cluster.check_ids
    }
    representative = _representative_rows(cluster.rows)
    parquet_samples = _parquet_samples(cluster, settings, conn, representative)
    html_excerpts = _html_excerpts(cluster, settings, representative)

    return {
        "root_cause_id": cluster.root_cause_id,
        "title": cluster.title,
        "check_ids": cluster.check_ids,
        "severity": cluster.severity,
        "row_count": cluster.row_count,
        "cluster_key": cluster.cluster_key,
        "related_cluster_ids": cluster.related_cluster_ids,
        "check_docs": check_docs,
        "representative_rows": [r.as_dict() for r in representative],
        "parquet_samples": parquet_samples,
        "html_excerpts": html_excerpts,
        "code_pointers": pointers_for_checks(cluster.check_ids),
        "stats": _cluster_stats(cluster),
    }


def _representative_rows(rows: list[DetailRow], limit: int = 5) -> list[DetailRow]:
    if not rows:
        return []
    if len(rows) <= limit:
        return rows

    seen_keys: set[str] = set()
    picked: list[DetailRow] = []
    for row in rows:
        key = row.entity_id or row.meeting_id or row.message[:80]
        if key in seen_keys:
            continue
        seen_keys.add(key)
        picked.append(row)
        if len(picked) >= limit:
            break
    return picked


def _cluster_stats(cluster: Cluster) -> dict[str, Any]:
    stats: dict[str, Any] = {"row_count": cluster.row_count}
    if cluster.check_ids == ["graph.edge_endpoints_exist"] and cluster.rows:
        to_types: dict[str, int] = {}
        for row in cluster.rows:
            m = EDGE_ENTITY_RE.match(row.entity_id)
            key = m.group("to_type") if m else "unknown"
            to_types[key] = to_types.get(key, 0) + 1
        stats["missing_to_type_counts"] = to_types
    return stats


def _parquet_samples(
    cluster: Cluster,
    settings: Settings,
    conn: duckdb.DuckDBPyConnection,
    representative: list[DetailRow],
) -> dict[str, Any]:
    samples: dict[str, Any] = {}
    primary = cluster.primary_check_id

    if primary == "graph.edge_endpoints_exist":
        samples.update(_sample_question_id_mismatch(settings, conn, representative))
    elif primary.startswith("vote."):
        samples.update(_sample_vote_cluster(settings, conn, representative))
    elif primary == "graph.voted_on_orphan_targets":
        samples.update(_sample_orphan_voted_on(settings, conn, representative))
    elif primary == "utterance.speech_char_coverage":
        samples.update(_sample_speech_coverage(settings, conn, representative))
    elif primary == "agenda.entity_count_vs_parquet":
        samples.update(_sample_agenda(settings, conn, representative))
    elif primary == "schema.unique_keys":
        samples.update(_sample_lobby_duplicates(settings, conn))

    unresolved_path = settings.parquet("normalized", "unresolved_persons.parquet")
    if unresolved_path.exists() and primary.startswith("vote."):
        samples["unresolved_votes_summary"] = _query_json(
            conn,
            f"""
            SELECT source_bucket, reason, count(*) AS n
            FROM read_parquet('{unresolved_path}')
            WHERE source_bucket = 'votes'
            GROUP BY 1, 2
            ORDER BY n DESC
            LIMIT 5
            """,
        )

    return samples


def _sample_question_id_mismatch(
    settings: Settings,
    conn: duckdb.DuckDBPyConnection,
    rows: list[DetailRow],
) -> dict[str, Any]:
    nodes_path = settings.parquet("graph", "nodes.parquet")
    out: dict[str, Any] = {}
    if not nodes_path.exists():
        return out

    target_ids: list[str] = []
    for row in rows:
        m = EDGE_ENTITY_RE.match(row.entity_id)
        if m and m.group("to_type") == "Question":
            target_ids.append(m.group("to_id"))

    if not target_ids:
        return out

    sample_target = target_ids[0]
    parts = sample_target.rsplit("_", 2)
    meeting_prefix = "_".join(parts[:2]) if len(parts) >= 3 else sample_target

    out["missing_question_target"] = sample_target
    out["graph_question_nodes_nearby"] = _query_json(
        conn,
        f"""
        SELECT node_id, label
        FROM read_parquet('{nodes_path}')
        WHERE node_type = 'Question' AND node_id LIKE '%{meeting_prefix}%'
        ORDER BY node_id
        LIMIT 10
        """,
    )

    utterances_path = settings.parquet("normalized", "utterances.parquet")
    if utterances_path.exists():
        out["normalized_utterance_item_ids"] = _query_json(
            conn,
            f"""
            SELECT utterance_id, item_kind, item_id, meeting_kind, meeting_id
            FROM read_parquet('{utterances_path}')
            WHERE item_id = '{sample_target}'
            LIMIT 5
            """,
        )

    for kind in ("plenary", "commission"):
        qpath = settings.parquet("sessions", "56", kind, "questions.parquet")
        if qpath.exists() and meeting_prefix.split("_")[-2:]:
            meeting_id = meeting_prefix.split("_")[-1]
            out[f"staging_{kind}_questions"] = _query_json(
                conn,
                f"""
                SELECT question_id, topics_nl
                FROM read_parquet('{qpath}')
                WHERE meeting_id = '{meeting_id}'
                ORDER BY question_id
                LIMIT 10
                """,
            )
    return out


def _sample_vote_cluster(
    settings: Settings,
    conn: duckdb.DuckDBPyConnection,
    rows: list[DetailRow],
) -> dict[str, Any]:
    vote_ids = [r.entity_id for r in rows if r.entity_id][:3]
    if not vote_ids:
        return {}
    vote_id = vote_ids[0]
    votes_path = settings.parquet("sessions", "56", "plenary", "votes.parquet")
    casts_path = settings.parquet("normalized", "vote_casts.parquet")
    recon_path = settings.parquet("normalized", "vote_reconciliation.parquet")
    out: dict[str, Any] = {"vote_id": vote_id}

    if votes_path.exists():
        out["staging_vote"] = _query_json(
            conn,
            f"""
            SELECT vote_id, meeting_id, yes, no, abstain,
                   length(members_yes) AS members_yes_len,
                   length(members_no) AS members_no_len,
                   title_nl, cache_path
            FROM read_parquet('{votes_path}')
            WHERE vote_id = '{vote_id}'
            """,
        )
    if casts_path.exists():
        out["cast_counts"] = _query_json(
            conn,
            f"""
            SELECT position, count(DISTINCT person_id) AS n
            FROM read_parquet('{casts_path}')
            WHERE vote_id = '{vote_id}'
            GROUP BY position
            """,
        )
    if recon_path.exists():
        out["reconciliation"] = _query_json(
            conn,
            f"""
            SELECT *
            FROM read_parquet('{recon_path}')
            WHERE vote_id = '{vote_id}'
            """,
        )
    return out


def _sample_orphan_voted_on(
    settings: Settings,
    conn: duckdb.DuckDBPyConnection,
    rows: list[DetailRow],
) -> dict[str, Any]:
    edges_path = settings.parquet("graph", "edges.parquet")
    if not edges_path.exists() or not rows:
        return {}
    target = rows[0].entity_id
    return {
        "orphan_target": target,
        "voted_on_edges": _query_json(
            conn,
            f"""
            SELECT edge_type, from_type, from_id, to_type, to_id
            FROM read_parquet('{edges_path}')
            WHERE edge_type = 'VOTED_ON' AND to_id = '{target}'
            LIMIT 5
            """,
        ),
    }


def _sample_speech_coverage(
    settings: Settings,
    conn: duckdb.DuckDBPyConnection,
    rows: list[DetailRow],
) -> dict[str, Any]:
    if not rows:
        return {}
    row = rows[0]
    kind = row.meeting_kind or "plenary"
    mid = row.meeting_id
    utter_path = settings.parquet("sessions", "56", kind, "utterances.parquet")
    out: dict[str, Any] = {
        "meeting_kind": kind,
        "meeting_id": mid,
        "expected": row.expected,
        "actual": row.actual,
    }
    if utter_path.exists():
        out["utterance_row_count"] = _query_json(
            conn,
            f"""
            SELECT count(*) AS n FROM read_parquet('{utter_path}')
            WHERE meeting_id = '{mid}'
            """,
        )
    return out


def _sample_agenda(
    settings: Settings,
    conn: duckdb.DuckDBPyConnection,
    rows: list[DetailRow],
) -> dict[str, Any]:
    if not rows:
        return {}
    row = rows[0]
    kind = row.meeting_kind or "plenary"
    mid = row.meeting_id
    qpath = settings.parquet("sessions", "56", kind, "questions.parquet")
    out = {"meeting_kind": kind, "meeting_id": mid, "message": row.message}
    if qpath.exists():
        out["question_rows"] = _query_json(
            conn,
            f"""
            SELECT count(*) AS n FROM read_parquet('{qpath}')
            WHERE meeting_id = '{mid}'
            """,
        )
    return out


def _sample_lobby_duplicates(
    settings: Settings,
    conn: duckdb.DuckDBPyConnection,
) -> dict[str, Any]:
    lobby_path = settings.parquet("sessions", "56", "lobby.parquet")
    if not lobby_path.exists():
        return {}
    return {
        "duplicate_keys": _query_json(
            conn,
            f"""
            SELECT name, count(*) AS n
            FROM read_parquet('{lobby_path}')
            GROUP BY name
            HAVING count(*) > 1
            ORDER BY n DESC
            LIMIT 10
            """,
        ),
    }


def _html_excerpts(
    cluster: Cluster,
    settings: Settings,
    representative: list[DetailRow],
) -> list[dict[str, str]]:
    excerpts: list[dict[str, str]] = []
    seen_paths: set[str] = set()

    for row in representative:
        if not row.cache_path or row.cache_path in seen_paths:
            continue
        seen_paths.add(row.cache_path)
        full = settings.cache_dir / row.cache_path
        if not full.exists():
            excerpts.append(
                {
                    "cache_path": row.cache_path,
                    "error": "cache file not found",
                }
            )
            continue
        text = extract_html_excerpt(full, row, cluster.primary_check_id)
        excerpts.append(
            {
                "cache_path": row.cache_path,
                "source_url": row.source_url,
                "excerpt": text[:4000],
            }
        )
        if len(excerpts) >= 2:
            break
    return excerpts


def extract_html_excerpt(
    path: Path,
    row: DetailRow,
    check_id: str,
) -> str:
    try:
        html = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        return f"(read error: {exc})"

    soup = BeautifulSoup(html, "html.parser")
    blocks: list[str] = []
    for tag in soup.find_all(["h1", "h2", "h3", "p", "table"]):
        text = " ".join(tag.get_text(" ", strip=True).split())
        if not text:
            continue
        blocks.append(f"<{tag.name}> {text[:500]}")

    needles: list[str] = []
    if check_id == "graph.edge_endpoints_exist":
        m = EDGE_ENTITY_RE.match(row.entity_id)
        if m:
            needles.append(m.group("to_id"))
    elif check_id.startswith("vote."):
        needles.extend(["Stemming", "Naamstemming", "Vote nominatif"])
        if row.entity_id:
            parts = row.entity_id.split("_")
            if len(parts) >= 2:
                needles.append(parts[-1])
    elif check_id == "agenda.entity_count_vs_parquet":
        needles.extend(["Vraag", "Question", "mondelinge"])
    elif check_id == "utterance.speech_char_coverage":
        needles.extend(["De voorzitter", "Vraag", "Question"])

    if needles:
        for i, block in enumerate(blocks):
            if any(
                str(n) in block
                for n in needles
                if isinstance(n, str) and len(str(n)) > 2
            ):
                start = max(0, i - 3)
                end = min(len(blocks), i + 8)
                return "\n".join(blocks[start:end])

    return "\n".join(blocks[:15])


def _query_json(conn: duckdb.DuckDBPyConnection, sql: str) -> list[dict[str, Any]]:
    try:
        result = conn.execute(sql)
        columns = [d[0] for d in result.description]
        return [
            dict(zip(columns, row, strict=True))
            for row in result.fetchall()
        ]
    except Exception as exc:
        return [{"error": str(exc)}]


def estimate_tokens(evidence: dict[str, Any]) -> int:
    serialized = json.dumps(evidence, ensure_ascii=False)
    return len(serialized) // 4
