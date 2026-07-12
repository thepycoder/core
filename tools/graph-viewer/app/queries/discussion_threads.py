from __future__ import annotations

from typing import Any

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
    session_id = row[13] or ""
    meeting_kind = row[14] or ""
    meeting_id = row[15] or ""
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
    item_kind, item_id,
    session_id, meeting_kind, meeting_id
"""


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
