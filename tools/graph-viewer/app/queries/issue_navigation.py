from __future__ import annotations

import re
from typing import Any

_EDGE_RE = re.compile(r"^([^:]+):(.+)->([^:]+):(.+)$")
_MISSING_TO_RE = re.compile(r"missing to node ([^:]+):(.+)$")
_MISSING_FROM_RE = re.compile(r"missing from node ([^:]+):(.+)$")

NODE_TYPE_MAP = {
    "utterance": "Utterance",
    "vote": "Vote",
    "vote_result": "VoteResult",
    "meeting": "Meeting",
    "person": "Person",
    "question": "Question",
    "document": "Document",
    "dossier": "Dossier",
    "externalperson": "ExternalPerson",
    "commission": "Commission",
    "party": "Party",
    "hearing": "Hearing",
    "interpellation": "Interpellation",
}


def format_sample_label(data: dict[str, Any]) -> str:
    message = (data.get("message") or "").strip()
    if message:
        return message if len(message) <= 140 else f"{message[:137]}…"

    entity_type = (data.get("entity_type") or "").strip()
    entity_id = (data.get("entity_id") or "").strip()
    if entity_type and entity_id:
        return f"{entity_type} {entity_id}"

    meeting_kind = (data.get("meeting_kind") or "").strip()
    meeting_id = (data.get("meeting_id") or "").strip()
    if meeting_kind and meeting_id:
        return f"{meeting_kind} meeting {meeting_id}"

    expected = (data.get("expected") or "").strip()
    actual = (data.get("actual") or "").strip()
    if expected or actual:
        return f"expected {expected or '—'} · actual {actual or '—'}"

    return "QA issue sample"


def _normalize_node_type(entity_type: str) -> str | None:
    if not entity_type:
        return None
    key = entity_type.strip().lower()
    if key in NODE_TYPE_MAP:
        return NODE_TYPE_MAP[key]
    if entity_type[0].isupper():
        return entity_type
    return None


def _meeting_node_id(
    meeting_kind: str,
    meeting_id: str,
    session_id: str = "56",
) -> str | None:
    kind = meeting_kind.strip().lower()
    mid = meeting_id.strip()
    if kind in {"plenary", "commission"} and mid:
        return f"{kind}_{session_id}_{mid}"
    return None


def _parse_edge_endpoints(
    entity_id: str,
) -> tuple[tuple[str, str], tuple[str, str]] | None:
    match = _EDGE_RE.match(entity_id.strip())
    if not match:
        return None
    from_type, from_id, to_type, to_id = match.groups()
    return (from_type, from_id), (to_type, to_id)


def _node_target(node_type: str, node_id: str, label: str) -> dict[str, Any]:
    return {
        "action": "node",
        "label": label,
        "node_type": node_type,
        "node_id": node_id,
    }


def _context_target(label: str, data: dict[str, Any]) -> dict[str, Any]:
    return {
        "action": "context",
        "label": label,
        "source_url": data.get("source_url") or "",
        "cache_path": data.get("cache_path") or "",
    }


def _artifact_target(
    artifact_id: str, label: str, data: dict[str, Any]
) -> dict[str, Any]:
    return {
        "action": "artifact",
        "label": label,
        "artifact_id": artifact_id,
        "source_url": data.get("source_url") or "",
        "cache_path": data.get("cache_path") or "",
    }


def _report_target(
    meeting_id: str,
    source_block: str,
    session_id: str,
    meeting_kind: str,
    label: str,
    data: dict[str, Any],
) -> dict[str, Any]:
    return {
        "action": "report",
        "label": label,
        "meeting_id": meeting_id,
        "source_block": source_block,
        "session_id": session_id,
        "meeting_kind": meeting_kind,
        "source_url": data.get("source_url") or "",
        "cache_path": data.get("cache_path") or "",
    }


def _is_report_block_ref(source_block: str) -> bool:
    value = source_block.strip()
    return bool(value) and value.isdigit()


def _unresolved_bucket_target(
    bucket: str,
    reason: str,
    label: str,
) -> dict[str, Any]:
    return {
        "action": "unresolved_bucket",
        "label": label,
        "unresolved_bucket": bucket,
        "unresolved_reason": reason,
    }


def _prefer_existing_endpoint(
    conn,
    endpoints: tuple[tuple[str, str], tuple[str, str]],
    message: str,
    label: str,
) -> dict[str, Any] | None:
    (from_type, from_id), (to_type, to_id) = endpoints
    missing_to = _MISSING_TO_RE.search(message)
    missing_from = _MISSING_FROM_RE.search(message)

    preferred: list[tuple[str, str]] = []
    if missing_to:
        preferred = [(from_type, from_id), (to_type, to_id)]
    elif missing_from:
        preferred = [(to_type, to_id), (from_type, from_id)]
    else:
        preferred = [(from_type, from_id), (to_type, to_id)]

    if conn is not None:
        for node_type, node_id in preferred:
            exists = conn.execute(
                """
                SELECT 1 FROM nodes
                WHERE node_type = ? AND node_id = ?
                LIMIT 1
                """,
                [node_type, node_id],
            ).fetchone()
            if exists:
                return _node_target(node_type, node_id, label)

    if preferred:
        node_type, node_id = preferred[0]
        return _node_target(node_type, node_id, label)
    return None


def _vote_for_orphan_target(
    conn,
    to_type: str,
    to_id: str,
    label: str,
) -> dict[str, Any] | None:
    if conn is None:
        return None
    row = conn.execute(
        """
        SELECT from_id
        FROM edges
        WHERE edge_type = 'VOTED_ON'
          AND to_type = ?
          AND to_id = ?
        LIMIT 1
        """,
        [to_type, to_id],
    ).fetchone()
    if row:
        return _node_target("Vote", row[0], label)
    return None


def resolve_issue_navigation(
    conn,
    check_id: str,
    data: dict[str, Any],
) -> dict[str, Any]:
    label = format_sample_label(data)
    entity_type = (data.get("entity_type") or "").strip()
    entity_id = (data.get("entity_id") or "").strip()
    message = (data.get("message") or "").strip()
    meeting_kind = (data.get("meeting_kind") or "").strip()
    meeting_id = (data.get("meeting_id") or "").strip()
    session_id = (data.get("session_id") or "56").strip() or "56"
    source_block = (data.get("source_block") or "").strip()

    if (
        source_block
        and _is_report_block_ref(source_block)
        and meeting_id
        and meeting_kind.lower() == "plenary"
    ):
        return _report_target(
            meeting_id, source_block, session_id, meeting_kind, label, data
        )

    if check_id == "normalize.unresolved_persons_by_bucket" and entity_id:
        bucket, _, reason = entity_id.partition(":")
        return _unresolved_bucket_target(bucket, reason, label)

    if check_id == "artifact.scraped_at_populated" and entity_id:
        return _artifact_target(entity_id, label, data)

    if check_id == "graph.edge_endpoints_exist" and entity_type == "edge" and entity_id:
        endpoints = _parse_edge_endpoints(entity_id)
        if endpoints:
            target = _prefer_existing_endpoint(conn, endpoints, message, label)
            if target:
                return target

    if check_id == "graph.voted_on_orphan_targets":
        node_type = _normalize_node_type(entity_type)
        if node_type and entity_id:
            vote_target = _vote_for_orphan_target(conn, node_type, entity_id, label)
            if vote_target:
                return vote_target

    if entity_type == "edge" and entity_id:
        endpoints = _parse_edge_endpoints(entity_id)
        if endpoints:
            target = _prefer_existing_endpoint(conn, endpoints, message, label)
            if target:
                return target

    node_type = _normalize_node_type(entity_type)
    if node_type and entity_id:
        if node_type == "Meeting":
            mid = meeting_id or entity_id
            meeting_node = _meeting_node_id(meeting_kind, mid, session_id)
            if meeting_node:
                return _node_target("Meeting", meeting_node, label)
        if node_type == "Dossier" and "/" not in entity_id and entity_id.isdigit():
            entity_id = f"{session_id}/{entity_id}"
        return _node_target(node_type, entity_id, label)

    meeting_node = _meeting_node_id(meeting_kind, meeting_id, session_id)
    if meeting_node and check_id.split(".", 1)[0] in {
        "vote",
        "meeting",
        "agenda",
        "commission",
        "source",
        "utterance",
    }:
        return _node_target("Meeting", meeting_node, label)

    if meeting_node and meeting_kind and meeting_id:
        return _node_target("Meeting", meeting_node, label)

    return _context_target(label, data)
