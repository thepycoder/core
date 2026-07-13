import duckdb
import pytest

from app.queries.report_coverage import fetch_report_coverage, list_report_meetings


@pytest.fixture
def conn():
    return duckdb.connect()


def test_fetch_report_coverage_empty(conn):
    payload = fetch_report_coverage(conn, "999999")
    assert payload.meeting_id == "999999"
    assert payload.blocks == []
    assert payload.coverage.total_words == 0


def test_fetch_report_coverage_with_fixture(conn):
    conn.execute(
        """
        CREATE TABLE report_blocks AS
        SELECT * FROM (VALUES
            ('a1', 'source-hash', 0, 'p', 'hello world', '{"inlines":[],"table_rows":null}', '', '', 2, 'x', false, 'report_blocks_v2', 'report_blocks_materialize_v1', 'http://example.com', 'sessions/56/meetings/plenary/56-60.html')
        ) AS t(artifact_id, source_content_hash, block_index, block_type, text, structured_json, language, class_name, word_count, content_hash, has_oraspr, block_parser_version, extractor_version, source_url, cache_path)
        """
    )
    conn.execute(
        """
        CREATE TABLE source_spans AS
        SELECT * FROM (VALUES
            ('s1', 'a1', 'source-hash', 56, 60, 'Vote', '56-60-v1', 'decision_title', 0, 1, 'extraction', 'title_nl', 1.0, 'vote_assembly', 'report_blocks_v2', 'vote_assembly_v1', 'http://example.com', 'sessions/56/meetings/plenary/56-60.html')
        ) AS t(span_id, artifact_id, source_content_hash, session_id, meeting_id, entity_type, entity_id, span_role, block_start, block_end, coverage_kind, field_names, confidence, extractor, block_parser_version, extractor_version, source_url, cache_path)
        """
    )

    payload = fetch_report_coverage(conn, "60")
    assert len(payload.blocks) == 1
    assert payload.blocks[0].has_extraction is True
    assert payload.coverage.covered_words == 2


def test_fetch_report_coverage_entity_type_filter(conn):
    conn.execute(
        """
        CREATE TABLE report_blocks AS
        SELECT * FROM (VALUES
            ('a1', 'source-hash', 0, 'p', 'hello', '{}', '', '', 1, 'x', false, 'v1', 'e1', 'http://example.com', 'sessions/56/meetings/plenary/56-60.html'),
            ('a1', 'source-hash', 1, 'p', 'world', '{}', '', '', 1, 'x', false, 'v1', 'e1', 'http://example.com', 'sessions/56/meetings/plenary/56-60.html')
        ) AS t(artifact_id, source_content_hash, block_index, block_type, text, structured_json, language, class_name, word_count, content_hash, has_oraspr, block_parser_version, extractor_version, source_url, cache_path)
        """
    )
    conn.execute(
        """
        CREATE TABLE source_spans AS
        SELECT * FROM (VALUES
            ('s1', 'a1', 'source-hash', 56, 60, 'Vote', '56-60-v1', 'decision_title', 0, 1, 'extraction', 'title_nl', 1.0, 'vote_assembly', 'v1', 'e1', 'http://example.com', 'sessions/56/meetings/plenary/56-60.html'),
            ('s2', 'a1', 'source-hash', 56, 60, 'Question', '56-60-q1', 'title', 1, 2, 'extraction', 'topics_nl', 1.0, 'agenda', 'v1', 'e1', 'http://example.com', 'sessions/56/meetings/plenary/56-60.html')
        ) AS t(span_id, artifact_id, source_content_hash, session_id, meeting_id, entity_type, entity_id, span_role, block_start, block_end, coverage_kind, field_names, confidence, extractor, block_parser_version, extractor_version, source_url, cache_path)
        """
    )

    vote_only = fetch_report_coverage(conn, "60", entity_types=["Vote"])
    assert len(vote_only.blocks) == 2
    assert vote_only.blocks[0].has_extraction is True
    assert vote_only.blocks[1].has_extraction is False
    assert vote_only.coverage.covered_words == 1

    question_only = fetch_report_coverage(conn, "60", entity_types=["Question"])
    assert question_only.blocks[1].has_extraction is True
    assert question_only.coverage.covered_words == 1


def test_fetch_report_coverage_coverage_kind_filter(conn):
    conn.execute(
        """
        CREATE TABLE report_blocks AS
        SELECT * FROM (VALUES
            ('a1', 'source-hash', 0, 'p', 'hello', '{}', '', '', 3, 'x', false, 'v1', 'e1', 'http://example.com', 'sessions/56/meetings/plenary/56-60.html')
        ) AS t(artifact_id, source_content_hash, block_index, block_type, text, structured_json, language, class_name, word_count, content_hash, has_oraspr, block_parser_version, extractor_version, source_url, cache_path)
        """
    )
    conn.execute(
        """
        CREATE TABLE source_spans AS
        SELECT * FROM (VALUES
            ('s1', 'a1', 'source-hash', 56, 60, 'Vote', '56-60-v1', 'decision_title', 0, 1, 'extraction', 'title_nl', 1.0, 'vote_assembly', 'v1', 'e1', 'http://example.com', 'sessions/56/meetings/plenary/56-60.html'),
            ('s2', 'a1', 'source-hash', 56, 60, 'Vote', '56-60-v1', 'decision_title', 0, 1, 'scope', 'title_nl', 1.0, 'vote_assembly', 'v1', 'e1', 'http://example.com', 'sessions/56/meetings/plenary/56-60.html')
        ) AS t(span_id, artifact_id, source_content_hash, session_id, meeting_id, entity_type, entity_id, span_role, block_start, block_end, coverage_kind, field_names, confidence, extractor, block_parser_version, extractor_version, source_url, cache_path)
        """
    )

    extraction_only = fetch_report_coverage(conn, "60", coverage_kinds=["extraction"])
    assert extraction_only.coverage.covered_words == 3

    scope_only = fetch_report_coverage(conn, "60", coverage_kinds=["scope"])
    assert scope_only.coverage.covered_words == 0
    assert len(scope_only.blocks[0].spans) == 1
    assert scope_only.blocks[0].spans[0].coverage_kind == "scope"


def test_fetch_report_coverage_parameterized_meeting_id(conn):
    conn.execute(
        """
        CREATE TABLE report_blocks AS
        SELECT * FROM (VALUES
            ('a1', 'source-hash', 0, 'p', 'safe', '{}', '', '', 1, 'x', false, 'v1', 'e1', 'http://example.com', 'sessions/56/meetings/plenary/56-42.html')
        ) AS t(artifact_id, source_content_hash, block_index, block_type, text, structured_json, language, class_name, word_count, content_hash, has_oraspr, block_parser_version, extractor_version, source_url, cache_path)
        """
    )
    conn.execute(
        """
        CREATE TABLE source_spans (
            span_id VARCHAR, artifact_id VARCHAR, source_content_hash VARCHAR,
            session_id UINTEGER, meeting_id UINTEGER, entity_type VARCHAR, entity_id VARCHAR,
            span_role VARCHAR, block_start UINTEGER, block_end UINTEGER, coverage_kind VARCHAR,
            field_names VARCHAR, confidence DOUBLE, extractor VARCHAR,
            block_parser_version VARCHAR, extractor_version VARCHAR,
            source_url VARCHAR, cache_path VARCHAR
        )
        """
    )

    payload = fetch_report_coverage(conn, "42'; DROP TABLE source_spans; --")
    assert payload.blocks == []


def _create_diagnostic_fixture(conn):
    conn.execute(
        """
        CREATE TABLE report_blocks AS
        SELECT * FROM (VALUES
            ('a1', 'hash-1', 0, 'h1', 'title', '{}', '', '', 2, 'b0', false, 'parser-v2', 'blocks-v1', 'https://example.test/60', 'sessions/57/meetings/commission/57-60.html'),
            ('a1', 'hash-1', 1, 'p', 'one two three', '{}', '', '', 3, 'b1', false, 'parser-v2', 'blocks-v1', 'https://example.test/60', 'sessions/57/meetings/commission/57-60.html'),
            ('a1', 'hash-1', 2, 'p', 'four five', '{}', '', '', 2, 'b2', false, 'parser-v2', 'blocks-v1', 'https://example.test/60', 'sessions/57/meetings/commission/57-60.html')
        ) AS t(artifact_id, source_content_hash, block_index, block_type, text, structured_json, language, class_name, word_count, content_hash, has_oraspr, block_parser_version, extractor_version, source_url, cache_path)
        """
    )
    conn.execute(
        """
        CREATE TABLE source_spans AS
        SELECT * FROM (VALUES
            ('valid-1', 'a1', 'hash-1', 57, 60, 'Vote', 'v1', 'title', 0, 2, 'extraction', 'title_nl', 0.9, 'votes', 'parser-v2', 'votes-v3', 'https://example.test/60', 'sessions/57/meetings/commission/57-60.html', 'valid', ''),
            ('valid-2', 'a1', 'hash-1', 57, 60, 'Question', 'q1', 'discussion', 1, 3, 'extraction', 'text', 1.0, 'questions', 'parser-v2', 'questions-v2', 'https://example.test/60', 'sessions/57/meetings/commission/57-60.html', 'valid', ''),
            ('bad-range', 'a1', 'hash-1', 57, 60, 'Vote', 'v2', 'counts', 2, 2, 'extraction', 'yes,no', 1.0, 'votes', 'parser-v2', 'votes-v3', 'https://example.test/60', 'sessions/57/meetings/commission/57-60.html', 'unresolved', 'invalid_half_open_range'),
            ('out-of-range', 'a1', 'hash-1', 57, 60, 'Vote', 'v3', 'counts', 2, 5, 'extraction', 'yes,no', 1.0, 'votes', 'parser-v2', 'votes-v3', 'https://example.test/60', 'sessions/57/meetings/commission/57-60.html', 'unresolved', 'out_of_bounds'),
            ('wrong-artifact', 'other', 'hash-1', 57, 60, 'Vote', 'v4', 'counts', 0, 1, 'extraction', 'yes', 1.0, 'votes', 'parser-v2', 'votes-v3', 'https://example.test/60', 'sessions/57/meetings/commission/57-60.html', 'valid', ''),
            ('stale-hash', 'a1', 'old-hash', 57, 60, 'Vote', 'v5', 'counts', 0, 1, 'extraction', 'yes', 1.0, 'votes', 'parser-v2', 'votes-v3', 'https://example.test/60', 'sessions/57/meetings/commission/57-60.html', 'valid', ''),
            ('stale-parser', 'a1', 'hash-1', 57, 60, 'Vote', 'v6', 'counts', 1, 2, 'scope', 'yes', 1.0, 'votes', 'parser-v1', 'votes-v3', 'https://example.test/60', 'sessions/57/meetings/commission/57-60.html', 'valid', '')
        ) AS t(span_id, artifact_id, source_content_hash, session_id, meeting_id, entity_type, entity_id, span_role, block_start, block_end, coverage_kind, field_names, confidence, extractor, block_parser_version, extractor_version, source_url, cache_path, validation_status, unresolved_reason)
        """
    )


def test_report_diagnostics_and_union_coverage(conn):
    _create_diagnostic_fixture(conn)
    payload = fetch_report_coverage(
        conn, "60", session_id="57", meeting_kind="commission"
    )

    assert payload.coverage.covered_words == 7
    assert payload.coverage.total_words == 7
    assert payload.coverage.valid_span_count == 2
    assert payload.coverage.invalid_span_count == 5
    assert len(payload.blocks[1].spans) == 3
    codes = {diagnostic.code for diagnostic in payload.diagnostics}
    assert codes == {
        "invalid_bounds",
        "wrong_artifact",
        "stale_content",
        "stale_parser",
    }


def test_report_multi_value_and_role_filters(conn):
    _create_diagnostic_fixture(conn)
    payload = fetch_report_coverage(
        conn,
        "60",
        session_id="57",
        meeting_kind="commission",
        entity_types=["Vote", "Question"],
        coverage_kinds=["extraction", "scope"],
        span_roles=["title", "discussion"],
    )
    assert payload.coverage.valid_span_count == 2
    assert payload.coverage.invalid_span_count == 0
    assert payload.coverage.covered_words == payload.coverage.total_words


def test_missing_derived_data_differs_from_valid_zero_spans(conn):
    missing = fetch_report_coverage(conn, "999")
    assert missing.derived_data_status == "missing"
    assert missing.diagnostics[0].code == "missing_derived_data"

    conn.execute(
        """
        CREATE TABLE report_blocks AS
        SELECT * FROM (VALUES
            ('a1', 'hash', 0, 'p', 'hello', '{}', '', '', 1, 'b', false, 'v2', 'v1', '', 'sessions/56/meetings/plenary/56-1.html')
        ) AS t(artifact_id, source_content_hash, block_index, block_type, text, structured_json, language, class_name, word_count, content_hash, has_oraspr, block_parser_version, extractor_version, source_url, cache_path)
        """
    )
    zero = fetch_report_coverage(conn, "1")
    assert zero.derived_data_status == "available"
    assert zero.diagnostics[0].code == "zero_spans"
    assert zero.diagnostics[0].state == "valid"


def test_report_spans_are_scoped_by_meeting_kind(conn):
    conn.execute(
        """
        CREATE TABLE report_blocks AS
        SELECT * FROM (VALUES
            ('commission-artifact', 'hash-c', 0, 'p', 'commission text', '{}', '', '', 2, 'x', false, 'v1', 'e1', 'http://example.com/c', 'sessions/56/meetings/commission/56-15.html'),
            ('plenary-artifact', 'hash-p', 0, 'p', 'plenary text', '{}', '', '', 2, 'x', false, 'v1', 'e1', 'http://example.com/p', 'sessions/56/meetings/plenary/56-15.html')
        ) AS t(artifact_id, source_content_hash, block_index, block_type, text, structured_json, language, class_name, word_count, content_hash, has_oraspr, block_parser_version, extractor_version, source_url, cache_path)
        """
    )
    conn.execute(
        """
        CREATE TABLE source_spans AS
        SELECT * FROM (VALUES
            ('c-span', 'commission-artifact', 'hash-c', 56, 15, 'Question', '56-commission-15-0', 'question_body', 0, 1, 'extraction', 'question_body_nl', 1.0, 'questions', 'v1', 'e1', 'http://example.com/c', 'sessions/56/meetings/commission/56-15.html', 'valid', ''),
            ('p-span', 'plenary-artifact', 'hash-p', 56, 15, 'Vote', '56-15-v1', 'decision_title', 0, 1, 'extraction', 'title_nl', 1.0, 'votes', 'v1', 'e1', 'http://example.com/p', 'sessions/56/meetings/plenary/56-15.html', 'valid', '')
        ) AS t(span_id, artifact_id, source_content_hash, session_id, meeting_id, entity_type, entity_id, span_role, block_start, block_end, coverage_kind, field_names, confidence, extractor, block_parser_version, extractor_version, source_url, cache_path, validation_status, unresolved_reason)
        """
    )

    commission = fetch_report_coverage(conn, "15", session_id="56", meeting_kind="commission")
    plenary = fetch_report_coverage(conn, "15", session_id="56", meeting_kind="plenary")

    assert commission.coverage.valid_span_count == 1
    assert commission.coverage.invalid_span_count == 0
    assert commission.blocks[0].has_extraction is True
    assert {diagnostic.code for diagnostic in commission.diagnostics} == set()

    assert plenary.coverage.valid_span_count == 1
    assert plenary.blocks[0].artifact_id == "plenary-artifact"


def test_report_meeting_list_is_session_and_kind_aware(conn):
    _create_diagnostic_fixture(conn)
    meetings = list_report_meetings(conn, "57", "commission")
    assert [(m.session_id, m.meeting_kind, m.meeting_id) for m in meetings] == [
        ("57", "commission", "60")
    ]
    assert list_report_meetings(conn, "56", "plenary") == []
