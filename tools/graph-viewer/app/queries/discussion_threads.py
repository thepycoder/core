from __future__ import annotations

import re
from typing import Any

_AGENDA_HEADING = re.compile(r"^(\d{2})\s{2,}(.+)$")
_NL_HEADING_MARKERS = (
    "vraag van ",
    "vragen van ",
    "samengevoegde vragen",
    "wijziging ",
    "mondelinge vragen",
)
_FR_HEADING_MARKERS = (
    "question de ",
    "questions jointes",
    "modification ",
    "questions orales",
)

PROCEEDING_NODE_TYPES: dict[str, str] = {
    "question": "Question",
    "hearing": "Hearing",
    "interpellation": "Interpellation",
}


def proceeding_node_type(item_kind: str | None) -> str | None:
    if not item_kind:
        return None
    return PROCEEDING_NODE_TYPES.get(item_kind)


def meeting_node_id(session_id: str, meeting_kind: str, meeting_id: str) -> str:
    return f"{meeting_kind}_{session_id}_{meeting_id}"


def _row_to_dict(row) -> dict[str, Any]:
    session_id = row[14] or ""
    meeting_kind = row[15] or ""
    meeting_id = row[16] or ""
    return {
        "utterance_id": row[0],
        "seq": row[1],
        "turn_number": row[2],
        "raw_speaker": row[3],
        "speaker_person_id": row[4],
        "speaker_role": row[5],
        "text": row[6],
        "confidence": row[7],
        "language": row[8],
        "speaker_entity_type": row[9] or "",
        "speaker_entity_id": row[10] or "",
        "item_kind": row[11] or "",
        "item_id": row[12] or "",
        "agenda_id": row[13] or "",
        "meeting_node_id": (
            meeting_node_id(session_id, meeting_kind, meeting_id)
            if session_id and meeting_kind and meeting_id
            else ""
        ),
    }


_THREAD_COLUMNS = """
    utterance_id, seq, turn_number, raw_speaker, speaker_person_id,
    speaker_role, text, confidence, language,
    speaker_entity_type, speaker_entity_id,
    item_kind, item_id, agenda_id,
    session_id, meeting_kind, meeting_id
"""


def _parse_agenda_heading(text: str) -> tuple[str, str] | None:
    stripped = text.strip()
    if stripped.startswith("- "):
        return None
    match = _AGENDA_HEADING.match(stripped)
    if not match:
        return None
    return match.group(1), match.group(2).strip()


def _heading_language_score(text: str) -> int:
    lower = text.lower()
    if any(marker in lower for marker in _NL_HEADING_MARKERS):
        return 2
    if any(marker in lower for marker in _FR_HEADING_MARKERS):
        return 1
    return 0


def _agenda_title_from_item_kind(item_kind: str | None) -> str:
    labels = {
        "question": "Question",
        "hearing": "Hearing",
        "interpellation": "Interpellation",
        "general_debate": "General debate",
        "proposition": "Proposition",
        "notice": "Notice",
        "vote": "Vote",
        "vote_explanation": "Vote explanation",
        "opening": "Opening",
        "closing": "Closing",
        "procedural": "Procedural",
        "unknown": "Other",
    }
    if not item_kind:
        return "Other"
    return labels.get(item_kind, item_kind.replace("_", " ").title())


def _agenda_sort_key(agenda_id: str) -> tuple[int, str]:
    if not agenda_id:
        return (-1, "")
    if agenda_id.isdigit():
        return (int(agenda_id), agenda_id)
    return (10_000, agenda_id)


def _fetch_agenda_titles_from_report_blocks(
    conn,
    meeting_kind: str,
    meeting_id: str,
    session_id: str,
) -> dict[str, str]:
    cache_pattern = f"%/{meeting_kind}/{session_id}-{meeting_id}.html"
    try:
        rows = conn.execute(
            """
            SELECT text
            FROM report_blocks
            WHERE cache_path LIKE ?
              AND block_type = 'h2'
            ORDER BY CAST(block_index AS INTEGER)
            """,
            [cache_pattern],
        ).fetchall()
    except Exception:
        return {}

    titles: dict[str, tuple[int, str]] = {}
    for (text,) in rows:
        parsed = _parse_agenda_heading(text or "")
        if not parsed:
            continue
        agenda_id, title = parsed
        score = _heading_language_score(title)
        current = titles.get(agenda_id)
        if current is None or score > current[0]:
            titles[agenda_id] = (score, title)
    return {agenda_id: title for agenda_id, (_, title) in titles.items()}


def fetch_meeting_agenda_items(
    conn,
    meeting_kind: str,
    meeting_id: str,
    *,
    session_id: str = "56",
) -> list[dict[str, Any]]:
    rows = conn.execute(
        """
        SELECT
            coalesce(nullif(agenda_id, ''), '') AS agenda_id,
            min(item_kind) AS item_kind,
            count(*) AS utterance_count
        FROM utterances
        WHERE meeting_kind = ?
          AND meeting_id = ?
        GROUP BY 1
        ORDER BY 1
        """,
        [meeting_kind, meeting_id],
    ).fetchall()

    titles = _fetch_agenda_titles_from_report_blocks(
        conn, meeting_kind, meeting_id, session_id
    )
    items: list[dict[str, Any]] = []
    for agenda_id, item_kind, utterance_count in rows:
        title = titles.get(agenda_id or "")
        if not title:
            if agenda_id:
                title = f"{_agenda_title_from_item_kind(item_kind)} {agenda_id}"
            else:
                title = "Unassigned"
        items.append(
            {
                "agenda_id": agenda_id or "",
                "title": title,
                "item_kind": item_kind or "",
                "utterance_count": int(utterance_count or 0),
            }
        )
    items.sort(key=lambda item: _agenda_sort_key(item["agenda_id"]))
    return items


def group_utterances_by_agenda(
    utterances: list[dict[str, Any]],
    agenda_items: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    by_agenda: dict[str, list[dict[str, Any]]] = {}
    for utterance in utterances:
        agenda_id = utterance.get("agenda_id") or ""
        by_agenda.setdefault(agenda_id, []).append(utterance)

    groups: list[dict[str, Any]] = []
    seen: set[str] = set()
    for item in agenda_items:
        agenda_id = item["agenda_id"]
        seen.add(agenda_id)
        groups.append(
            {
                "agenda_id": agenda_id,
                "title": item["title"],
                "item_kind": item.get("item_kind") or "",
                "utterances": by_agenda.get(agenda_id, []),
            }
        )

    for agenda_id, group_utterances in by_agenda.items():
        if agenda_id in seen:
            continue
        title = f"Agenda {agenda_id}" if agenda_id else "Unassigned"
        groups.append(
            {
                "agenda_id": agenda_id,
                "title": title,
                "item_kind": group_utterances[0].get("item_kind") or "",
                "utterances": group_utterances,
            }
        )
    groups.sort(key=lambda group: _agenda_sort_key(group["agenda_id"]))
    return groups


def fetch_question_thread(
    conn, question_id: str, *, limit: int = 500
) -> list[dict[str, Any]]:
    return fetch_proceeding_thread(conn, "question", question_id, limit=limit)


def fetch_proceeding_thread(
    conn,
    item_kind: str,
    item_id: str,
    *,
    limit: int = 500,
) -> list[dict[str, Any]]:
    rows = conn.execute(
        f"""
        SELECT {_THREAD_COLUMNS}
        FROM utterances
        WHERE item_kind = ? AND item_id = ?
        ORDER BY cast(seq AS integer), utterance_id
        LIMIT ?
        """,
        [item_kind, item_id, limit],
    ).fetchall()
    return [_row_to_dict(row) for row in rows]


def fetch_agenda_thread(
    conn,
    meeting_kind: str,
    meeting_id: str,
    agenda_id: str,
    *,
    limit: int = 500,
) -> list[dict[str, Any]]:
    rows = conn.execute(
        f"""
        SELECT {_THREAD_COLUMNS}
        FROM utterances
        WHERE meeting_kind = ?
          AND meeting_id = ?
          AND agenda_id = ?
        ORDER BY cast(seq AS integer), utterance_id
        LIMIT ?
        """,
        [meeting_kind, meeting_id, agenda_id, limit],
    ).fetchall()
    return [_row_to_dict(row) for row in rows]


def fetch_meeting_thread(
    conn, meeting_kind: str, meeting_id: str, *, limit: int = 2000
) -> list[dict[str, Any]]:
    rows = conn.execute(
        f"""
        SELECT {_THREAD_COLUMNS}
        FROM utterances
        WHERE meeting_kind = ?
          AND meeting_id = ?
        ORDER BY cast(seq AS integer), utterance_id
        LIMIT ?
        """,
        [meeting_kind, meeting_id, limit],
    ).fetchall()
    return [_row_to_dict(row) for row in rows]
