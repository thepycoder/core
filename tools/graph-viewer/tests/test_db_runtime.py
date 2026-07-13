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
