from __future__ import annotations

import json
import re
from typing import Any

from app.config import Settings, get_settings
from app.models import EntityPreview, PreviewField, PreviewRelated
from app.queries.discussion_threads import fetch_question_thread


def fetch_entity_preview(
    conn,
    node_type: str,
    node_id: str,
    settings: Settings | None = None,
) -> EntityPreview | None:
    settings = settings or get_settings()
    handlers = {
        "Utterance": _preview_utterance,
        "Question": _preview_question,
        "Document": _preview_document,
        "Dossier": _preview_dossier,
        "Vote": _preview_vote,
        "Person": _preview_person,
        "ExternalPerson": _preview_external_person,
        "Meeting": _preview_meeting,
        "Topic": _preview_topic,
        "Party": _preview_party,
        "Commission": _preview_commission,
    }
    handler = handlers.get(node_type)
    if not handler:
        return None
    return handler(conn, node_id, settings)


def _pq(settings: Settings, *parts: str) -> str | None:
    path = settings.parquet_path(*parts)
    if not path.exists():
        return None
    return path.as_posix()


def _clip(text: str | None, limit: int = 4000) -> str:
    if not text:
        return ""
    text = text.strip()
    if len(text) <= limit:
        return text
    return text[:limit] + "…"


def _field(label: str, value: str, link: str = "") -> PreviewField:
    return PreviewField(label=label, value=value or "—", link=link or None)


def _related(node_type: str, node_id: str, label: str) -> PreviewRelated:
    return PreviewRelated(type=node_type, id=node_id, label=label or node_id)


def _preview_utterance(conn, node_id: str, settings: Settings) -> EntityPreview | None:
    row = conn.execute(
        """
        SELECT session_id, meeting_id, meeting_kind, seq, turn_number,
               raw_speaker, speaker_person_id, speaker_role,
               speaker_entity_type, speaker_entity_id, text, confidence, item_id
        FROM utterances
        WHERE utterance_id = ?
        LIMIT 1
        """,
        [node_id],
    ).fetchone()
    if not row:
        return None

    fields = [
        _field("Speaker", row[5]),
        _field("Role", row[7]),
        _field("Turn", row[4] or "—"),
        _field("Meeting", f"{row[2] or 'unknown'} {row[1]} (session {row[0]})"),
        _field("Sequence", row[3]),
        _field("Confidence", row[11]),
    ]
    if row[6]:
        fields.append(_field("Person id", row[6]))
    if row[9] and row[8]:
        fields.append(_field("Entity", f"{row[8]} → {row[9]}"))
    if row[12]:
        fields.append(_field("Item", row[12]))

    related: list[PreviewRelated] = []
    if row[12]:
        related.append(_related("Question", row[12], f"Question {row[12]}"))
    if row[9] and row[8]:
        related.append(_related(row[8], row[9], row[5] or row[9]))

    return EntityPreview(
        title=row[5] or node_id,
        fields=fields,
        content=_clip(row[10], 5000),
        content_label="Utterance text",
        related=related,
    )


def _preview_question(conn, node_id: str, settings: Settings) -> EntityPreview | None:
    row = _fetch_question_row(conn, node_id, settings)
    if not row:
        return None

    fields = [
        _field("Questioners", row["questioners"]),
        _field("Respondents", row["respondents"]),
        _field("Topic (NL)", row["topics_nl"]),
        _field("Topic (FR)", row["topics_fr"]),
        _field("Internal ids", row["internal_ids"]),
        _field("Meeting", row["meeting_id"]),
    ]

    content_parts: list[str] = []
    thread = fetch_question_thread(conn, node_id, limit=12)
    for block in thread:
        speaker = block.get("raw_speaker") or "?"
        turn = block.get("turn_number")
        prefix = f"{turn} {speaker}" if turn else speaker
        text = _clip(block.get("text", ""), 800)
        content_parts.append(f"{prefix}:\n{text}")

    related: list[PreviewRelated] = []
    for person_name in _split_names(row["questioners"]):
        related.append(_related("Unresolved", person_name, person_name))

    return EntityPreview(
        title=row["topics_nl"] or row["topics_fr"] or node_id,
        fields=fields,
        content="\n\n—\n\n".join(content_parts) if content_parts else None,
        content_label="Discussion excerpt",
        related=related[:6],
    )


def _fetch_question_row(conn, question_id: str, settings: Settings) -> dict[str, str] | None:
    for rel in ("sessions/56/plenary/questions.parquet", "sessions/56/commission/questions.parquet"):
        path = _pq(settings, rel)
        if not path:
            continue
        row = conn.execute(
            f"""
            SELECT question_id, meeting_id, questioners, respondents,
                   topics_nl, topics_fr, internal_ids
            FROM read_parquet('{path}')
            WHERE question_id = ?
            LIMIT 1
            """,
            [question_id],
        ).fetchone()
        if row:
            return {
                "question_id": row[0],
                "meeting_id": row[1],
                "questioners": row[2],
                "respondents": row[3],
                "topics_nl": row[4],
                "topics_fr": row[5],
                "internal_ids": row[6],
            }
    return None


def _preview_document(conn, node_id: str, settings: Settings) -> EntityPreview | None:
    path = _pq(settings, "sessions/56/subdocuments.parquet")
    if not path:
        return None

    dossier_nodes = conn.execute(
        """
        SELECT to_id
        FROM edges
        WHERE edge_type = 'SUBMITTED'
          AND from_type = 'Document'
          AND from_id = ?
        """,
        [node_id],
    ).fetchall()

    dossier_ids = [d[0].split("/")[-1] for d in dossier_nodes if d[0]]

    if dossier_ids:
        placeholders = ", ".join("?" * len(dossier_ids))
        rows = conn.execute(
            f"""
            SELECT dossier_id, type, authors, date, file_url
            FROM read_parquet('{path}')
            WHERE id = ? AND dossier_id IN ({placeholders})
            ORDER BY dossier_id
            """,
            [node_id, *dossier_ids],
        ).fetchall()
    else:
        rows = conn.execute(
            f"""
            SELECT dossier_id, type, authors, date, file_url
            FROM read_parquet('{path}')
            WHERE id = ?
            ORDER BY dossier_id
            LIMIT 20
            """,
            [node_id],
        ).fetchall()

    if not rows:
        return None

    if len(rows) > 1 and not dossier_ids:
        note = (
            f"Document id “{node_id}” appears in {len(rows)} dossiers "
            "(ids are only unique per dossier). Showing matches:"
        )
    elif len(rows) > 1:
        note = f"Matched {len(rows)} dossier contexts for this document id."
    else:
        note = None

    row = rows[0]
    dossier_node = f"56/{row[0]}"
    dossier_title = _dossier_title(conn, dossier_node, settings)

    fields = [
        _field("Type", row[1]),
        _field("Authors", row[2]),
        _field("Date", row[3]),
        _field("Dossier", f"{row[0]} — {dossier_title}" if dossier_title else row[0]),
    ]
    if row[4]:
        fields.append(_field("PDF", row[4], link=row[4]))

    related = [_related("Dossier", dossier_node, dossier_title or dossier_node)]

    content_lines = []
    if note:
        content_lines.append(note)
    if len(rows) > 1:
        for r in rows[:15]:
            title = _dossier_title(conn, f"56/{r[0]}", settings)
            line = f"• Dossier {r[0]} ({r[1]})"
            if title:
                line += f": {_clip(title, 120)}"
            if r[4]:
                line += f"\n  PDF: {r[4]}"
            content_lines.append(line)
        if len(rows) > 15:
            content_lines.append(f"… and {len(rows) - 15} more dossiers")

    return EntityPreview(
        title=f"{node_id} ({row[1]})",
        fields=fields,
        content="\n".join(content_lines) if content_lines else None,
        content_label="Document contexts" if len(rows) > 1 else None,
        related=related,
    )


def _dossier_title(conn, dossier_node: str, settings: Settings) -> str:
    path = _pq(settings, "sessions/56/dossiers.parquet")
    if not path:
        return ""
    session_id, dossier_id = dossier_node.split("/", 1)
    row = conn.execute(
        f"""
        SELECT title FROM read_parquet('{path}')
        WHERE session_id = ? AND id = ?
        LIMIT 1
        """,
        [session_id, dossier_id],
    ).fetchone()
    return row[0] if row else ""


def _preview_dossier(conn, node_id: str, settings: Settings) -> EntityPreview | None:
    path = _pq(settings, "sessions/56/dossiers.parquet")
    if not path:
        return None

    if "/" not in node_id:
        return None
    session_id, dossier_id = node_id.split("/", 1)

    row = conn.execute(
        f"""
        SELECT title, authors, document_type, status, submission_date, end_date,
               vote_date, eurovoc_main_descriptor, eurovoc_descriptors,
               latest_adopted_text_url, latest_report_url
        FROM read_parquet('{path}')
        WHERE session_id = ? AND id = ?
        LIMIT 1
        """,
        [session_id, dossier_id],
    ).fetchone()
    if not row:
        return None

    fields = [
        _field("Authors", row[1]),
        _field("Type", row[2]),
        _field("Status", row[3]),
        _field("Submitted", row[4]),
        _field("End date", row[5]),
        _field("Vote date", row[6]),
        _field("Eurovoc (main)", row[7]),
        _field("Eurovoc (all)", row[8]),
    ]
    if row[9]:
        fields.append(_field("Latest adopted text", row[9], link=row[9]))
    if row[10]:
        fields.append(_field("Latest report", row[10], link=row[10]))

    subdoc_path = _pq(settings, "sessions/56/subdocuments.parquet")
    content = None
    if subdoc_path:
        subdocs = conn.execute(
            f"""
            SELECT id, type, authors, date, file_url
            FROM read_parquet('{subdoc_path}')
            WHERE dossier_id = ?
            ORDER BY cast(id as integer)
            LIMIT 25
            """,
            [dossier_id],
        ).fetchall()
        if subdocs:
            lines = [f"Subdocuments ({len(subdocs)} shown):"]
            for sd in subdocs:
                line = f"• #{sd[0]} {sd[1]}"
                if sd[2]:
                    line += f" — {sd[2]}"
                if sd[3]:
                    line += f" ({sd[3]})"
                if sd[4]:
                    line += f"\n  {sd[4]}"
                lines.append(line)
            content = "\n".join(lines)

    return EntityPreview(
        title=_clip(row[0], 500),
        fields=fields,
        content=content,
        content_label="Subdocuments",
    )


def _preview_vote(conn, node_id: str, settings: Settings) -> EntityPreview | None:
    path = _pq(settings, "sessions/56/plenary/votes.parquet")
    if not path:
        return None

    row = conn.execute(
        f"""
        SELECT title_nl, title_fr, date, yes, no, abstain,
               dossier_id, document_id, motion_id, meeting_id
        FROM read_parquet('{path}')
        WHERE vote_id = ?
        LIMIT 1
        """,
        [node_id],
    ).fetchone()
    if not row:
        return None

    fields = [
        _field("Date", row[2]),
        _field("Result", f"Yes {row[3]} · No {row[4]} · Abstain {row[5]}"),
        _field("Meeting", row[9]),
        _field("Dossier ref", row[6]),
        _field("Document ref", row[7]),
        _field("Motion ref", row[8]),
    ]

    related: list[PreviewRelated] = []
    if row[6]:
        dossier_node = f"56/{row[6]}"
        related.append(_related("Dossier", dossier_node, f"Dossier {row[6]}"))
    if row[7]:
        related.append(_related("Document", row[7], f"Document {row[7]}"))

    return EntityPreview(
        title=row[0] or row[1] or node_id,
        fields=fields,
        content=row[1] if row[1] and row[1] != row[0] else None,
        content_label="Title (FR)" if row[1] and row[1] != row[0] else None,
        related=related,
    )


def _preview_person(conn, node_id: str, settings: Settings) -> EntityPreview | None:
    person_path = _pq(settings, "identity/persons.parquet")
    member_path = _pq(settings, "sessions/56/members.parquet")
    membership_path = _pq(settings, "identity/memberships.parquet")

    first, last = "", ""
    if person_path:
        row = conn.execute(
            f"""
            SELECT first_name, last_name
            FROM read_parquet('{person_path}')
            WHERE person_id = ?
            LIMIT 1
            """,
            [node_id],
        ).fetchone()
        if row:
            first, last = row[0], row[1]

    fields: list[PreviewField] = []
    if member_path:
        row = conn.execute(
            f"""
            SELECT fraction, constituency, email, active, language
            FROM read_parquet('{member_path}')
            WHERE member_id = ?
            LIMIT 1
            """,
            [node_id],
        ).fetchone()
        if row:
            fields.extend(
                [
                    _field("Party", row[0]),
                    _field("Constituency", row[1]),
                    _field("Email", row[2]),
                    _field("Active", row[3]),
                    _field("Language", row[4]),
                ]
            )

    if membership_path:
        memberships = conn.execute(
            f"""
            SELECT org_type, org_id
            FROM read_parquet('{membership_path}')
            WHERE person_id = ?
            ORDER BY org_type, org_id
            """,
            [node_id],
        ).fetchall()
        parties = [m[1] for m in memberships if m[0] == "party"]
        commissions = [m[1] for m in memberships if m[0] == "commission"]
        if parties:
            fields.append(_field("Party (identity)", ", ".join(parties)))
        if commissions:
            fields.append(_field("Commissions", ", ".join(commissions[:8])))

    alias_path = _pq(settings, "identity/person_aliases.parquet")
    if alias_path:
        aliases = conn.execute(
            f"""
            SELECT alias_norm FROM read_parquet('{alias_path}')
            WHERE person_id = ?
            LIMIT 10
            """,
            [node_id],
        ).fetchall()
        if aliases:
            fields.append(_field("Aliases", ", ".join(a[0] for a in aliases)))

    name = f"{first} {last}".strip() or node_id
    return EntityPreview(title=name, fields=fields)


def _preview_external_person(conn, node_id: str, settings: Settings) -> EntityPreview | None:
    ext_path = _pq(settings, "identity/external_persons.parquet")
    if not ext_path:
        return None

    row = conn.execute(
        f"""
        SELECT display_name, kind, source, first_seen_bucket, source_url
        FROM read_parquet('{ext_path}')
        WHERE external_person_id = ?
        LIMIT 1
        """,
        [node_id],
    ).fetchone()
    if not row:
        return None

    fields = [
        _field("Kind", row[1]),
        _field("Source", row[2]),
        _field("First seen in", row[3]),
    ]
    if row[4]:
        fields.append(_field("Source URL", row[4], link=row[4]))

    alias_path = _pq(settings, "identity/external_person_aliases.parquet")
    if alias_path:
        aliases = conn.execute(
            f"""
            SELECT alias_norm FROM read_parquet('{alias_path}')
            WHERE external_person_id = ?
            LIMIT 10
            """,
            [node_id],
        ).fetchall()
        if aliases:
            fields.append(_field("Aliases", ", ".join(a[0] for a in aliases)))

    bio_content = None
    bio_label = None
    try:
        bio_row = conn.execute(
            """
            SELECT bio_nl, bio_json, model, created_at
            FROM external_person_bios
            WHERE external_person_id = ?
            ORDER BY created_at DESC
            LIMIT 1
            """,
            [node_id],
        ).fetchone()
        if bio_row:
            bio_content = bio_row[0]
            bio_label = f"Bio ({bio_row[2]}, {bio_row[3]})"
            if bio_row[1]:
                try:
                    parsed = json.loads(bio_row[1])
                    if parsed.get("holder_name"):
                        fields.append(_field("Holder", parsed["holder_name"]))
                    if parsed.get("role"):
                        fields.append(_field("Role", parsed["role"]))
                    if parsed.get("affiliation"):
                        fields.append(_field("Affiliation", parsed["affiliation"]))
                    if parsed.get("confidence"):
                        fields.append(_field("Bio confidence", parsed["confidence"]))
                except json.JSONDecodeError:
                    pass
    except Exception:
        pass

    return EntityPreview(
        title=row[0] or node_id,
        fields=fields,
        content=bio_content,
        content_label=bio_label,
    )


def _preview_meeting(conn, node_id: str, settings: Settings) -> EntityPreview | None:
    match = re.match(r"^(plenary|commission)_(\d+)_(\d+)$", node_id)
    if not match:
        return None
    kind, _session, meeting_num = match.groups()
    rel = (
        "sessions/56/plenary/meetings.parquet"
        if kind == "plenary"
        else "sessions/56/commission/meetings.parquet"
    )
    path = _pq(settings, rel)
    if not path:
        return None

    if kind == "commission":
        row = conn.execute(
            f"""
            SELECT date, time_of_day, start_time, end_time, commission, chair
            FROM read_parquet('{path}')
            WHERE meeting_id = ?
            LIMIT 1
            """,
            [meeting_num],
        ).fetchone()
        if not row:
            return None
        fields = [
            _field("Kind", "Commission"),
            _field("Date", row[0]),
            _field("Time", f"{row[1]} ({row[2]}–{row[3]})"),
            _field("Commission", row[4]),
            _field("Chair", row[5]),
        ]
    else:
        row = conn.execute(
            f"""
            SELECT date, time_of_day, start_time, end_time
            FROM read_parquet('{path}')
            WHERE meeting_id = ?
            LIMIT 1
            """,
            [meeting_num],
        ).fetchone()
        if not row:
            return None
        fields = [
            _field("Kind", "Plenary"),
            _field("Date", row[0]),
            _field("Time", f"{row[1]} ({row[2]}–{row[3]})"),
        ]

    return EntityPreview(title=node_id, fields=fields)


def _preview_topic(conn, node_id: str, _settings: Settings) -> EntityPreview | None:
    return EntityPreview(
        title=node_id,
        fields=[_field("Eurovoc descriptor", node_id)],
    )


def _preview_party(conn, node_id: str, _settings: Settings) -> EntityPreview | None:
    return EntityPreview(
        title=node_id,
        fields=[_field("Party slug", node_id)],
    )


def _preview_commission(conn, node_id: str, settings: Settings) -> EntityPreview | None:
    path = _pq(settings, "identity/commissions.parquet")
    if not path:
        return EntityPreview(title=node_id, fields=[_field("Commission id", node_id)])

    row = conn.execute(
        f"""
        SELECT name, chairs, permanent_members
        FROM read_parquet('{path}')
        WHERE commission_id = ?
        LIMIT 1
        """,
        [node_id],
    ).fetchone()
    if not row:
        return EntityPreview(title=node_id, fields=[_field("Commission id", node_id)])

    fields = [
        _field("Chairs", row[1]),
        _field("Permanent members", _clip(row[2], 500)),
    ]
    return EntityPreview(title=row[0] or node_id, fields=fields)


def _split_names(csv: str) -> list[str]:
    if not csv:
        return []
    return [part.strip() for part in re.split(r"[,;]", csv) if part.strip()]
