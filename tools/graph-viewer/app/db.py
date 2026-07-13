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


PARQUET_SOURCES: list[tuple[str, str | list[str], list[tuple[str, str]]]] = [
    (
        "nodes",
        "graph/nodes.parquet",
        [
            ("node_type", "VARCHAR"),
            ("node_id", "VARCHAR"),
            ("label", "VARCHAR"),
            ("source_artifact_id", "VARCHAR"),
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
            ("confidence", "DOUBLE"),
            ("properties_json", "VARCHAR"),
        ],
    ),
    (
        "artifacts",
        "graph/source_artifacts.parquet",
        [
            ("source_artifact_id", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
            ("source_content_hash", "VARCHAR"),
            ("block_parser_version", "VARCHAR"),
            ("extractor_version", "VARCHAR"),
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
            ("source_artifact_id", "VARCHAR"),
            ("source_content_hash", "VARCHAR"),
            ("block_parser_version", "VARCHAR"),
            ("extractor_version", "VARCHAR"),
            ("confidence", "DOUBLE"),
        ],
    ),
    (
        "vote_reconciliation",
        "normalized/vote_reconciliation.parquet",
        [
            ("result_id", "VARCHAR"),
            ("session_id", "UINTEGER"),
            ("meeting_id", "UINTEGER"),
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
            ("result_id", "VARCHAR"),
            ("session_id", "UINTEGER"),
            ("meeting_id", "UINTEGER"),
            ("person_id", "VARCHAR"),
            ("position", "VARCHAR"),
            ("raw_name", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
            ("source_artifact_id", "VARCHAR"),
            ("source_content_hash", "VARCHAR"),
            ("block_parser_version", "VARCHAR"),
            ("extractor_version", "VARCHAR"),
            ("confidence", "DOUBLE"),
        ],
    ),
    (
        "report_blocks",
        [
            "derived/sessions/56/plenary/report_blocks.parquet",
            "derived/sessions/56/commission/report_blocks.parquet",
        ],
        [
            ("artifact_id", "VARCHAR"),
            ("source_content_hash", "VARCHAR"),
            ("block_index", "UINTEGER"),
            ("block_type", "VARCHAR"),
            ("text", "VARCHAR"),
            ("structured_json", "VARCHAR"),
            ("language", "VARCHAR"),
            ("class_name", "VARCHAR"),
            ("word_count", "UINTEGER"),
            ("content_hash", "VARCHAR"),
            ("has_oraspr", "BOOLEAN"),
            ("block_parser_version", "VARCHAR"),
            ("extractor_version", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
        ],
    ),
    (
        "source_spans",
        [
            "derived/sessions/56/plenary/source_spans.parquet",
            "derived/sessions/56/commission/source_spans.parquet",
        ],
        [
            ("span_id", "VARCHAR"),
            ("artifact_id", "VARCHAR"),
            ("source_content_hash", "VARCHAR"),
            ("session_id", "UINTEGER"),
            ("meeting_id", "UINTEGER"),
            ("entity_type", "VARCHAR"),
            ("entity_id", "VARCHAR"),
            ("span_role", "VARCHAR"),
            ("block_start", "UINTEGER"),
            ("block_end", "UINTEGER"),
            ("coverage_kind", "VARCHAR"),
            ("field_names", "VARCHAR"),
            ("confidence", "DOUBLE"),
            ("extractor", "VARCHAR"),
            ("block_parser_version", "VARCHAR"),
            ("extractor_version", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
            ("validation_status", "VARCHAR"),
            ("unresolved_reason", "VARCHAR"),
        ],
    ),
    (
        "vote_result_members",
        "sessions/56/plenary/vote_result_members.parquet",
        [
            ("result_id", "VARCHAR"),
            ("position", "VARCHAR"),
            ("seq", "UINTEGER"),
            ("raw_name", "VARCHAR"),
        ],
    ),
    (
        "vote_unresolved_events",
        "sessions/56/plenary/vote_unresolved_events.parquet",
        [
            ("session_id", "UINTEGER"),
            ("meeting_id", "UINTEGER"),
            ("event_kind", "VARCHAR"),
            ("source_roll_call_number", "VARCHAR"),
            ("block_start", "UINTEGER"),
            ("block_end", "UINTEGER"),
            ("reason", "VARCHAR"),
            ("evidence_text", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
        ],
    ),
    (
        "votes",
        "sessions/56/plenary/votes.parquet",
        [
            ("vote_id", "VARCHAR"),
            ("result_id", "VARCHAR"),
            ("session_id", "UINTEGER"),
            ("meeting_id", "UINTEGER"),
            ("date", "VARCHAR"),
            ("seq", "UINTEGER"),
            ("title_nl", "VARCHAR"),
            ("title_fr", "VARCHAR"),
            ("method", "VARCHAR"),
            ("status", "VARCHAR"),
            ("outcome", "VARCHAR"),
            ("dossier_id", "VARCHAR"),
            ("document_id", "VARCHAR"),
            ("motion_id", "VARCHAR"),
            ("source_roll_call_number", "VARCHAR"),
            ("reuses_result", "BOOLEAN"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
        ],
    ),
    (
        "vote_results",
        "sessions/56/plenary/vote_results.parquet",
        [
            ("result_id", "VARCHAR"),
            ("session_id", "UINTEGER"),
            ("meeting_id", "UINTEGER"),
            ("seq", "UINTEGER"),
            ("method", "VARCHAR"),
            ("named", "BOOLEAN"),
            ("status", "VARCHAR"),
            ("outcome", "VARCHAR"),
            ("source_roll_call_number", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
        ],
    ),
    (
        "vote_tallies",
        "sessions/56/plenary/vote_tallies.parquet",
        [
            ("result_id", "VARCHAR"),
            ("tally_kind", "VARCHAR"),
            ("option_key", "VARCHAR"),
            ("label_nl", "VARCHAR"),
            ("label_fr", "VARCHAR"),
            ("dimension", "VARCHAR"),
            ("count", "UINTEGER"),
            ("selected", "BOOLEAN"),
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
            ("speaker_entity_type", "VARCHAR"),
            ("speaker_entity_id", "VARCHAR"),
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
    (
        "external_persons",
        "identity/external_persons.parquet",
        [
            ("external_person_id", "VARCHAR"),
            ("display_name", "VARCHAR"),
            ("kind", "VARCHAR"),
            ("source", "VARCHAR"),
            ("first_seen_bucket", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
        ],
    ),
    (
        "qa_checks",
        "qa/checks.parquet",
        [
            ("table", "VARCHAR"),
            ("check", "VARCHAR"),
            ("status", "VARCHAR"),
            ("count", "VARCHAR"),
            ("detail", "VARCHAR"),
            ("examples", "VARCHAR"),
        ],
    ),
    (
        "qa_details",
        "qa/meeting_report_check_details.parquet",
        [
            ("check_id", "VARCHAR"),
            ("severity", "VARCHAR"),
            ("status", "VARCHAR"),
            ("session_id", "VARCHAR"),
            ("meeting_kind", "VARCHAR"),
            ("meeting_id", "VARCHAR"),
            ("entity_type", "VARCHAR"),
            ("entity_id", "VARCHAR"),
            ("expected", "VARCHAR"),
            ("actual", "VARCHAR"),
            ("message", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
            ("source_block", "VARCHAR"),
            ("created_at", "VARCHAR"),
        ],
    ),
    (
        "answers",
        "normalized/answers.parquet",
        [
            ("answer_id", "VARCHAR"),
            ("question_id", "VARCHAR"),
            ("route_id", "VARCHAR"),
            ("kind", "VARCHAR"),
            ("text_nl", "VARCHAR"),
            ("text_fr", "VARCHAR"),
            ("status", "VARCHAR"),
            ("source_kind", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
            ("confidence", "VARCHAR"),
        ],
    ),
    (
        "written_questions",
        "sessions/56/written/questions.parquet",
        [
            ("question_id", "VARCHAR"),
            ("session_id", "VARCHAR"),
            ("docname", "VARCHAR"),
            ("kind", "VARCHAR"),
            ("author_actr_id", "VARCHAR"),
            ("author_raw", "VARCHAR"),
            ("depot_date", "VARCHAR"),
            ("deadline_date", "VARCHAR"),
            ("lang", "VARCHAR"),
            ("title_nl", "VARCHAR"),
            ("title_fr", "VARCHAR"),
            ("text_nl", "VARCHAR"),
            ("text_fr", "VARCHAR"),
            ("main_thesa_nl", "VARCHAR"),
            ("main_thesa_fr", "VARCHAR"),
            ("oral_refs", "VARCHAR"),
            ("qrva_route_ids", "VARCHAR"),
            ("internal_ids", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
        ],
    ),
    (
        "written_routes",
        "sessions/56/written/routes.parquet",
        [
            ("route_id", "VARCHAR"),
            ("question_id", "VARCHAR"),
            ("session_id", "VARCHAR"),
            ("qrva_id", "VARCHAR"),
            ("sdocname", "VARCHAR"),
            ("docname", "VARCHAR"),
            ("deptnum", "VARCHAR"),
            ("deptpres", "VARCHAR"),
            ("dept_title_nl", "VARCHAR"),
            ("dept_title_fr", "VARCHAR"),
            ("subdept_nl", "VARCHAR"),
            ("subdept_fr", "VARCHAR"),
            ("questnum", "VARCHAR"),
            ("statusq", "VARCHAR"),
            ("source_url", "VARCHAR"),
            ("cache_path", "VARCHAR"),
        ],
    ),
    (
        "external_person_bios",
        "identity/external_person_bios.parquet",
        [
            ("external_person_id", "VARCHAR"),
            ("input_hash", "VARCHAR"),
            ("bio_nl", "VARCHAR"),
            ("bio_json", "VARCHAR"),
            ("model", "VARCHAR"),
            ("search_queries", "VARCHAR"),
            ("created_at", "VARCHAR"),
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

        for view_name, rel_paths, columns in PARQUET_SOURCES:
            paths = [rel_paths] if isinstance(rel_paths, str) else rel_paths
            resolved_paths = [settings.parquet_path(path) for path in paths]
            for path in resolved_paths:
                db.sources.append(
                    ParquetSource(view_name=view_name, path=path, columns=columns)
                )
            existing_paths = [path for path in resolved_paths if path.exists()]
            if existing_paths:
                selects = [
                    db._project_parquet_select(path, columns, view_name)
                    for path in existing_paths
                ]
                conn.execute(
                    f"CREATE OR REPLACE VIEW {view_name} AS {' UNION ALL '.join(selects)}"
                )
            else:
                cols = ", ".join(f'NULL::{typ} AS "{name}"' for name, typ in columns)
                conn.execute(
                    f"CREATE OR REPLACE VIEW {view_name} AS SELECT {cols} WHERE false"
                )
                for path in resolved_paths:
                    db.warnings.append(f"Missing parquet: {path}")

        return db

    def _project_parquet_select(
        self,
        path: Path,
        columns: list[tuple[str, str]],
        view_name: str,
    ) -> str:
        escaped_path = path.as_posix().replace("'", "''")
        physical_columns = {
            row[0]
            for row in self.conn.execute(
                f"DESCRIBE SELECT * FROM read_parquet('{escaped_path}')"
            ).fetchall()
        }
        projections = []
        for name, typ in columns:
            source_name = name
            if (
                name == "source_artifact_id"
                and name not in physical_columns
                and "artifact_id" in physical_columns
            ):
                source_name = "artifact_id"
            if source_name in physical_columns:
                projections.append(f'TRY_CAST("{source_name}" AS {typ}) AS "{name}"')
            else:
                projections.append(f'NULL::{typ} AS "{name}"')
                self.warnings.append(f"Missing column {view_name}.{name}: {path}")
        return (
            f"SELECT {', '.join(projections)} FROM read_parquet('{escaped_path}')"
        )

    def file_status(self) -> list[tuple[str, bool]]:
        return [(str(source.path), source.path.exists()) for source in self.sources]

    def has_view_data(self, view_name: str) -> bool:
        sources = [s for s in self.sources if s.view_name == view_name]
        if not any(source.path.exists() for source in sources):
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
