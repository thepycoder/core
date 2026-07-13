from __future__ import annotations

import duckdb


def resolve_result_id(conn, node_type: str, node_id: str) -> str | None:
    if node_type == "VoteResult":
        return node_id
    if node_type != "Vote":
        return None

    row = conn.execute(
        """
        SELECT result_id
        FROM votes
        WHERE vote_id = ?
        LIMIT 1
        """,
        [node_id],
    ).fetchone()
    return row[0] if row else None


def fetch_headline_tallies(conn, result_id: str) -> dict[str, int]:
    rows = conn.execute(
        """
        SELECT option_key, count
        FROM vote_tallies
        WHERE result_id = ?
          AND tally_kind = 'position'
          AND dimension = 'overall'
          AND option_key IN ('yes', 'no', 'abstain')
        """,
        [result_id],
    ).fetchall()
    headline = {"yes": 0, "no": 0, "abstain": 0}
    for option_key, count in rows:
        if option_key in headline:
            headline[option_key] = int(count or 0)
    return headline


def fetch_result_member_names(
    conn,
    result_id: str,
) -> dict[str, list[str]]:
    raw_lists: dict[str, list[str]] = {"yes": [], "no": [], "abstain": []}
    try:
        rows = conn.execute(
            """
            SELECT position, raw_name
            FROM vote_result_members
            WHERE result_id = ?
            ORDER BY seq, raw_name
            """,
            [result_id],
        ).fetchall()
    except duckdb.Error:
        return raw_lists
    for position, raw_name in rows:
        if position in raw_lists and raw_name:
            raw_lists[position].append(raw_name.strip())
    return raw_lists


def format_tally_result(headline: dict[str, int]) -> str:
    parts = []
    for position, label in (("yes", "Yes"), ("no", "No"), ("abstain", "Abstain")):
        value = headline.get(position, 0)
        if value:
            parts.append(f"{label} {value}")
    return " · ".join(parts) if parts else "—"
