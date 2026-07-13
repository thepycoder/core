from __future__ import annotations

import json
from collections import defaultdict
from typing import Any

import duckdb

from app.models import (
    ReportBlock,
    ReportCoverageResponse,
    ReportCoverageStats,
    ReportDiagnostic,
    ReportMeeting,
    ReportSpan,
)


def _table_columns(conn: duckdb.DuckDBPyConnection, table: str) -> set[str]:
    try:
        return {
            row[1] for row in conn.execute(f"PRAGMA table_info('{table}')").fetchall()
        }
    except duckdb.Error:
        return set()


def list_report_meetings(
    conn: duckdb.DuckDBPyConnection,
    session_id: str,
    meeting_kind: str,
) -> list[ReportMeeting]:
    try:
        rows = conn.execute(
            """
            SELECT DISTINCT
                regexp_extract(cache_path, '/meetings/[^/]+/[^/-]+-([0-9]+)\\.html', 1)
                    AS meeting_id,
                cache_path,
                source_url
            FROM report_blocks
            WHERE cache_path LIKE ?
            ORDER BY TRY_CAST(meeting_id AS UINTEGER), meeting_id
            """,
            [f"%/meetings/{meeting_kind}/{session_id}-%"],
        ).fetchall()
    except duckdb.Error:
        rows = []
    return [
        ReportMeeting(
            session_id=session_id,
            meeting_kind=meeting_kind,
            meeting_id=row[0],
            cache_path=row[1] or "",
            source_url=row[2] or "",
        )
        for row in rows
        if row[0]
    ]


def fetch_report_coverage(
    conn: duckdb.DuckDBPyConnection,
    meeting_id: str,
    session_id: str = "56",
    meeting_kind: str = "plenary",
    entity_types: list[str] | None = None,
    coverage_kinds: list[str] | None = None,
    span_roles: list[str] | None = None,
) -> ReportCoverageResponse:
    diagnostics: list[ReportDiagnostic] = []
    try:
        blocks = conn.execute(
            """
            SELECT artifact_id, source_content_hash, block_index, block_type, text,
                   structured_json, word_count, content_hash, has_oraspr,
                   block_parser_version, extractor_version, cache_path, source_url
            FROM report_blocks rb
            WHERE rb.cache_path LIKE ?
            ORDER BY artifact_id, block_index
            """,
            [f"%/meetings/{meeting_kind}/{session_id}-{meeting_id}.html"],
        ).fetchall()
    except duckdb.Error:
        blocks = []

    span_filters = ["session_id = ?", "meeting_id = ?", "cache_path LIKE ?"]
    span_params: list[Any] = [
        session_id,
        meeting_id,
        f"%/meetings/{meeting_kind}/{session_id}-{meeting_id}.html",
    ]
    if entity_types:
        placeholders = ", ".join(["?"] * len(entity_types))
        span_filters.append(f"entity_type IN ({placeholders})")
        span_params.extend(entity_types)
    if coverage_kinds:
        placeholders = ", ".join(["?"] * len(coverage_kinds))
        span_filters.append(f"coverage_kind IN ({placeholders})")
        span_params.extend(coverage_kinds)
    if span_roles:
        placeholders = ", ".join(["?"] * len(span_roles))
        span_filters.append(f"span_role IN ({placeholders})")
        span_params.extend(span_roles)
    where = " AND ".join(span_filters)

    spans: list[tuple[Any, ...]] = []
    try:
        span_columns = _table_columns(conn, "source_spans")
        validation_status = (
            "validation_status" if "validation_status" in span_columns else "'valid'"
        )
        unresolved_reason = (
            "unresolved_reason" if "unresolved_reason" in span_columns else "''"
        )
        spans = conn.execute(
            f"""
            SELECT span_id, artifact_id, source_content_hash, entity_type, entity_id,
                   span_role, block_start, block_end, coverage_kind, field_names,
                   confidence, extractor, block_parser_version, extractor_version,
                   source_url, cache_path, {validation_status}, {unresolved_reason}
            FROM source_spans
            WHERE {where}
            ORDER BY block_start, block_end, span_role, span_id
            """,
            span_params,
        ).fetchall()
    except duckdb.Error:
        spans = []

    block_metadata: dict[str, dict[str, Any]] = {}
    block_indices: dict[str, set[int]] = defaultdict(set)
    for row in blocks:
        artifact_id = row[0] or ""
        block_indices[artifact_id].add(int(row[2]))
        block_metadata.setdefault(
            artifact_id,
            {
                "source_content_hash": row[1] or "",
                "block_parser_version": row[9] or "",
            },
        )

    span_by_block: dict[tuple[str, int], list[ReportSpan]] = defaultdict(list)
    extraction_blocks: set[tuple[str, int]] = set()
    invalid_by_reason: dict[str, list[str]] = defaultdict(list)
    valid_span_count = 0
    for row in spans:
        artifact_id = row[1] or ""
        start = int(row[6])
        end = int(row[7])
        reason = ""
        if artifact_id not in block_metadata:
            reason = "wrong_artifact"
        elif (row[2] or "") != block_metadata[artifact_id]["source_content_hash"]:
            reason = "stale_source_content"
        elif (row[12] or "") != block_metadata[artifact_id]["block_parser_version"]:
            reason = "stale_block_parser"
        elif start >= end:
            reason = "invalid_half_open_range"
        elif not set(range(start, end)).issubset(block_indices[artifact_id]):
            reason = "out_of_bounds"
        elif (row[16] or "valid") != "valid":
            reason = row[17] or "unresolved"

        status = "valid" if not reason else "unresolved"
        entry = ReportSpan(
            span_id=row[0],
            artifact_id=artifact_id,
            source_content_hash=row[2] or "",
            entity_type=row[3],
            entity_id=row[4],
            span_role=row[5],
            block_start=start,
            block_end=end,
            coverage_kind=row[8],
            field_names=row[9] or "",
            confidence=float(row[10]) if row[10] is not None else 0.0,
            extractor=row[11] or "",
            block_parser_version=row[12] or "",
            extractor_version=row[13] or "",
            source_url=row[14] or "",
            cache_path=row[15] or "",
            validation_status=status,
            unresolved_reason=reason,
        )
        if reason:
            invalid_by_reason[reason].append(row[0])
        else:
            valid_span_count += 1
        if artifact_id in block_metadata and start < end:
            for idx in range(start, end):
                if idx not in block_indices[artifact_id]:
                    continue
                span_by_block[(artifact_id, idx)].append(entry)
                if not reason and row[8] == "extraction":
                    extraction_blocks.add((artifact_id, idx))

    diagnostic_specs = {
        "wrong_artifact": (
            "wrong_artifact",
            "invalid",
            "Span references another artifact",
        ),
        "stale_source_content": (
            "stale_content",
            "stale",
            "Span source hash differs from report blocks",
        ),
        "stale_block_parser": (
            "stale_parser",
            "stale",
            "Span parser version differs from report blocks",
        ),
        "invalid_half_open_range": (
            "invalid_bounds",
            "invalid",
            "Span has an invalid half-open block range",
        ),
        "out_of_bounds": (
            "invalid_bounds",
            "invalid",
            "Span references blocks outside this artifact",
        ),
        "unresolved": ("unresolved_span", "invalid", "Span is unresolved"),
    }
    grouped_diagnostics: dict[tuple[str, str, str], list[str]] = defaultdict(list)
    for reason, span_ids in invalid_by_reason.items():
        spec = diagnostic_specs.get(
            reason,
            ("unresolved_span", "invalid", f"Span is unresolved: {reason}"),
        )
        grouped_diagnostics[spec].extend(span_ids)
    for (code, state, message), span_ids in grouped_diagnostics.items():
        diagnostics.append(
            ReportDiagnostic(
                code=code,
                state=state,
                message=message,
                count=len(span_ids),
                span_ids=span_ids,
            )
        )

    total_words = 0
    covered_words = 0
    display_blocks: list[ReportBlock] = []
    for row in blocks:
        artifact_id = row[0] or ""
        idx = int(row[2])
        word_count = int(row[6] or 0)
        total_words += word_count
        block_key = (artifact_id, idx)
        if block_key in extraction_blocks:
            covered_words += word_count
        structured: dict[str, Any] = {}
        if row[5]:
            try:
                structured = json.loads(row[5])
            except json.JSONDecodeError:
                structured = {}
        block_spans = span_by_block.get(block_key, [])
        display_blocks.append(
            ReportBlock(
                block_index=idx,
                block_type=row[3],
                text=row[4] or "",
                structured=structured,
                word_count=word_count,
                has_oraspr=bool(row[8]),
                artifact_id=artifact_id,
                source_content_hash=row[1] or "",
                content_hash=row[7] or "",
                block_parser_version=row[9] or "",
                extractor_version=row[10] or "",
                spans=block_spans,
                has_extraction=block_key in extraction_blocks,
                has_scope=any(
                    span.coverage_kind == "scope" and span.validation_status == "valid"
                    for span in block_spans
                ),
                has_invalid=any(
                    span.validation_status != "valid"
                    and not span.unresolved_reason.startswith("stale_")
                    for span in block_spans
                ),
                has_stale=any(
                    span.unresolved_reason.startswith("stale_") for span in block_spans
                ),
            )
        )

    if not blocks:
        diagnostics.insert(
            0,
            ReportDiagnostic(
                code="missing_derived_data",
                state="missing",
                message="No derived report blocks are available for this report",
            ),
        )
    elif not spans:
        diagnostics.insert(
            0,
            ReportDiagnostic(
                code="zero_spans",
                state="valid",
                message="The derived report is valid and contains zero matching spans",
                count=0,
            ),
        )

    return ReportCoverageResponse(
        session_id=session_id,
        meeting_kind=meeting_kind,
        meeting_id=meeting_id,
        blocks=display_blocks,
        coverage=ReportCoverageStats(
            total_words=total_words,
            covered_words=covered_words,
            ratio=(covered_words / total_words) if total_words else 0.0,
            valid_span_count=valid_span_count,
            invalid_span_count=sum(len(ids) for ids in invalid_by_reason.values()),
        ),
        cache_path=blocks[0][11] if blocks else "",
        source_url=blocks[0][12] if blocks else "",
        block_parser_version=blocks[0][9] if blocks else "",
        extractor_version=blocks[0][10] if blocks else "",
        derived_data_status="available" if blocks else "missing",
        diagnostics=diagnostics,
    )
