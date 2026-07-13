from app.models import SearchResponse, SearchResult


def _tokens(q: str) -> list[str]:
    return [part.strip() for part in q.split() if part.strip()]


def _match_score(label: str, node_id: str, tokens: list[str]) -> int:
    haystack = f"{label} {node_id}".lower()
    score = 0
    for token in tokens:
        t = token.lower()
        if t not in haystack:
            return -1
        if label.lower() == t:
            score += 100
        elif label.lower().startswith(t):
            score += 20
        elif f" {t}" in f" {haystack}":
            score += 10
        else:
            score += 1
    return score


def fetch_search(conn, q: str, node_type: str | None, limit: int) -> SearchResponse:
    q = q.strip()
    tokens = _tokens(q)
    if not tokens:
        return SearchResponse(results=[])

    results: list[SearchResult] = []

    if node_type:
        results.extend(_search_nodes(conn, tokens, node_type, limit * 3))
        if node_type == "Person":
            results.extend(_search_unresolved(conn, tokens, limit))
            results.extend(_search_speakers(conn, tokens, limit))
        if node_type == "ExternalPerson":
            results.extend(_search_external_persons(conn, tokens, limit))
        results.extend(_search_content(conn, tokens, node_type, limit))
    else:
        results.extend(_search_nodes(conn, tokens, None, limit * 3))
        results.extend(_search_unresolved(conn, tokens, limit))
        results.extend(_search_speakers(conn, tokens, limit))
        results.extend(_search_content(conn, tokens, None, limit))

    seen: set[tuple[str, ...]] = set()
    unique: list[SearchResult] = []
    for row in sorted(results, key=lambda r: (-r.score, r.label, r.id)):
        if row.type == "Unresolved":
            key = ("Unresolved", row.id)
        else:
            key = (row.source, row.type, row.id)
        if key in seen:
            continue
        seen.add(key)
        unique.append(row)

    return SearchResponse(results=unique[:limit])


def _search_nodes(
    conn, tokens: list[str], node_type: str | None, fetch_limit: int
) -> list[SearchResult]:
    where_parts = []
    params: list = []
    for token in tokens:
        where_parts.append("(n.label ILIKE ? OR n.node_id ILIKE ?)")
        pattern = f"%{token}%"
        params.extend([pattern, pattern])

    type_filter = ""
    if node_type:
        type_filter = "AND n.node_type = ?"
        params.append(node_type)

    params.append(fetch_limit)

    rows = conn.execute(
        f"""
        SELECT n.node_type, n.node_id, n.label
        FROM nodes n
        WHERE {" AND ".join(where_parts)}
        {type_filter}
        LIMIT ?
        """,
        params,
    ).fetchall()

    results: list[SearchResult] = []
    for row in rows:
        score = _match_score(row[2] or "", row[1], tokens)
        if score < 0:
            continue
        degree_in = conn.execute(
            "SELECT count(*) FROM edges WHERE to_type = ? AND to_id = ?",
            [row[0], row[1]],
        ).fetchone()[0]
        degree_out = conn.execute(
            "SELECT count(*) FROM edges WHERE from_type = ? AND from_id = ?",
            [row[0], row[1]],
        ).fetchone()[0]
        results.append(
            SearchResult(
                id=row[1],
                type=row[0],
                label=row[2] or row[1],
                degree_in=degree_in,
                degree_out=degree_out,
                source="node",
                score=score + degree_in + degree_out,
                subtitle=f"{degree_in} in · {degree_out} out",
            )
        )
    return results


def _search_unresolved(conn, tokens: list[str], fetch_limit: int) -> list[SearchResult]:
    where_parts = []
    params: list = []
    for token in tokens:
        where_parts.append(
            "(raw_name ILIKE ? OR norm_primary ILIKE ? OR norm_reordered ILIKE ?)"
        )
        pattern = f"%{token}%"
        params.extend([pattern, pattern, pattern])

    rows = conn.execute(
        f"""
        SELECT
            raw_name,
            count(*) AS mentions,
            min(source_bucket) AS source_bucket,
            min(reason) AS reason,
            min(context_id) AS sample_context_id,
            min(context_label) AS sample_context_label
        FROM unresolved
        WHERE {" AND ".join(where_parts)}
        GROUP BY raw_name
        ORDER BY mentions DESC, raw_name
        LIMIT ?
        """,
        [*params, fetch_limit],
    ).fetchall()

    results: list[SearchResult] = []
    for row in rows:
        score = _match_score(row[0], row[0], tokens)
        if score < 0:
            continue
        context_type = _context_type_for_bucket(row[2], row[4])
        results.append(
            SearchResult(
                id=row[0],
                type="Unresolved",
                label=row[0],
                degree_in=0,
                degree_out=row[1],
                source="unresolved",
                score=score + row[1],
                subtitle=(
                    f"{row[1]} mentions · {row[2]} · {row[3]}"
                    + (" · also unresolved speaker" if _speaker_exists(conn, row[0], tokens) else "")
                ),
                context_type=context_type,
                context_id=row[4] or None,
                context_label=row[5] or None,
            )
        )
    return results


def _speaker_exists(conn, name: str, _tokens: list[str]) -> bool:
    row = conn.execute(
        """
        SELECT 1 FROM utterances
        WHERE speaker_person_id = '' AND raw_speaker = ?
        LIMIT 1
        """,
        [name],
    ).fetchone()
    return row is not None


def _search_speakers(conn, tokens: list[str], fetch_limit: int) -> list[SearchResult]:
    where_parts = []
    params: list = []
    for token in tokens:
        where_parts.append("raw_speaker ILIKE ?")
        params.append(f"%{token}%")

    rows = conn.execute(
        f"""
        SELECT
            raw_speaker,
            count(*) AS mentions,
            min(CASE WHEN item_kind = 'question' THEN item_id ELSE NULL END) AS sample_question_id
        FROM utterances
        WHERE speaker_person_id = ''
          AND (speaker_entity_id IS NULL OR speaker_entity_id = '')
          AND {" AND ".join(where_parts)}
        GROUP BY raw_speaker
        ORDER BY mentions DESC, raw_speaker
        LIMIT ?
        """,
        [*params, fetch_limit],
    ).fetchall()

    results: list[SearchResult] = []
    for row in rows:
        score = _match_score(row[0], row[0], tokens)
        if score < 0:
            continue
        results.append(
            SearchResult(
                id=row[0],
                type="Unresolved",
                label=row[0],
                degree_in=0,
                degree_out=row[1],
                source="speaker",
                score=score + row[1],
                subtitle=f"{row[1]} utterances · unresolved speaker",
                context_type="Question",
                context_id=row[2] or None,
                context_label=f"question {row[2]}" if row[2] else None,
            )
        )
    return results


def _search_external_persons(
    conn, tokens: list[str], fetch_limit: int
) -> list[SearchResult]:
    try:
        where_parts = []
        params: list = []
        for token in tokens:
            where_parts.append("(display_name ILIKE ? OR external_person_id ILIKE ?)")
            pattern = f"%{token}%"
            params.extend([pattern, pattern])

        rows = conn.execute(
            f"""
            SELECT external_person_id, display_name, kind
            FROM external_persons
            WHERE {" AND ".join(where_parts)}
            LIMIT ?
            """,
            [*params, fetch_limit],
        ).fetchall()
    except Exception:
        return []

    results: list[SearchResult] = []
    for row in rows:
        score = _match_score(row[1] or "", row[0], tokens)
        if score < 0:
            continue
        degree_out = conn.execute(
            """
            SELECT count(*) FROM edges
            WHERE from_type = 'ExternalPerson' AND from_id = ?
            """,
            [row[0]],
        ).fetchone()[0]
        results.append(
            SearchResult(
                id=row[0],
                type="ExternalPerson",
                label=row[1] or row[0],
                degree_in=0,
                degree_out=degree_out,
                source="external_person",
                score=score + degree_out,
                subtitle=row[2] or "external",
            )
        )
    return results


def _context_type_for_bucket(bucket: str, context_id: str) -> str | None:
    if not context_id:
        return None
    if bucket in ("questioners", "respondents", "speakers"):
        return "Question"
    if bucket == "votes":
        return "Vote"
    return None


def _clip(text: str, limit: int = 120) -> str:
    text = (text or "").strip()
    if len(text) <= limit:
        return text
    return text[: limit - 1] + "…"


def _content_score(label: str, node_id: str, snippet: str, tokens: list[str]) -> int:
    """Score content matches; SQL already filtered on full text."""
    score = _match_score(f"{label} {snippet}", node_id, tokens)
    return score if score >= 0 else 5


def _search_content(
    conn, tokens: list[str], node_type: str | None, fetch_limit: int
) -> list[SearchResult]:
    results: list[SearchResult] = []
    if node_type in (None, "Utterance"):
        results.extend(_search_utterance_text(conn, tokens, fetch_limit))
    if node_type in (None, "Question"):
        results.extend(_search_question_content(conn, tokens, fetch_limit))
    if node_type in (None, "Answer"):
        results.extend(_search_answer_content(conn, tokens, fetch_limit))
    if node_type in (None, "Dossier"):
        results.extend(_search_dossier_content(conn, tokens, fetch_limit))
    if node_type in (None, "Document"):
        results.extend(_search_document_content(conn, tokens, fetch_limit))
    if node_type in (None, "Vote"):
        results.extend(_search_vote_content(conn, tokens, fetch_limit))
    return results


def _search_utterance_text(conn, tokens: list[str], fetch_limit: int) -> list[SearchResult]:
    where_parts = []
    params: list = []
    for token in tokens:
        where_parts.append("(text ILIKE ? OR raw_speaker ILIKE ?)")
        pattern = f"%{token}%"
        params.extend([pattern, pattern])

    rows = conn.execute(
        f"""
        SELECT utterance_id, raw_speaker, left(text, 400) AS snippet
        FROM utterances
        WHERE {" AND ".join(where_parts)}
        ORDER BY length(text) DESC
        LIMIT ?
        """,
        [*params, fetch_limit],
    ).fetchall()

    results: list[SearchResult] = []
    for row in rows:
        score = _content_score(row[1] or "", row[0], row[2] or "", tokens)
        results.append(
            SearchResult(
                id=row[0],
                type="Utterance",
                label=row[1] or row[0],
                degree_in=0,
                degree_out=0,
                source="content",
                score=score,
                subtitle=_clip(row[2], 100),
            )
        )
    return results


def _search_question_content(conn, tokens: list[str], fetch_limit: int) -> list[SearchResult]:
    from app.config import get_settings

    settings = get_settings()
    paths = []
    for rel in (
        "sessions/56/plenary/questions.parquet",
        "sessions/56/commission/questions.parquet",
    ):
        path = settings.parquet_path(rel)
        if path.exists():
            paths.append(path.as_posix())

    results: list[SearchResult] = []
    seen: set[str] = set()

    if paths:
        union = " UNION ALL ".join(
            f"""
            SELECT question_id, topics_nl, topics_fr, questioners, respondents
            FROM read_parquet('{p}')
            """
            for p in paths
        )

        where_parts = []
        params: list = []
        for token in tokens:
            where_parts.append(
                "(topics_nl ILIKE ? OR topics_fr ILIKE ? OR questioners ILIKE ? "
                "OR respondents ILIKE ?)"
            )
            pattern = f"%{token}%"
            params.extend([pattern] * 4)

        rows = conn.execute(
            f"""
            SELECT question_id, topics_nl, questioners, respondents
            FROM ({union}) q
            WHERE {" AND ".join(where_parts)}
            LIMIT ?
            """,
            [*params, fetch_limit],
        ).fetchall()

        for row in rows:
            label = row[1] or row[0]
            score = _content_score(label, row[0], f"{label} {row[2]} {row[3]}", tokens)
            results.append(
                SearchResult(
                    id=row[0],
                    type="Question",
                    label=label,
                    degree_in=0,
                    degree_out=0,
                    source="content",
                    score=score,
                    subtitle=f"{row[2]} → {row[3]}" if row[2] else "oral question match",
                )
            )
            seen.add(row[0])

        utterance_path = settings.parquet_path("normalized/utterances.parquet")
        if utterance_path.exists():
            utterance_where = []
            utterance_params: list = []
            for token in tokens:
                utterance_where.append(
                    "(u.text ILIKE ? OR u.raw_speaker ILIKE ? OR u.item_id ILIKE ?)"
                )
                pattern = f"%{token}%"
                utterance_params.extend([pattern, pattern, pattern])
            utterance_rows = conn.execute(
                f"""
                SELECT u.item_id, q.topics_nl, q.questioners, q.respondents
                FROM read_parquet('{utterance_path.as_posix()}') u
                LEFT JOIN ({union}) q ON q.question_id = u.item_id
                WHERE u.item_kind = 'question'
                  AND {" AND ".join(utterance_where)}
                LIMIT ?
                """,
                [*utterance_params, fetch_limit],
            ).fetchall()
            for row in utterance_rows:
                question_id = row[0]
                if not question_id or question_id in seen:
                    continue
                label = row[1] or question_id
                score = _content_score(label, question_id, f"{label} {row[2]} {row[3]}", tokens)
                results.append(
                    SearchResult(
                        id=question_id,
                        type="Question",
                        label=label,
                        degree_in=0,
                        degree_out=0,
                        source="content",
                        score=score,
                        subtitle="utterance text match",
                    )
                )
                seen.add(question_id)

    results.extend(_search_written_question_content(conn, tokens, fetch_limit, seen))
    return results


def _search_written_question_content(
    conn, tokens: list[str], fetch_limit: int, seen: set[str]
) -> list[SearchResult]:
    try:
        where_parts = []
        params: list = []
        for token in tokens:
            where_parts.append(
                "("
                "title_nl ILIKE ? OR title_fr ILIKE ? OR text_nl ILIKE ? OR text_fr ILIKE ? "
                "OR author_raw ILIKE ? OR docname ILIKE ? OR internal_ids ILIKE ? OR oral_refs ILIKE ?"
                ")"
            )
            pattern = f"%{token}%"
            params.extend([pattern] * 8)

        rows = conn.execute(
            f"""
            SELECT question_id, title_nl, title_fr, author_raw,
                   left(text_nl, 240) AS snippet_nl, left(text_fr, 240) AS snippet_fr
            FROM written_questions
            WHERE {" AND ".join(where_parts)}
            LIMIT ?
            """,
            [*params, fetch_limit],
        ).fetchall()
    except Exception:
        return []

    results: list[SearchResult] = []
    for row in rows:
        if row[0] in seen:
            continue
        label = row[1] or row[2] or row[0]
        snippet = row[4] or row[5] or ""
        author = " ".join((row[3] or "").split())
        score = _content_score(label, row[0], f"{label} {snippet} {author}", tokens)
        subtitle = _clip(snippet, 100) if snippet else f"written · {author}" if author else "written question"
        results.append(
            SearchResult(
                id=row[0],
                type="Question",
                label=_clip(label, 120),
                degree_in=0,
                degree_out=0,
                source="content",
                score=score + 5,
                subtitle=subtitle,
            )
        )
        seen.add(row[0])

    try:
        route_where = []
        route_params: list = []
        for token in tokens:
            route_where.append(
                "(dept_title_nl ILIKE ? OR dept_title_fr ILIKE ? OR subdept_nl ILIKE ? "
                "OR subdept_fr ILIKE ? OR questnum ILIKE ? OR sdocname ILIKE ?)"
            )
            pattern = f"%{token}%"
            route_params.extend([pattern] * 6)
        route_rows = conn.execute(
            f"""
            SELECT DISTINCT r.question_id, q.title_nl, q.title_fr, r.dept_title_nl, r.questnum
            FROM written_routes r
            JOIN written_questions q ON q.question_id = r.question_id
            WHERE {" AND ".join(route_where)}
            LIMIT ?
            """,
            [*route_params, fetch_limit],
        ).fetchall()
    except Exception:
        return results

    for row in route_rows:
        if row[0] in seen:
            continue
        label = row[1] or row[2] or row[0]
        score = _content_score(label, row[0], f"{label} {row[3]} {row[4]}", tokens)
        results.append(
            SearchResult(
                id=row[0],
                type="Question",
                label=_clip(label, 120),
                degree_in=0,
                degree_out=0,
                source="content",
                score=score,
                subtitle=f"route · {row[3]} (#{row[4]})" if row[3] else "written route match",
            )
        )
        seen.add(row[0])

    return results


def _search_answer_content(conn, tokens: list[str], fetch_limit: int) -> list[SearchResult]:
    where_parts = []
    params: list = []
    for token in tokens:
        where_parts.append("(text_nl ILIKE ? OR text_fr ILIKE ? OR answer_id ILIKE ?)")
        pattern = f"%{token}%"
        params.extend([pattern, pattern, pattern])

    try:
        rows = conn.execute(
            f"""
            SELECT answer_id, question_id, left(text_nl, 200) AS snippet_nl, left(text_fr, 200) AS snippet_fr
            FROM answers
            WHERE {" AND ".join(where_parts)}
            LIMIT ?
            """,
            [*params, fetch_limit],
        ).fetchall()
    except Exception:
        return []

    results: list[SearchResult] = []
    for row in rows:
        snippet = row[2] or row[3] or ""
        label = _clip(snippet, 120) if snippet else row[0]
        score = _content_score(label, row[0], snippet, tokens)
        kind_hint = ""
        try:
            kind_row = conn.execute(
                "SELECT kind FROM answers WHERE answer_id = ? LIMIT 1",
                [row[0]],
            ).fetchone()
            if kind_row and kind_row[0]:
                kind_hint = f" · {kind_row[0]}"
        except Exception:
            pass
        results.append(
            SearchResult(
                id=row[0],
                type="Answer",
                label=label,
                degree_in=0,
                degree_out=0,
                source="content",
                score=score,
                subtitle=f"question {row[1]}{kind_hint}" if row[1] else "answer text match",
            )
        )
    return results


def _search_dossier_content(conn, tokens: list[str], fetch_limit: int) -> list[SearchResult]:
    from app.config import get_settings

    path = get_settings().parquet_path("sessions/56/dossiers.parquet")
    if not path.exists():
        return []

    where_parts = []
    params: list = []
    for token in tokens:
        where_parts.append(
            "(title ILIKE ? OR authors ILIKE ? OR eurovoc_descriptors ILIKE ?)"
        )
        pattern = f"%{token}%"
        params.extend([pattern, pattern, pattern])

    rows = conn.execute(
        f"""
        SELECT session_id, id, left(title, 160) AS title, authors
        FROM read_parquet('{path.as_posix()}')
        WHERE {" AND ".join(where_parts)}
        LIMIT ?
        """,
        [*params, fetch_limit],
    ).fetchall()

    results: list[SearchResult] = []
    for row in rows:
        node_id = f"{row[0]}/{row[1]}"
        score = _content_score(row[2] or "", node_id, f"{row[2]} {row[3]}", tokens)
        results.append(
            SearchResult(
                id=node_id,
                type="Dossier",
                label=_clip(row[2], 80),
                degree_in=0,
                degree_out=0,
                source="content",
                score=score,
                subtitle=row[3] or node_id,
            )
        )
    return results


def _search_document_content(conn, tokens: list[str], fetch_limit: int) -> list[SearchResult]:
    from app.config import get_settings

    path = get_settings().parquet_path("sessions/56/subdocuments.parquet")
    if not path.exists():
        return []

    where_parts = []
    params: list = []
    for token in tokens:
        where_parts.append("(type ILIKE ? OR authors ILIKE ? OR dossier_id ILIKE ?)")
        pattern = f"%{token}%"
        params.extend([pattern, pattern, pattern])

    rows = conn.execute(
        f"""
        SELECT id, dossier_id, type, authors
        FROM read_parquet('{path.as_posix()}')
        WHERE {" AND ".join(where_parts)}
        LIMIT ?
        """,
        [*params, fetch_limit * 3],
    ).fetchall()

    seen: set[str] = set()
    results: list[SearchResult] = []
    for row in rows:
        if row[0] in seen:
            continue
        seen.add(row[0])
        label = f"{row[0]} ({row[2]})"
        score = _content_score(label, row[0], f"{label} {row[3]} dossier {row[1]}", tokens)
        results.append(
            SearchResult(
                id=row[0],
                type="Document",
                label=label,
                degree_in=0,
                degree_out=0,
                source="content",
                score=score,
                subtitle=f"dossier {row[1]} · {row[3]}" if row[3] else f"dossier {row[1]}",
            )
        )
        if len(results) >= fetch_limit:
            break
    return results


def _search_vote_content(conn, tokens: list[str], fetch_limit: int) -> list[SearchResult]:
    from app.config import get_settings

    path = get_settings().parquet_path("sessions/56/plenary/votes.parquet")
    if not path.exists():
        return []

    where_parts = []
    params: list = []
    for token in tokens:
        where_parts.append("(title_nl ILIKE ? OR title_fr ILIKE ?)")
        pattern = f"%{token}%"
        params.extend([pattern, pattern])

    rows = conn.execute(
        f"""
        SELECT vote_id, left(title_nl, 160) AS title_nl
        FROM read_parquet('{path.as_posix()}')
        WHERE {" AND ".join(where_parts)}
        LIMIT ?
        """,
        [*params, fetch_limit],
    ).fetchall()

    results: list[SearchResult] = []
    for row in rows:
        score = _content_score(row[1] or "", row[0], row[1] or "", tokens)
        results.append(
            SearchResult(
                id=row[0],
                type="Vote",
                label=_clip(row[1], 80),
                degree_in=0,
                degree_out=0,
                source="content",
                score=score,
                subtitle="vote title match",
            )
        )
    return results
