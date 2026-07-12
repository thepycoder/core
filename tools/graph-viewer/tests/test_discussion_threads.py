import pytest

from app.db import Database
from app.queries.discussion_threads import fetch_meeting_thread, fetch_question_thread
from app.queries.entity_preview import fetch_entity_preview


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


def test_thread_includes_navigation_fields(conn):
    rows = fetch_meeting_thread(conn, "plenary", "91", limit=5)
    if not rows:
        pytest.skip("normalized utterances not available for fixture meeting")
    row = rows[0]
    for key in (
        "utterance_id",
        "speaker_entity_type",
        "speaker_entity_id",
        "item_kind",
        "item_id",
        "meeting_node_id",
    ):
        assert key in row
    assert row["meeting_node_id"] == "plenary_56_91"


def test_utterance_preview_related_includes_meeting(conn):
    row = conn.execute(
        """
        SELECT utterance_id
        FROM utterances
        WHERE meeting_kind = 'plenary' AND meeting_id = '91'
        LIMIT 1
        """
    ).fetchone()
    if not row:
        pytest.skip("normalized utterances not available")
    preview = fetch_entity_preview(conn, "Utterance", row[0])
    assert preview is not None
    related_types = {item.type for item in preview.related}
    assert "Meeting" in related_types
