from __future__ import annotations

from pathlib import Path

from app.models import Issue, IssueSample, IssuesResponse
from app.queries.issue_navigation import format_sample_label, resolve_issue_navigation


def _rows_to_samples(conn, check_id: str, rows, limit: int = 5) -> list[IssueSample]:
    cols = [desc[0] for desc in rows.description]
    samples: list[IssueSample] = []
    for row in rows.fetchmany(limit):
        data = dict(zip(cols, row, strict=True))
        nav = resolve_issue_navigation(conn, check_id, data)
        samples.append(
            IssueSample(
                data=data,
                label=nav.get("label") or format_sample_label(data),
                action=nav.get("action") or "context",
                node_type=nav.get("node_type") or "",
                node_id=nav.get("node_id") or "",
                unresolved_bucket=nav.get("unresolved_bucket") or "",
                unresolved_reason=nav.get("unresolved_reason") or "",
                artifact_id=nav.get("artifact_id") or "",
                source_url=nav.get("source_url") or data.get("source_url") or "",
                cache_path=nav.get("cache_path") or data.get("cache_path") or "",
                session_id=nav.get("session_id") or data.get("session_id") or "",
                meeting_id=nav.get("meeting_id") or data.get("meeting_id") or "",
                source_block=nav.get("source_block") or data.get("source_block") or "",
            )
        )
    return samples


def _qa_parquet_paths(data_dir: Path) -> tuple[Path, Path]:
    checks = data_dir / "qa" / "checks.parquet"
    details = data_dir / "qa" / "meeting_report_check_details.parquet"
    return checks, details


def fetch_issues(conn, data_dir: Path | None = None) -> IssuesResponse:
    """Read QA output from data/qa/ — qa (Rust) is the single source of truth."""
    from app.config import get_settings

    settings = get_settings()
    root = data_dir or settings.data_dir
    checks_path, details_path = _qa_parquet_paths(root)

    issues: list[Issue] = []

    if not checks_path.exists():
        return IssuesResponse(
            issues=[
                Issue(
                    id="qa_not_run",
                    severity="warning",
                    count=1,
                    summary="QA has not been run — execute `just qa` after `just build-graph`",
                    samples=[],
                )
            ]
        )

    conn.execute(
        f"CREATE OR REPLACE VIEW qa_checks AS SELECT * FROM read_parquet('{checks_path.as_posix()}')"
    )
    if details_path.exists():
        conn.execute(
            f"CREATE OR REPLACE VIEW qa_details AS SELECT * FROM read_parquet('{details_path.as_posix()}')"
        )

    rows = conn.execute(
        """
        SELECT "table", "check", status, count, detail, examples
        FROM qa_checks
        WHERE status != 'pass' AND "check" != 'qa.summary_vs_detail'
        ORDER BY
            CASE status WHEN 'fail' THEN 1 WHEN 'error' THEN 1 WHEN 'warn' THEN 2 ELSE 3 END,
            count DESC
        """
    ).fetchall()

    severity_map = {
        "fail": "error",
        "error": "error",
        "warn": "warning",
        "info": "info",
    }

    for table, check, status, count, detail, examples in rows:
        count_int = int(count) if str(count).isdigit() else 0
        if count_int <= 0 and status == "pass":
            continue

        samples: list[IssueSample] = []
        if details_path.exists():
            sample_rows = conn.execute(
                """
                SELECT check_id, entity_type, entity_id, expected, actual, message,
                       source_url, cache_path, meeting_kind, meeting_id,
                       session_id, source_block
                FROM qa_details
                WHERE check_id = ?
                LIMIT 5
                """,
                [check],
            )
            samples = _rows_to_samples(conn, check, sample_rows)

        issues.append(
            Issue(
                id=check.replace(".", "_"),
                severity=severity_map.get(status, "info"),
                count=count_int,
                summary=f"[{table}] {check}: {detail}",
                samples=samples,
            )
        )

    return IssuesResponse(issues=issues)
