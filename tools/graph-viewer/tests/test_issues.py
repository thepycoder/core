import pytest

from app.config import get_settings
from app.db import Database
from app.queries.issues import fetch_issues


@pytest.fixture
def conn():
    db = Database.open()
    yield db.conn


def test_fetch_issues_reads_qa_parquet(conn):
    settings = get_settings()
    checks = settings.data_dir / "qa" / "checks.parquet"
    if not checks.exists():
        pytest.skip("run `just qa` first")

    resp = fetch_issues(conn, settings.data_dir)
    assert resp.issues
    assert any(i.id == "vote_compact_total_vs_member_names" for i in resp.issues)
