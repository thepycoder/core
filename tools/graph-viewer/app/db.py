from __future__ import annotations

import threading
from dataclasses import dataclass, field
from pathlib import Path

import duckdb

from app.config import Settings, get_settings


@dataclass
class ParquetSource:
    view_name: str
    path: Path
    columns: list[tuple[str, str]]


PARQUET_SOURCES: list[tuple[str, str, list[tuple[str, str]]]] = [
    (
        "nodes",
        "graph/nodes.parquet",
        [
            ("node_type", "VARCHAR"),
            ("node_id", "VARCHAR"),
            ("label", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
        ],
    ),
    (
        "edges",
        "graph/edges.parquet",
        [
            ("edge_type", "VARCHAR"),
            ("from_type", "VARCHAR"),
            ("from_id", "VARCHAR"),
            ("to_type", "VARCHAR"),
            ("to_id", "VARCHAR"),
            ("role", "VARCHAR"),
            ("source_artifact_id", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
            ("confidence", "VARCHAR"),
        ],
    ),
    (
        "artifacts",
        "graph/source_artifacts.parquet",
        [
            ("source_artifact_id", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
            ("parser_version", "VARCHAR"),
            ("scraped_at", "VARCHAR"),
        ],
    ),
    (
        "unresolved",
        "normalized/unresolved_persons.parquet",
        [
            ("raw_name", "VARCHAR"),
            ("typo_corrected", "VARCHAR"),
            ("norm_primary", "VARCHAR"),
            ("norm_reordered", "VARCHAR"),
            ("reason", "VARCHAR"),
            ("source_bucket", "VARCHAR"),
            ("role", "VARCHAR"),
            ("context_id", "VARCHAR"),
            ("context_label", "VARCHAR"),
            ("raw_field", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
        ],
    ),
    (
        "vote_reconciliation",
        "normalized/vote_reconciliation.parquet",
        [
            ("vote_id", "VARCHAR"),
            ("session_id", "VARCHAR"),
            ("meeting_id", "VARCHAR"),
            ("yes", "VARCHAR"),
            ("no", "VARCHAR"),
            ("abstain", "VARCHAR"),
            ("members_yes_count", "VARCHAR"),
            ("members_no_count", "VARCHAR"),
            ("members_abstain_count", "VARCHAR"),
            ("reconciled", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
        ],
    ),
    (
        "vote_casts",
        "normalized/vote_casts.parquet",
        [
            ("vote_cast_id", "VARCHAR"),
            ("vote_id", "VARCHAR"),
            ("session_id", "VARCHAR"),
            ("meeting_id", "VARCHAR"),
            ("person_id", "VARCHAR"),
            ("position", "VARCHAR"),
            ("raw_name", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
            ("confidence", "VARCHAR"),
        ],
    ),
    (
        "utterances",
        "normalized/utterances.parquet",
        [
            ("utterance_id", "VARCHAR"),
            ("session_id", "VARCHAR"),
            ("meeting_id", "VARCHAR"),
            ("meeting_kind", "VARCHAR"),
            ("agenda_id", "VARCHAR"),
            ("turn_number", "VARCHAR"),
            ("seq", "VARCHAR"),
            ("item_kind", "VARCHAR"),
            ("item_id", "VARCHAR"),
            ("question_ids", "VARCHAR"),
            ("dossier_id", "VARCHAR"),
            ("document_id", "VARCHAR"),
            ("motion_id", "VARCHAR"),
            ("vote_id", "VARCHAR"),
            ("raw_speaker", "VARCHAR"),
            ("speaker_role", "VARCHAR"),
            ("text", "VARCHAR"),
            ("language", "VARCHAR"),
            ("block_start", "VARCHAR"),
            ("block_end", "VARCHAR"),
            ("source_section", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
            ("speaker_person_id", "VARCHAR"),
            ("confidence", "VARCHAR"),
        ],
    ),
]


@dataclass
class Database:
    conn: duckdb.DuckDBPyConnection
    settings: Settings
    sources: list[ParquetSource] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    @classmethod
    def open(cls, settings: Settings | None = None) -> Database:
        settings = settings or get_settings()
        conn = duckdb.connect(database=":memory:")
        db = cls(conn=conn, settings=settings)

        for view_name, rel_path, columns in PARQUET_SOURCES:
            path = settings.parquet_path(rel_path)
            source = ParquetSource(view_name=view_name, path=path, columns=columns)
            db.sources.append(source)
            if path.exists():
                conn.execute(
                    f"CREATE OR REPLACE VIEW {view_name} AS "
                    f"SELECT * FROM read_parquet('{path.as_posix()}')"
                )
            else:
                cols = ", ".join(f"NULL::{typ} AS {name}" for name, typ in columns)
                conn.execute(
                    f"CREATE OR REPLACE VIEW {view_name} AS SELECT {cols} WHERE false"
                )
                db.warnings.append(f"Missing parquet: {path}")

        return db

    def file_status(self) -> list[tuple[str, bool]]:
        return [(str(source.path), source.path.exists()) for source in self.sources]

    def has_view_data(self, view_name: str) -> bool:
        source = next(s for s in self.sources if s.view_name == view_name)
        if not source.path.exists():
            return False
        row = self.conn.execute(f"SELECT count(*) FROM {view_name}").fetchone()
        return bool(row and row[0] > 0)


_db_local = threading.local()


def get_db() -> Database:
    """One DuckDB connection per thread — the driver is not thread-safe."""
    db = getattr(_db_local, "db", None)
    if db is None:
        db = Database.open()
        _db_local.db = db
    return db
