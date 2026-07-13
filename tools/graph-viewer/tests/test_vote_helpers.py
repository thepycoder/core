import duckdb

from app.queries.vote_helpers import fetch_result_member_names, resolve_result_id


def test_raw_vote_members_are_read_from_registered_view_in_sequence():
    conn = duckdb.connect()
    conn.execute(
        """
        CREATE TABLE vote_result_members(
            result_id VARCHAR, position VARCHAR, seq UINTEGER, raw_name VARCHAR
        )
        """
    )
    conn.execute(
        """
        INSERT INTO vote_result_members VALUES
            ('r1', 'yes', 2, 'Beta'),
            ('r1', 'yes', 1, 'Alpha'),
            ('r1', 'no', 1, 'Gamma')
        """
    )
    assert fetch_result_member_names(conn, "r1") == {
        "yes": ["Alpha", "Beta"],
        "no": ["Gamma"],
        "abstain": [],
    }


def test_reused_votes_resolve_to_the_shared_result():
    conn = duckdb.connect()
    conn.execute(
        """
        CREATE TABLE votes(vote_id VARCHAR, result_id VARCHAR, reuses_result BOOLEAN)
        """
    )
    conn.execute("INSERT INTO votes VALUES ('v1', 'r1', false), ('v2', 'r1', true)")
    assert resolve_result_id(conn, "Vote", "v1") == "r1"
    assert resolve_result_id(conn, "Vote", "v2") == "r1"
    assert resolve_result_id(conn, "VoteResult", "r1") == "r1"
