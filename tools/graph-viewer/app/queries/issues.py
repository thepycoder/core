from app.models import Issue, IssueSample, IssuesResponse


def _rows_to_samples(rows, limit: int = 5) -> list[IssueSample]:
    cols = [desc[0] for desc in rows.description]
    return [
        IssueSample(data=dict(zip(cols, row, strict=True)))
        for row in rows.fetchmany(limit)
    ]


def fetch_issues(conn) -> IssuesResponse:
    issues: list[Issue] = []

    orphan_from = conn.execute(
        """
        SELECT count(*) AS n
        FROM edges e
        LEFT JOIN nodes n ON e.from_type = n.node_type AND e.from_id = n.node_id
        WHERE n.node_id IS NULL
        """
    ).fetchone()[0]
    if orphan_from:
        samples = _rows_to_samples(
            conn.execute(
                """
                SELECT e.edge_type, e.from_type, e.from_id, e.to_type, e.to_id
                FROM edges e
                LEFT JOIN nodes n ON e.from_type = n.node_type AND e.from_id = n.node_id
                WHERE n.node_id IS NULL
                LIMIT 5
                """
            )
        )
        issues.append(
            Issue(
                id="orphan_from",
                severity="error",
                count=orphan_from,
                summary="Edges whose source node is missing from nodes.parquet",
                samples=samples,
            )
        )

    orphan_to = conn.execute(
        """
        SELECT count(*) AS n
        FROM edges e
        LEFT JOIN nodes n ON e.to_type = n.node_type AND e.to_id = n.node_id
        WHERE n.node_id IS NULL
        """
    ).fetchone()[0]
    if orphan_to:
        samples = _rows_to_samples(
            conn.execute(
                """
                SELECT e.edge_type, e.from_type, e.from_id, e.to_type, e.to_id
                FROM edges e
                LEFT JOIN nodes n ON e.to_type = n.node_type AND e.to_id = n.node_id
                WHERE n.node_id IS NULL
                ORDER BY e.edge_type, e.to_id
                LIMIT 5
                """
            )
        )
        issues.append(
            Issue(
                id="orphan_to",
                severity="error",
                count=orphan_to,
                summary="Edges whose target node is missing from nodes.parquet",
                samples=samples,
            )
        )

    vote_mismatch = conn.execute(
        """
        SELECT count(*) FROM vote_reconciliation WHERE reconciled != 'true'
        """
    ).fetchone()[0]
    if vote_mismatch:
        samples = _rows_to_samples(
            conn.execute(
                """
                SELECT vote_id, yes, no, abstain,
                       members_yes_count, members_no_count, members_abstain_count,
                       source_url, cache_path
                FROM vote_reconciliation
                WHERE reconciled != 'true'
                LIMIT 5
                """
            )
        )
        issues.append(
            Issue(
                id="vote_reconciliation",
                severity="warning",
                count=vote_mismatch,
                summary="Votes where headline totals do not match parsed member name counts",
                samples=samples,
            )
        )

    duplicate_utterances = conn.execute(
        """
        SELECT coalesce(sum(cnt - 1), 0)
        FROM (
            SELECT utterance_id, count(*) AS cnt
            FROM utterances
            GROUP BY 1
            HAVING count(*) > 1
        ) d
        """
    ).fetchone()[0]
    if duplicate_utterances:
        samples = _rows_to_samples(
            conn.execute(
                """
                SELECT utterance_id, count(*) AS occurrences
                FROM utterances
                GROUP BY 1
                HAVING count(*) > 1
                ORDER BY occurrences DESC
                LIMIT 5
                """
            )
        )
        issues.append(
            Issue(
                id="duplicate_utterance_ids",
                severity="warning",
                count=duplicate_utterances,
                summary="Extra utterance rows sharing the same utterance_id",
                samples=samples,
            )
        )

    without_spoke = conn.execute(
        """
        SELECT count(*)
        FROM nodes u
        WHERE u.node_type = 'Utterance'
          AND NOT EXISTS (
              SELECT 1 FROM edges e
              WHERE e.edge_type = 'SPOKE'
                AND e.to_type = 'Utterance'
                AND e.to_id = u.node_id
          )
        """
    ).fetchone()[0]
    if without_spoke:
        samples = _rows_to_samples(
            conn.execute(
                """
                SELECT u.node_id AS utterance_id, u.label AS raw_speaker
                FROM nodes u
                WHERE u.node_type = 'Utterance'
                  AND NOT EXISTS (
                      SELECT 1 FROM edges e
                      WHERE e.edge_type = 'SPOKE'
                        AND e.to_type = 'Utterance'
                        AND e.to_id = u.node_id
                  )
                LIMIT 5
                """
            )
        )
        issues.append(
            Issue(
                id="utterances_without_spoke",
                severity="info",
                count=without_spoke,
                summary="Utterance nodes with no resolved SPOKE edge",
                samples=samples,
            )
        )

    empty_scraped_at = conn.execute(
        """
        SELECT count(*) FROM artifacts
        WHERE scraped_at IS NULL OR scraped_at = ''
        """
    ).fetchone()[0]
    if empty_scraped_at:
        issues.append(
            Issue(
                id="empty_scraped_at",
                severity="info",
                count=empty_scraped_at,
                summary="Source artifacts missing scraped_at timestamp",
                samples=[],
            )
        )

    unresolved_rows = conn.execute("SELECT count(*) FROM unresolved").fetchone()[0]
    if unresolved_rows:
        samples = _rows_to_samples(
            conn.execute(
                """
                SELECT source_bucket, reason, count(*) AS n
                FROM unresolved
                GROUP BY 1, 2
                ORDER BY n DESC
                LIMIT 5
                """
            )
        )
        issues.append(
            Issue(
                id="unresolved_summary",
                severity="info",
                count=unresolved_rows,
                summary="Unresolved person names from normalization (grouped by bucket/reason)",
                samples=samples,
            )
        )

    return IssuesResponse(issues=issues)
