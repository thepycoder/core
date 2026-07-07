import pytest

from app.db import Database
from app.queries.node_detail import fetch_node_links


@pytest.fixture
def conn():
    db = Database.open()
    yield db.conn


def test_spoke_filter_matches_utterance_text(conn):
    row = conn.execute(
        """
        SELECT e.from_id, left(u.text, 80) AS snippet
        FROM edges e
        JOIN utterances u ON e.to_id = u.utterance_id
        WHERE e.edge_type = 'SPOKE'
          AND e.from_type = 'Person'
          AND e.to_type = 'Utterance'
          AND length(u.text) > 40
        LIMIT 1
        """
    ).fetchone()
    if not row:
        pytest.skip("no SPOKE edges with utterance text in local data")

    person_id, snippet = row
    token = snippet.split()[5]
    if len(token) < 4:
        pytest.skip("could not pick a stable token from utterance text")

    unfiltered = fetch_node_links(
        conn, "Person", person_id, "out", "SPOKE", None, 200, 0
    )
    filtered = fetch_node_links(
        conn, "Person", person_id, "out", "SPOKE", token, 200, 0
    )

    assert filtered.total <= unfiltered.total
    assert filtered.total >= 1

    matched_ids = [link.neighbor_id for link in filtered.links]
    placeholders = ", ".join("?" * len(matched_ids))
    texts = conn.execute(
        f"SELECT text FROM utterances WHERE utterance_id IN ({placeholders})",
        matched_ids,
    ).fetchall()
    assert any(token.lower() in (text[0] or "").lower() for text in texts)
