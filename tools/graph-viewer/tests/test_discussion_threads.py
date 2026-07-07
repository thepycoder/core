import pytest

from app.db import Database
from app.queries.discussion_threads import fetch_question_thread


@pytest.fixture
def conn():
    db = Database.open()
    yield db.conn


def test_question_thread_ordering(conn):
    rows = fetch_question_thread(conn, "56_plenary_91_13")
    if not rows:
        pytest.skip("normalized utterances not available for fixture question")
    seqs = [int(r["seq"]) for r in rows]
    assert seqs == sorted(seqs)
    assert len(rows) > 10
