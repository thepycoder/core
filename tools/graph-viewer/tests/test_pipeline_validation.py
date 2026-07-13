"""Pipeline invariants against rebuilt parquet (skipped when data absent)."""

from __future__ import annotations

import duckdb
import pytest

from app.config import get_settings


@pytest.fixture
def data_conn():
    settings = get_settings()
    votes = settings.parquet_path("sessions/56/plenary/votes.parquet")
    if not votes.exists():
        pytest.skip("run `just scrape-plenary-meetings` first")
    conn = duckdb.connect()
    conn.execute(
        f"CREATE OR REPLACE VIEW votes AS SELECT * FROM read_parquet('{votes.as_posix()}')"
    )
    spans = settings.parquet_path("derived/sessions/56/plenary/source_spans.parquet")
    if spans.exists():
        conn.execute(
            f"CREATE OR REPLACE VIEW source_spans AS SELECT * FROM read_parquet('{spans.as_posix()}')"
        )
    results = settings.parquet_path("sessions/56/plenary/vote_results.parquet")
    conn.execute(
        f"CREATE OR REPLACE VIEW vote_results AS SELECT * FROM read_parquet('{results.as_posix()}')"
    )
    return conn


def test_every_decision_has_result(data_conn):
    orphan = data_conn.execute(
        """
        SELECT count(*) FROM votes v
        LEFT JOIN vote_results r ON v.result_id = r.result_id
        WHERE r.result_id IS NULL
        """
    ).fetchone()[0]
    assert orphan == 0


def test_no_quorum_results_have_no_yes_tallies(data_conn):
    settings = get_settings()
    tallies = settings.parquet_path("sessions/56/plenary/vote_tallies.parquet")
    if not tallies.exists():
        pytest.skip("no tallies")
    data_conn.execute(
        f"CREATE OR REPLACE VIEW vote_tallies AS SELECT * FROM read_parquet('{tallies.as_posix()}')"
    )
    bad = data_conn.execute(
        """
        SELECT count(*) FROM vote_results r
        JOIN vote_tallies t ON r.result_id = t.result_id
        WHERE r.status = 'no_quorum' AND t.option_key IN ('yes','no','abstain')
        """
    ).fetchone()[0]
    assert bad == 0


def test_source_spans_have_valid_ranges(data_conn):
    try:
        invalid = data_conn.execute(
            """
            SELECT count(*) FROM source_spans
            WHERE CAST(block_start AS INTEGER) >= CAST(block_end AS INTEGER)
            """
        ).fetchone()[0]
    except duckdb.CatalogException:
        pytest.skip("no source_spans view")
    assert invalid == 0
