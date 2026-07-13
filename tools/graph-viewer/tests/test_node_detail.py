import duckdb
import pytest

from app.db import Database
from app.queries.node_detail import _fetch_source_evidence, fetch_node_detail


@pytest.fixture
def conn():
    db = Database.open()
    yield db.conn


def test_fetch_node_detail_vote_joins_result_id(conn):
    row = conn.execute(
        """
        SELECT vote_id, result_id
        FROM votes
        JOIN vote_casts USING (result_id)
        LIMIT 1
        """
    ).fetchone()
    if not row:
        pytest.skip("no vote with casts in local data")

    vote_id, result_id = row
    detail = fetch_node_detail(conn, "Vote", vote_id)

    assert detail.vote_breakdown is not None
    assert detail.vote_breakdown.groups
    assert detail.preview is not None
    assert detail.preview.title


def test_fetch_node_detail_vote_result_direct(conn):
    row = conn.execute(
        """
        SELECT result_id
        FROM vote_results
        LIMIT 1
        """
    ).fetchone()
    if not row:
        pytest.skip("no vote results in local data")

    result_id = row[0]
    detail = fetch_node_detail(conn, "VoteResult", result_id)

    assert detail.type == "VoteResult"
    assert detail.id == result_id
    assert detail.preview is not None
    assert "Result" in (detail.preview.title or "")


def test_fetch_node_detail_vote_fixture():
    conn = duckdb.connect()
    conn.execute(
        """
        CREATE TABLE nodes (
            node_type VARCHAR, node_id VARCHAR, label VARCHAR,
            source_url VARCHAR, cache_path VARCHAR
        )
        """
    )
    conn.execute(
        """
        INSERT INTO nodes VALUES
            ('Vote', '56-60-v1', 'Title NL', '', ''),
            ('Person', 'person-1', 'Alice Example', '', '')
        """
    )
    conn.execute(
        """
        CREATE TABLE votes AS
        SELECT * FROM (VALUES
            ('56-60-v1', '56-60-r1', 56, 60, '2024-01-01', 1, 'Title NL', 'Title FR',
             'roll_call', 'complete', 'adopted', '', '', '', '1', false, '', '')
        ) AS t(
            vote_id, result_id, session_id, meeting_id, date, seq, title_nl, title_fr,
            method, status, outcome, dossier_id, document_id, motion_id,
            source_roll_call_number, reuses_result, source_url, cache_path
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE vote_tallies AS
        SELECT * FROM (VALUES
            ('56-60-r1', 'position', 'yes', 'yes', 'yes', 'overall', 81, false),
            ('56-60-r1', 'position', 'no', 'no', 'no', 'overall', 0, false),
            ('56-60-r1', 'position', 'abstain', 'abstain', 'abstain', 'overall', 0, false)
        ) AS t(
            result_id, tally_kind, option_key, label_nl, label_fr, dimension, count, selected
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE vote_casts AS
        SELECT * FROM (VALUES
            ('cast-1', '56-60-r1', 56, 60, 'person-1', 'yes', 'Alice Example', '', '',
             'artifact-1', 'source-hash', 'report_blocks_v2', 'vote_assembly_v1', 1.0)
        ) AS t(
            vote_cast_id, result_id, session_id, meeting_id, person_id, position,
            raw_name, source_url, cache_path, artifact_id, source_content_hash,
            block_parser_version, extractor_version, confidence
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE vote_reconciliation AS
        SELECT * FROM (VALUES
            ('56-60-r1', '56', '60', '81', '0', '0', '1', '0', '0', 'true', '', '')
        ) AS t(
            result_id, session_id, meeting_id, yes, no, abstain,
            members_yes_count, members_no_count, members_abstain_count,
            reconciled, source_url, cache_path
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE vote_results AS
        SELECT * FROM (VALUES
            ('56-60-r1', 56, 60, 1, 'roll_call', true, 'complete', 'adopted', '1', '', '')
        ) AS t(
            result_id, session_id, meeting_id, seq, method, named, status, outcome,
            source_roll_call_number, source_url, cache_path
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE edges AS
        SELECT * FROM (VALUES
            ('x', 'x', 'x', 'x', 'x', '', '', '', '', 1.0, '')
        ) AS t(
            edge_type, from_type, from_id, to_type, to_id, role,
            source_artifact_id, source_url, cache_path, confidence, properties_json
        )
        WHERE false
        """
    )

    detail = fetch_node_detail(conn, "Vote", "56-60-v1")

    assert detail.vote_breakdown is not None
    yes_group = next(g for g in detail.vote_breakdown.groups if g.position == "yes")
    assert yes_group.headline_count == 81
    assert yes_group.members
    assert detail.vote_reconciliation is not None
    assert detail.vote_reconciliation["yes"] == "81"


@pytest.mark.parametrize(
    "node_type",
    [
        "Vote",
        "VoteResult",
        "Question",
        "Utterance",
        "Hearing",
        "Interpellation",
        "Proposition",
        "Notice",
    ],
)
def test_supported_node_types_link_to_report_evidence(node_type):
    conn = duckdb.connect()
    conn.execute(
        """
        CREATE TABLE source_spans AS
        SELECT * FROM (VALUES
            ('s1', 'a1', 'hash', 57, 12, ?, 'entity-1', 'title', 4, 6,
             'extraction', 'title_nl,title_fr', 0.8, 'extractor', 'parser-v2',
             'extractor-v3', 'https://example.test', 'sessions/57/meetings/commission/57-12.html',
             'valid', '')
        ) AS t(span_id, artifact_id, source_content_hash, session_id, meeting_id,
               entity_type, entity_id, span_role, block_start, block_end,
               coverage_kind, field_names, confidence, extractor,
               block_parser_version, extractor_version, source_url, cache_path,
               validation_status, unresolved_reason)
        """,
        [node_type],
    )

    evidence = _fetch_source_evidence(conn, node_type, "entity-1")
    assert len(evidence) == 1
    assert evidence[0].session_id == "57"
    assert evidence[0].meeting_kind == "commission"
    assert evidence[0].meeting_id == "12"
    assert evidence[0].block_start == 4
