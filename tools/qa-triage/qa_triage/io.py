from __future__ import annotations

import duckdb

from qa_triage.config import Settings
from qa_triage.models import DetailRow


def load_warn_fail_details(settings: Settings) -> list[DetailRow]:
    path = settings.qa_dir / "meeting_report_check_details.parquet"
    if not path.exists():
        raise FileNotFoundError(
            f"QA detail parquet not found at {path}. Run `just qa` first."
        )

    conn = duckdb.connect()
    result = conn.execute(
        f"""
        SELECT *
        FROM read_parquet('{path}')
        WHERE status IN ('warn', 'fail')
        ORDER BY check_id, meeting_id, entity_id
        """
    )
    columns = [d[0] for d in result.description]
    raw_rows = result.fetchall()
    conn.close()

    rows: list[DetailRow] = []
    for raw in raw_rows:
        mapping = dict(zip(columns, raw, strict=True))
        rows.append(DetailRow.from_mapping(mapping))
    return rows


def open_duckdb() -> duckdb.DuckDBPyConnection:
    return duckdb.connect()
