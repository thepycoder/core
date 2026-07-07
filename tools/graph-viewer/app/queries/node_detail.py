from app.config import get_settings
from app.models import (
    EdgeDetailResponse,
    EdgeGroup,
    NodeDetailResponse,
    NodeLink,
    NodeLinksResponse,
    VoteBreakdown,
    VoteCastMember,
    VotePositionGroup,
)
from app.queries.entity_preview import fetch_entity_preview


def fetch_node_detail(conn, node_type: str, node_id: str) -> NodeDetailResponse:
    row = conn.execute(
        """
        SELECT label, source_url, cache_path
        FROM nodes
        WHERE node_type = ? AND node_id = ?
        """,
        [node_type, node_id],
    ).fetchone()

    label = row[0] if row else node_id
    source_url = row[1] if row else ""
    cache_path = row[2] if row else ""

    in_edges = _edge_groups(conn, node_type, node_id, "in")
    out_edges = _edge_groups(conn, node_type, node_id, "out")
    utterances = _fetch_utterances(conn, node_type, node_id)
    vote_reconciliation = _fetch_vote_reconciliation(conn, node_type, node_id)
    vote_breakdown = _fetch_vote_breakdown(conn, node_type, node_id)
    preview = fetch_entity_preview(conn, node_type, node_id)

    return NodeDetailResponse(
        id=node_id,
        type=node_type,
        label=label or node_id,
        source_url=source_url or "",
        cache_path=cache_path or "",
        in_edges=in_edges,
        out_edges=out_edges,
        preview=preview,
        utterances=utterances,
        vote_reconciliation=vote_reconciliation,
        vote_breakdown=vote_breakdown,
    )


def _fetch_utterances(conn, node_type: str, node_id: str) -> list[dict]:
    if node_type != "Question":
        return []
    rows = conn.execute(
        """
        SELECT utterance_id, seq, raw_speaker, speaker_person_id,
               speaker_entity_type, speaker_entity_id, text, confidence
        FROM utterances
        WHERE question_id = ?
        ORDER BY cast(seq as integer), utterance_id
        LIMIT 50
        """,
        [node_id],
    ).fetchall()
    return [
        {
            "utterance_id": row[0],
            "seq": row[1],
            "raw_speaker": row[2],
            "speaker_person_id": row[3],
            "speaker_entity_type": row[4],
            "speaker_entity_id": row[5],
            "text": row[6],
            "confidence": row[7],
        }
        for row in rows
    ]


def _fetch_vote_reconciliation(conn, node_type: str, node_id: str) -> dict | None:
    if node_type != "Vote":
        return None
    row = conn.execute(
        """
        SELECT yes, no, abstain,
               members_yes_count, members_no_count, members_abstain_count,
               reconciled, source_url, cache_path
        FROM vote_reconciliation
        WHERE vote_id = ?
        LIMIT 1
        """,
        [node_id],
    ).fetchone()
    if not row:
        return None
    return {
        "yes": row[0],
        "no": row[1],
        "abstain": row[2],
        "members_yes_count": row[3],
        "members_no_count": row[4],
        "members_abstain_count": row[5],
        "reconciled": row[6],
        "source_url": row[7],
        "cache_path": row[8],
    }


def _split_csv(value: str | None) -> list[str]:
    if not value or not value.strip():
        return []
    return [part.strip() for part in value.split(",") if part.strip()]


def _person_label_lookup(conn) -> dict[str, tuple[str, str]]:
    rows = conn.execute(
        """
        SELECT node_id, label
        FROM nodes
        WHERE node_type = 'Person' AND label IS NOT NULL AND label != ''
        """
    ).fetchall()
    return {label.lower(): (person_id, label) for person_id, label in rows}


def _member_from_raw_name(
    raw_name: str, lookup: dict[str, tuple[str, str]]
) -> VoteCastMember:
    match = lookup.get(raw_name.lower())
    if match:
        person_id, label = match
        return VoteCastMember(
            person_id=person_id,
            label=label,
            raw_name=raw_name,
        )
    return VoteCastMember(label=raw_name, raw_name=raw_name, unresolved=True)


def _fetch_vote_breakdown(conn, node_type: str, node_id: str) -> VoteBreakdown | None:
    if node_type != "Vote":
        return None

    settings = get_settings()
    votes_path = settings.parquet_path("sessions/56/plenary/votes.parquet")
    headline = {"yes": "", "no": "", "abstain": ""}
    raw_lists: dict[str, list[str]] = {"yes": [], "no": [], "abstain": []}

    if votes_path.exists():
        row = conn.execute(
            f"""
            SELECT yes, no, abstain, members_yes, members_no, members_abstain
            FROM read_parquet('{votes_path.as_posix()}')
            WHERE vote_id = ?
            LIMIT 1
            """,
            [node_id],
        ).fetchone()
        if row:
            headline["yes"], headline["no"], headline["abstain"] = row[0], row[1], row[2]
            raw_lists["yes"] = _split_csv(row[3])
            raw_lists["no"] = _split_csv(row[4])
            raw_lists["abstain"] = _split_csv(row[5])

    rows = conn.execute(
        """
        SELECT
            vc.position,
            vc.person_id,
            coalesce(n.label, vc.raw_name) AS label,
            vc.raw_name,
            vc.confidence
        FROM vote_casts vc
        LEFT JOIN nodes n ON n.node_type = 'Person' AND n.node_id = vc.person_id
        WHERE vc.vote_id = ?
        ORDER BY vc.position, label, vc.raw_name
        """,
        [node_id],
    ).fetchall()

    groups_by_position: dict[str, list[VoteCastMember]] = {
        "yes": [],
        "no": [],
        "abstain": [],
    }
    resolved_names: dict[str, set[str]] = {"yes": set(), "no": set(), "abstain": set()}

    for position, person_id, label, raw_name, confidence in rows:
        if position not in groups_by_position:
            continue
        groups_by_position[position].append(
            VoteCastMember(
                person_id=person_id or None,
                label=label or raw_name,
                raw_name=raw_name,
                confidence=confidence or "exact",
            )
        )
        if raw_name:
            resolved_names[position].add(raw_name)

    person_lookup = _person_label_lookup(conn)

    if not rows and any(raw_lists.values()):
        for position in ("yes", "no", "abstain"):
            groups_by_position[position] = [
                _member_from_raw_name(name, person_lookup)
                for name in raw_lists[position]
            ]
    else:
        for position in ("yes", "no", "abstain"):
            for name in raw_lists[position]:
                if name not in resolved_names[position]:
                    groups_by_position[position].append(
                        _member_from_raw_name(name, person_lookup)
                    )

    def _has_headline(position: str) -> bool:
        value = headline[position]
        return bool(value and value != "0")

    groups = [
        VotePositionGroup(
            position=position,
            headline_count=headline[position],
            members=groups_by_position[position],
        )
        for position in ("yes", "no", "abstain")
        if groups_by_position[position] or _has_headline(position)
    ]

    if not groups and not any(headline.values()):
        return None

    return VoteBreakdown(groups=groups)


def _edge_groups(
    conn, node_type: str, node_id: str, direction: str
) -> list[EdgeGroup]:
    if direction == "out":
        count_sql = """
            SELECT edge_type, count(*) AS n
            FROM edges
            WHERE from_type = ? AND from_id = ?
            GROUP BY edge_type
            ORDER BY edge_type
        """
        sample_sql = """
            SELECT
                e.edge_type, e.from_type, e.from_id, e.to_type, e.to_id,
                e.source_artifact_id, e.source_url, e.cache_path, e.confidence,
                coalesce(n.label, e.to_id) AS neighbor_label,
                e.to_type AS neighbor_type,
                e.to_id AS neighbor_id
            FROM edges e
            LEFT JOIN nodes n ON e.to_type = n.node_type AND e.to_id = n.node_id
            WHERE e.from_type = ? AND e.from_id = ?
              AND e.edge_type = ?
            ORDER BY neighbor_label, e.to_id
            LIMIT 8
        """
    else:
        count_sql = """
            SELECT edge_type, count(*) AS n
            FROM edges
            WHERE to_type = ? AND to_id = ?
            GROUP BY edge_type
            ORDER BY edge_type
        """
        sample_sql = """
            SELECT
                e.edge_type, e.from_type, e.from_id, e.to_type, e.to_id,
                e.source_artifact_id, e.source_url, e.cache_path, e.confidence,
                coalesce(n.label, e.from_id) AS neighbor_label,
                e.from_type AS neighbor_type,
                e.from_id AS neighbor_id
            FROM edges e
            LEFT JOIN nodes n ON e.from_type = n.node_type AND e.from_id = n.node_id
            WHERE e.to_type = ? AND e.to_id = ?
              AND e.edge_type = ?
            ORDER BY neighbor_label, e.from_id
            LIMIT 8
        """

    params = [node_type, node_id]
    counts = conn.execute(count_sql, params).fetchall()

    groups: list[EdgeGroup] = []
    for etype, total in counts:
        rows = conn.execute(sample_sql, [node_type, node_id, etype]).fetchall()
        samples = [_row_to_sample(row) for row in rows]
        groups.append(
            EdgeGroup(edge_type=etype, count=total, samples=samples)
        )
    return groups


def _row_to_sample(row) -> dict:
    if len(row) < 12:
        raise ValueError(f"expected 12 columns in edge sample row, got {len(row)}: {row!r}")
    return {
        "edge_type": row[0],
        "from_type": row[1],
        "from_id": row[2],
        "to_type": row[3],
        "to_id": row[4],
        "source_artifact_id": row[5],
        "source_url": row[6],
        "cache_path": row[7],
        "confidence": row[8],
        "neighbor_label": row[9],
        "neighbor_type": row[10],
        "neighbor_id": row[11],
    }


def fetch_node_links(
    conn,
    node_type: str,
    node_id: str,
    direction: str,
    edge_type: str | None,
    q: str | None,
    limit: int,
    offset: int,
) -> NodeLinksResponse:
    if direction == "out":
        base_where = "e.from_type = ? AND e.from_id = ?"
        neighbor_label = "coalesce(n.label, e.to_id)"
        neighbor_type = "e.to_type"
        neighbor_id = "e.to_id"
        join = "LEFT JOIN nodes n ON e.to_type = n.node_type AND e.to_id = n.node_id"
    elif direction == "in":
        base_where = "e.to_type = ? AND e.to_id = ?"
        neighbor_label = "coalesce(n.label, e.from_id)"
        neighbor_type = "e.from_type"
        neighbor_id = "e.from_id"
        join = "LEFT JOIN nodes n ON e.from_type = n.node_type AND e.from_id = n.node_id"
    else:
        raise ValueError("direction must be 'in' or 'out'")

    params: list = [node_type, node_id]
    filters = [base_where]

    if edge_type:
        filters.append("e.edge_type = ?")
        params.append(edge_type)

    if q and q.strip():
        pattern = f"%{q.strip()}%"
        filters.append(
            f"({neighbor_label} ILIKE ? OR {neighbor_id} ILIKE ? OR e.edge_type ILIKE ?)"
        )
        params.extend([pattern, pattern, pattern])

    where = " AND ".join(filters)

    total = conn.execute(
        f"""
        SELECT count(*)
        FROM edges e
        {join}
        WHERE {where}
        """,
        params,
    ).fetchone()[0]

    rows = conn.execute(
        f"""
        SELECT
            e.edge_type,
            e.from_type, e.from_id, e.to_type, e.to_id,
            e.confidence,
            {neighbor_label} AS neighbor_label,
            {neighbor_type} AS neighbor_type,
            {neighbor_id} AS neighbor_id,
            e.source_url,
            e.cache_path
        FROM edges e
        {join}
        WHERE {where}
        ORDER BY neighbor_label, {neighbor_id}
        LIMIT ? OFFSET ?
        """,
        [*params, limit, offset],
    ).fetchall()

    links = [
        NodeLink(
            edge_type=row[0],
            from_type=row[1],
            from_id=row[2],
            to_type=row[3],
            to_id=row[4],
            confidence=row[5] or "exact",
            neighbor_label=row[6] or row[8],
            neighbor_type=row[7],
            neighbor_id=row[8],
            source_url=row[9] or "",
            cache_path=row[10] or "",
        )
        for row in rows
    ]

    return NodeLinksResponse(
        direction=direction,
        edge_type=edge_type,
        total=total,
        limit=limit,
        offset=offset,
        links=links,
    )


def fetch_edge_detail(
    conn,
    edge_type: str,
    from_type: str,
    from_id: str,
    to_type: str,
    to_id: str,
) -> EdgeDetailResponse | None:
    row = conn.execute(
        """
        SELECT source_artifact_id, source_url, cache_path, confidence
        FROM edges
        WHERE edge_type = ?
          AND from_type = ?
          AND from_id = ?
          AND to_type = ?
          AND to_id = ?
        LIMIT 1
        """,
        [edge_type, from_type, from_id, to_type, to_id],
    ).fetchone()
    if not row:
        return None

    artifact = None
    if row[0]:
        art = conn.execute(
            """
            SELECT source_artifact_id, source_url, cache_path, parser_version, scraped_at
            FROM artifacts
            WHERE source_artifact_id = ?
            LIMIT 1
            """,
            [row[0]],
        ).fetchone()
        if art:
            artifact = {
                "source_artifact_id": art[0],
                "source_url": art[1],
                "cache_path": art[2],
                "parser_version": art[3],
                "scraped_at": art[4],
            }

    return EdgeDetailResponse(
        edge_type=edge_type,
        from_type=from_type,
        from_id=from_id,
        to_type=to_type,
        to_id=to_id,
        source_artifact_id=row[0] or "",
        source_url=row[1] or "",
        cache_path=row[2] or "",
        confidence=row[3] or "exact",
        artifact=artifact,
    )
