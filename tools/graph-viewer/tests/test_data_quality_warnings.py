import duckdb

from app.queries.data_quality_warnings import (
    fetch_data_quality_warnings,
    looks_like_source_local_id,
)


def _make_conn_with_warnings():
    conn = duckdb.connect()
    conn.execute(
        """
        CREATE TABLE votes AS
        SELECT * FROM (VALUES
            ('56-135-v14', '56-135-r16'),
            ('56-135-v99', '56-135-r16'),
            ('56-10-v1', '56-10-r1')
        ) AS t(vote_id, result_id)
        """
    )
    conn.execute(
        """
        CREATE TABLE qa_details AS
        SELECT * FROM (VALUES
            ('wid-r16', 'source_conflict', 'vote.compact_total_vs_member_names',
             'warn', 'warn', 'compact vs appendix disagree',
             'yes=126', 'members_yes=126 members_no=0',
             'VoteResult', '56-135-r16',
             'https://example.com/ip135', 'sessions/56/meetings/plenary/56-135.html',
             '', 'art-1'),
            ('wid-dossier', 'source_anomaly', 'dossier.date_chronology',
             'warn', 'warn', 'submission after vote',
             'submission=2026-04-08', 'vote_date=2026-03-19',
             'Dossier', '56/1236',
             'https://example.com/dossier', 'sessions/56/dossiers/x.html',
             '', 'art-2'),
            ('wid-info', 'coverage', 'utterance.speech_char_coverage',
             'info', 'info', 'low coverage',
             '', '',
             'VoteResult', '56-135-r16',
             '', '', '', ''),
            ('wid-local', 'source_conflict', 'vote.appendix_bucket_counts',
             'warn', 'warn', 'should be filtered',
             '10', '8',
             'VoteResult', '16#1',
             '', '', '', '')
        ) AS t(
            warning_id, warning_kind, check_id, severity, status, message,
            expected, actual, graph_node_type, graph_node_id,
            source_url, cache_path, source_block, source_artifact_id
        )
        """
    )
    return conn


def test_looks_like_source_local_id():
    assert looks_like_source_local_id("16#1")
    assert not looks_like_source_local_id("56-135-r16")


def test_vote_result_warning_direct():
    conn = _make_conn_with_warnings()
    warnings = fetch_data_quality_warnings(conn, "VoteResult", "56-135-r16")
    assert len(warnings) == 1
    assert warnings[0].warning_id == "wid-r16"
    assert warnings[0].warning_kind == "source_conflict"


def test_vote_inherits_result_warning_once():
    conn = _make_conn_with_warnings()
    warnings = fetch_data_quality_warnings(conn, "Vote", "56-135-v14")
    assert len(warnings) == 1
    assert warnings[0].warning_id == "wid-r16"


def test_reused_votes_share_same_result_warning():
    conn = _make_conn_with_warnings()
    a = fetch_data_quality_warnings(conn, "Vote", "56-135-v14")
    b = fetch_data_quality_warnings(conn, "Vote", "56-135-v99")
    assert len(a) == 1 and len(b) == 1
    assert a[0].warning_id == b[0].warning_id == "wid-r16"


def test_dossier_warning_direct():
    conn = _make_conn_with_warnings()
    warnings = fetch_data_quality_warnings(conn, "Dossier", "56/1236")
    assert len(warnings) == 1
    assert warnings[0].check_id == "dossier.date_chronology"


def test_unrelated_node_has_no_warnings():
    conn = _make_conn_with_warnings()
    assert fetch_data_quality_warnings(conn, "VoteResult", "56-10-r1") == []
    assert fetch_data_quality_warnings(conn, "Person", "x") == []


def test_info_rows_not_actionable():
    conn = _make_conn_with_warnings()
    warnings = fetch_data_quality_warnings(conn, "VoteResult", "56-135-r16")
    assert all(w.status != "info" for w in warnings)


def test_missing_qa_returns_empty():
    conn = duckdb.connect()
    conn.execute(
        "CREATE TABLE votes AS SELECT * FROM (VALUES ('56-1-v1', '56-1-r1')) AS t(vote_id, result_id)"
    )
    # no qa_details table
    assert fetch_data_quality_warnings(conn, "Vote", "56-1-v1") == []
    assert fetch_data_quality_warnings(conn, "VoteResult", "56-1-r1") == []


def test_source_local_graph_ids_filtered():
    conn = _make_conn_with_warnings()
    warnings = fetch_data_quality_warnings(conn, "VoteResult", "16#1")
    assert warnings == []
