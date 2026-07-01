from app.models import UnresolvedResponse, UnresolvedRow


def fetch_unresolved(
    conn,
    bucket: str | None,
    reason: str | None,
    limit: int,
    offset: int,
) -> UnresolvedResponse:
    clauses = []
    params: list = []
    if bucket:
        clauses.append("source_bucket = ?")
        params.append(bucket)
    if reason:
        clauses.append("reason = ?")
        params.append(reason)

    where = f"WHERE {' AND '.join(clauses)}" if clauses else ""
    total = conn.execute(
        f"SELECT count(*) FROM unresolved {where}",
        params,
    ).fetchone()[0]

    rows = conn.execute(
        f"""
        SELECT raw_name, reason, source_bucket, role, context_id,
               context_label, source_url, cache_path
        FROM unresolved
        {where}
        ORDER BY source_bucket, raw_name, context_id
        LIMIT ? OFFSET ?
        """,
        [*params, limit, offset],
    ).fetchall()

    return UnresolvedResponse(
        rows=[
            UnresolvedRow(
                raw_name=row[0],
                reason=row[1],
                source_bucket=row[2],
                role=row[3],
                context_id=row[4],
                context_label=row[5],
                source_url=row[6] or "",
                cache_path=row[7] or "",
            )
            for row in rows
        ],
        total=total,
    )


def fetch_unresolved_context(conn, raw_name: str) -> UnresolvedResponse:
    rows = conn.execute(
        """
        SELECT raw_name, reason, source_bucket, role, context_id,
               context_label, source_url, cache_path
        FROM unresolved
        WHERE raw_name = ?
        ORDER BY source_bucket, context_id
        """,
        [raw_name],
    ).fetchall()

    return UnresolvedResponse(
        rows=[
            UnresolvedRow(
                raw_name=row[0],
                reason=row[1],
                source_bucket=row[2],
                role=row[3],
                context_id=row[4],
                context_label=row[5],
                source_url=row[6] or "",
                cache_path=row[7] or "",
            )
            for row in rows
        ],
        total=len(rows),
    )
