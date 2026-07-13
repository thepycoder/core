from __future__ import annotations

from typing import Any

import duckdb

from app.config import Settings, get_settings
from app.models import BrowseCategory, BrowseItem, BrowseResponse


def _pq(settings: Settings, *parts: str) -> str | None:
    path = settings.parquet_path(*parts)
    if not path.exists():
        return None
    return path.as_posix()


def _clip(text: str | None, limit: int = 120) -> str:
    if not text:
        return ""
    text = text.strip()
    if len(text) <= limit:
        return text
    return text[: limit - 1] + "…"


def fetch_browse_categories(
    conn: duckdb.DuckDBPyConnection,
    settings: Settings | None = None,
) -> list[BrowseCategory]:
    settings = settings or get_settings()
    specs = [
        ("dossiers", "Dossiers", "Legislative files and their documents", "Dossier", _count_dossiers),
        (
            "plenary-meetings",
            "Plenary meetings",
            "Chamber plenary sittings",
            "Meeting",
            _count_plenary_meetings,
        ),
        (
            "commission-meetings",
            "Commission meetings",
            "Committee sittings",
            "Meeting",
            _count_commission_meetings,
        ),
        (
            "oral-questions",
            "Oral questions",
            "Questions asked in plenary or commission",
            "Question",
            _count_oral_questions,
        ),
        (
            "written-questions",
            "Written questions",
            "Parliamentary written Q&A",
            "Question",
            _count_written_questions,
        ),
        ("votes", "Votes", "Roll-call and show-of-hands decisions", "Vote", _count_votes),
        ("persons", "MPs", "Chamber members", "Person", _count_persons),
        ("parties", "Parties", "Political groups", "Party", _count_parties),
        ("documents", "Documents", "Dossier subdocuments", "Document", _count_documents),
    ]
    categories: list[BrowseCategory] = []
    for cat_id, label, description, node_type, counter in specs:
        count = counter(conn, settings)
        if count > 0:
            categories.append(
                BrowseCategory(
                    id=cat_id,
                    label=label,
                    description=description,
                    node_type=node_type,
                    count=count,
                )
            )
    return categories


def fetch_browse(
    conn: duckdb.DuckDBPyConnection,
    category: str,
    limit: int = 40,
    offset: int = 0,
    q: str | None = None,
    settings: Settings | None = None,
) -> BrowseResponse:
    settings = settings or get_settings()
    handlers: dict[str, Any] = {
        "dossiers": _browse_dossiers,
        "plenary-meetings": _browse_plenary_meetings,
        "commission-meetings": _browse_commission_meetings,
        "oral-questions": _browse_oral_questions,
        "written-questions": _browse_written_questions,
        "votes": _browse_votes,
        "persons": _browse_persons,
        "parties": _browse_parties,
        "documents": _browse_documents,
    }
    handler = handlers.get(category)
    if handler is None:
        return BrowseResponse(category=category, total=0, limit=limit, offset=offset, items=[])
    items, total = handler(conn, settings, limit, offset, (q or "").strip())
    return BrowseResponse(
        category=category,
        total=total,
        limit=limit,
        offset=offset,
        items=items,
    )


def _count_dossiers(conn, settings: Settings) -> int:
    path = _pq(settings, "sessions/56/dossiers.parquet")
    if not path:
        return 0
    return conn.execute(f"SELECT count(*) FROM read_parquet('{path}')").fetchone()[0]


def _count_plenary_meetings(conn, settings: Settings) -> int:
    path = _pq(settings, "sessions/56/plenary/meetings.parquet")
    if not path:
        return 0
    return conn.execute(f"SELECT count(*) FROM read_parquet('{path}')").fetchone()[0]


def _count_commission_meetings(conn, settings: Settings) -> int:
    path = _pq(settings, "sessions/56/commission/meetings.parquet")
    if not path:
        return 0
    return conn.execute(f"SELECT count(*) FROM read_parquet('{path}')").fetchone()[0]


def _count_oral_questions(conn, settings: Settings) -> int:
    total = 0
    for rel in (
        "sessions/56/plenary/questions.parquet",
        "sessions/56/commission/questions.parquet",
    ):
        path = _pq(settings, rel)
        if path:
            total += conn.execute(
                f"SELECT count(*) FROM read_parquet('{path}')"
            ).fetchone()[0]
    return total


def _count_written_questions(conn, settings: Settings) -> int:
    try:
        return conn.execute("SELECT count(*) FROM written_questions").fetchone()[0]
    except duckdb.Error:
        return 0


def _count_votes(conn, settings: Settings) -> int:
    try:
        return conn.execute("SELECT count(*) FROM votes").fetchone()[0]
    except duckdb.Error:
        return 0


def _count_persons(conn, settings: Settings) -> int:
    try:
        return conn.execute(
            "SELECT count(*) FROM nodes WHERE node_type = 'Person'"
        ).fetchone()[0]
    except duckdb.Error:
        return 0


def _count_parties(conn, settings: Settings) -> int:
    try:
        return conn.execute(
            "SELECT count(*) FROM nodes WHERE node_type = 'Party'"
        ).fetchone()[0]
    except duckdb.Error:
        return 0


def _count_documents(conn, settings: Settings) -> int:
    try:
        return conn.execute(
            "SELECT count(*) FROM nodes WHERE node_type = 'Document'"
        ).fetchone()[0]
    except duckdb.Error:
        return 0


def _filter_clause(columns: list[str], q: str, params: list) -> str:
    if not q:
        return ""
    parts = []
    pattern = f"%{q}%"
    for col in columns:
        parts.append(f"CAST({col} AS VARCHAR) ILIKE ?")
        params.append(pattern)
    return f"AND ({' OR '.join(parts)})"


def _browse_dossiers(
    conn, settings: Settings, limit: int, offset: int, q: str
) -> tuple[list[BrowseItem], int]:
    path = _pq(settings, "sessions/56/dossiers.parquet")
    if not path:
        return [], 0
    params: list = []
    filt = _filter_clause(["title", "authors", "id", "eurovoc_descriptors"], q, params)
    total = conn.execute(
        f"SELECT count(*) FROM read_parquet('{path}') WHERE true {filt}",
        params,
    ).fetchone()[0]
    rows = conn.execute(
        f"""
        SELECT session_id, id, title, authors, submission_date, status
        FROM read_parquet('{path}')
        WHERE true {filt}
        ORDER BY submission_date DESC NULLS LAST, TRY_CAST(id AS INTEGER) DESC
        LIMIT ? OFFSET ?
        """,
        [*params, limit, offset],
    ).fetchall()
    items = [
        BrowseItem(
            id=f"{row[0]}/{row[1]}",
            type="Dossier",
            label=_clip(row[2] or f"Dossier {row[1]}", 100),
            subtitle=" · ".join(p for p in [row[3], row[5], row[4]] if p),
            sort_key=row[4] or "",
        )
        for row in rows
    ]
    return items, total


def _browse_plenary_meetings(
    conn, settings: Settings, limit: int, offset: int, q: str
) -> tuple[list[BrowseItem], int]:
    path = _pq(settings, "sessions/56/plenary/meetings.parquet")
    if not path:
        return [], 0
    params: list = []
    filt = _filter_clause(
        ["meeting_id", "date", "time_of_day", "start_time", "end_time"], q, params
    )
    total = conn.execute(
        f"SELECT count(*) FROM read_parquet('{path}') WHERE true {filt}",
        params,
    ).fetchone()[0]
    rows = conn.execute(
        f"""
        SELECT session_id, meeting_id, date, time_of_day, start_time, end_time
        FROM read_parquet('{path}')
        WHERE true {filt}
        ORDER BY TRY_CAST(meeting_id AS INTEGER) DESC
        LIMIT ? OFFSET ?
        """,
        [*params, limit, offset],
    ).fetchall()
    items = [
        BrowseItem(
            id=f"plenary_{row[0]}_{row[1]}",
            type="Meeting",
            label=f"Plenary meeting {row[1]}",
            subtitle=" · ".join(
                p for p in [row[2], row[3], f"{row[4]}–{row[5]}" if row[4] else ""] if p
            ),
            sort_key=row[2] or "",
        )
        for row in rows
    ]
    return items, total


def _browse_commission_meetings(
    conn, settings: Settings, limit: int, offset: int, q: str
) -> tuple[list[BrowseItem], int]:
    path = _pq(settings, "sessions/56/commission/meetings.parquet")
    if not path:
        return [], 0
    params: list = []
    filt = _filter_clause(
        ["meeting_id", "date", "commission", "chair", "time_of_day"], q, params
    )
    total = conn.execute(
        f"SELECT count(*) FROM read_parquet('{path}') WHERE true {filt}",
        params,
    ).fetchone()[0]
    rows = conn.execute(
        f"""
        SELECT session_id, meeting_id, date, commission, chair, time_of_day
        FROM read_parquet('{path}')
        WHERE true {filt}
        ORDER BY TRY_CAST(meeting_id AS INTEGER) DESC
        LIMIT ? OFFSET ?
        """,
        [*params, limit, offset],
    ).fetchall()
    items = [
        BrowseItem(
            id=f"commission_{row[0]}_{row[1]}",
            type="Meeting",
            label=f"Commission meeting {row[1]}",
            subtitle=" · ".join(p for p in [row[3], row[2], row[5], row[4]] if p),
            sort_key=row[2] or "",
        )
        for row in rows
    ]
    return items, total


def _oral_question_union(settings: Settings) -> str | None:
    selects = []
    for rel, kind in (
        ("sessions/56/plenary/questions.parquet", "plenary"),
        ("sessions/56/commission/questions.parquet", "commission"),
    ):
        path = _pq(settings, rel)
        if path:
            selects.append(
                f"""
                SELECT question_id, topics_nl, topics_fr, questioners, respondents,
                       meeting_id, '{kind}' AS meeting_kind
                FROM read_parquet('{path}')
                """
            )
    if not selects:
        return None
    return " UNION ALL ".join(selects)


def _browse_oral_questions(
    conn, settings: Settings, limit: int, offset: int, q: str
) -> tuple[list[BrowseItem], int]:
    union = _oral_question_union(settings)
    if not union:
        return [], 0
    params: list = []
    filt = _filter_clause(
        ["question_id", "topics_nl", "topics_fr", "questioners", "respondents"],
        q,
        params,
    )
    total = conn.execute(
        f"SELECT count(*) FROM ({union}) q WHERE true {filt}",
        params,
    ).fetchone()[0]
    rows = conn.execute(
        f"""
        SELECT question_id,
               coalesce(nullif(topics_nl, ''), nullif(topics_fr, ''), question_id) AS label,
               questioners, respondents, meeting_kind, meeting_id
        FROM ({union}) q
        WHERE true {filt}
        ORDER BY meeting_kind, TRY_CAST(meeting_id AS INTEGER) DESC, question_id DESC
        LIMIT ? OFFSET ?
        """,
        [*params, limit, offset],
    ).fetchall()
    items = [
        BrowseItem(
            id=row[0],
            type="Question",
            label=_clip(row[1], 100),
            subtitle=" · ".join(
                p
                for p in [
                    row[4],
                    f"meeting {row[5]}" if row[5] else "",
                    f"{row[2]} → {row[3]}" if row[2] else row[3],
                ]
                if p
            ),
            sort_key=row[0],
        )
        for row in rows
    ]
    return items, total


def _browse_written_questions(
    conn, settings: Settings, limit: int, offset: int, q: str
) -> tuple[list[BrowseItem], int]:
    try:
        params: list = []
        filt = _filter_clause(
            [
                "question_id",
                "title_nl",
                "title_fr",
                "author_raw",
                "docname",
                "text_nl",
                "text_fr",
            ],
            q,
            params,
        )
        total = conn.execute(
            f"SELECT count(*) FROM written_questions WHERE true {filt}",
            params,
        ).fetchone()[0]
        rows = conn.execute(
            f"""
            SELECT question_id,
                   coalesce(nullif(title_nl, ''), nullif(title_fr, ''), docname, question_id),
                   author_raw, depot_date, kind
            FROM written_questions
            WHERE true {filt}
            ORDER BY depot_date DESC NULLS LAST, question_id DESC
            LIMIT ? OFFSET ?
            """,
            [*params, limit, offset],
        ).fetchall()
    except duckdb.Error:
        return [], 0
    items = [
        BrowseItem(
            id=row[0],
            type="Question",
            label=_clip(row[1], 100),
            subtitle=" · ".join(p for p in [row[4], row[3], row[2]] if p),
            sort_key=row[3] or "",
        )
        for row in rows
    ]
    return items, total


def _browse_votes(
    conn, settings: Settings, limit: int, offset: int, q: str
) -> tuple[list[BrowseItem], int]:
    try:
        params: list = []
        filt = _filter_clause(
            ["vote_id", "title_nl", "title_fr", "outcome", "dossier_id"], q, params
        )
        total = conn.execute(
            f"SELECT count(*) FROM votes WHERE true {filt}",
            params,
        ).fetchone()[0]
        rows = conn.execute(
            f"""
            SELECT vote_id,
                   coalesce(nullif(title_nl, ''), nullif(title_fr, ''), vote_id),
                   date, outcome, meeting_id
            FROM votes
            WHERE true {filt}
            ORDER BY date DESC NULLS LAST, TRY_CAST(meeting_id AS INTEGER) DESC, seq DESC
            LIMIT ? OFFSET ?
            """,
            [*params, limit, offset],
        ).fetchall()
    except duckdb.Error:
        return [], 0
    items = [
        BrowseItem(
            id=row[0],
            type="Vote",
            label=_clip(row[1], 100),
            subtitle=" · ".join(p for p in [row[2], row[3], f"meeting {row[4]}"] if p),
            sort_key=row[2] or "",
        )
        for row in rows
    ]
    return items, total


def _browse_persons(
    conn, settings: Settings, limit: int, offset: int, q: str
) -> tuple[list[BrowseItem], int]:
    params: list = []
    filt = _filter_clause(["node_id", "label"], q, params)
    try:
        total = conn.execute(
            f"SELECT count(*) FROM nodes WHERE node_type = 'Person' {filt}",
            params,
        ).fetchone()[0]
        rows = conn.execute(
            f"""
            SELECT node_id, label
            FROM nodes
            WHERE node_type = 'Person' {filt}
            ORDER BY label, node_id
            LIMIT ? OFFSET ?
            """,
            [*params, limit, offset],
        ).fetchall()
    except duckdb.Error:
        return [], 0
    items = [
        BrowseItem(id=row[0], type="Person", label=row[1] or row[0], subtitle=row[0])
        for row in rows
    ]
    return items, total


def _browse_parties(
    conn, settings: Settings, limit: int, offset: int, q: str
) -> tuple[list[BrowseItem], int]:
    params: list = []
    filt = _filter_clause(["node_id", "label"], q, params)
    try:
        total = conn.execute(
            f"SELECT count(*) FROM nodes WHERE node_type = 'Party' {filt}",
            params,
        ).fetchone()[0]
        rows = conn.execute(
            f"""
            SELECT node_id, label
            FROM nodes
            WHERE node_type = 'Party' {filt}
            ORDER BY label, node_id
            LIMIT ? OFFSET ?
            """,
            [*params, limit, offset],
        ).fetchall()
    except duckdb.Error:
        return [], 0
    items = [
        BrowseItem(id=row[0], type="Party", label=row[1] or row[0], subtitle=row[0])
        for row in rows
    ]
    return items, total


def _browse_documents(
    conn, settings: Settings, limit: int, offset: int, q: str
) -> tuple[list[BrowseItem], int]:
    params: list = []
    filt = _filter_clause(["node_id", "label"], q, params)
    try:
        total = conn.execute(
            f"SELECT count(*) FROM nodes WHERE node_type = 'Document' {filt}",
            params,
        ).fetchone()[0]
        rows = conn.execute(
            f"""
            SELECT node_id, label
            FROM nodes
            WHERE node_type = 'Document' {filt}
            ORDER BY node_id DESC
            LIMIT ? OFFSET ?
            """,
            [*params, limit, offset],
        ).fetchall()
    except duckdb.Error:
        return [], 0
    items = [
        BrowseItem(id=row[0], type="Document", label=row[1] or row[0], subtitle=row[0])
        for row in rows
    ]
    return items, total
