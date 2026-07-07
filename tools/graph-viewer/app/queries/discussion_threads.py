from __future__ import annotations

from typing import Any


def _row_to_dict(row) -> dict[str, Any]:
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
    }


_THREAD_COLUMNS = """
    utterance_id, seq, turn_number, raw_speaker, speaker_person_id,
    speaker_role, text, confidence, language
"""


def fetch_question_thread(
    conn, question_id: str, *, limit: int = 500
) -> list[dict[str, Any]]:
    rows = conn.execute(
        f"""
        SELECT {_THREAD_COLUMNS}
        FROM utterances
        WHERE item_kind = 'question' AND item_id = ?
        ORDER BY cast(seq AS integer), utterance_id
        LIMIT ?
        """,
        [question_id, limit],
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
