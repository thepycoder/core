import duckdb

from app.config import Settings
from app.db import Database


def _types(conn, view):
    return {row[0]: row[1] for row in conn.execute(f"DESCRIBE {view}").fetchall()}


def test_registered_runtime_schemas_are_typed_when_files_are_missing(tmp_path):
    db = Database.open(
        Settings(scraper_data_dir=tmp_path, scraper_cache_dir=tmp_path / "cache")
    )

    assert _types(db.conn, "nodes")["source_artifact_id"] == "VARCHAR"
    assert _types(db.conn, "edges")["confidence"] == "DOUBLE"
    assert _types(db.conn, "vote_casts")["confidence"] == "DOUBLE"
    assert _types(db.conn, "vote_result_members")["seq"] == "UINTEGER"
    assert _types(db.conn, "vote_unresolved_events")["block_start"] == "UINTEGER"
    assert _types(db.conn, "report_blocks")["has_oraspr"] == "BOOLEAN"
    assert _types(db.conn, "source_spans")["validation_status"] == "VARCHAR"
    assert _types(db.conn, "source_spans")["confidence"] == "DOUBLE"


def test_union_report_block_views(tmp_path):
    derived = tmp_path / "derived" / "sessions" / "56"
    plenary = derived / "plenary"
    commission = derived / "commission"
    plenary.mkdir(parents=True)
    commission.mkdir(parents=True)
    writer = duckdb.connect()
    writer.execute(
        f"""
        COPY (
            SELECT 'a1' artifact_id, 'hash' source_content_hash, 0::UINTEGER block_index,
                   'p' block_type, 'hello' AS "text", '{{}}' structured_json, '' AS language,
                   '' class_name, 1::UINTEGER word_count, 'x' content_hash, false has_oraspr,
                   'v1' block_parser_version, 'e1' extractor_version,
                   'http://example.test/1' source_url,
                   'sessions/56/meetings/plenary/56-1.html' cache_path
        ) TO '{(plenary / "report_blocks.parquet").as_posix()}' (FORMAT PARQUET)
        """
    )
    writer.execute(
        f"""
        COPY (
            SELECT 'a2' artifact_id, 'hash' source_content_hash, 0::UINTEGER block_index,
                   'p' block_type, 'world' AS "text", '{{}}' structured_json, '' AS language,
                   '' class_name, 1::UINTEGER word_count, 'y' content_hash, false has_oraspr,
                   'v1' block_parser_version, 'e1' extractor_version,
                   'http://example.test/2' source_url,
                   'sessions/56/meetings/commission/56-2.html' cache_path
        ) TO '{(commission / "report_blocks.parquet").as_posix()}' (FORMAT PARQUET)
        """
    )

    db = Database.open(
        Settings(scraper_data_dir=tmp_path, scraper_cache_dir=tmp_path / "cache")
    )
    rows = db.conn.execute(
        """
        SELECT regexp_extract(cache_path, 'meetings/([^/]+)/', 1) AS kind, count(*)
        FROM report_blocks
        GROUP BY 1
        ORDER BY 1
        """
    ).fetchall()
    assert rows == [("commission", 1), ("plenary", 1)]


def test_registered_views_cast_physical_columns_to_declared_types(tmp_path):
    graph = tmp_path / "graph"
    graph.mkdir()
    writer = duckdb.connect()
    edge_path = graph / "edges.parquet"
    writer.execute(
        f"""
        COPY (
            SELECT 'SPOKE' edge_type, 'Person' from_type, 'p1' from_id,
                   'Utterance' to_type, 'u1' to_id, '' AS "role", 'a1' source_artifact_id,
                   '' source_url, '' cache_path, '0.75' confidence, '' properties_json
        ) TO '{edge_path.as_posix()}' (FORMAT PARQUET)
        """
    )

    db = Database.open(
        Settings(scraper_data_dir=tmp_path, scraper_cache_dir=tmp_path / "cache")
    )
    value = db.conn.execute("SELECT confidence FROM edges").fetchone()[0]
    assert value == 0.75
    assert _types(db.conn, "edges")["confidence"] == "DOUBLE"
