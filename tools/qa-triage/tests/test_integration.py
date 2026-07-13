from __future__ import annotations

import json
from pathlib import Path

import pytest

from qa_triage.config import CORE_ROOT, get_settings
from qa_triage.evidence import build_evidence
from qa_triage.io import load_warn_fail_details, open_duckdb
from qa_triage.cluster import cluster_rows

DETAIL_PATH = CORE_ROOT / "data" / "qa" / "meeting_report_check_details.parquet"


@pytest.mark.skipif(not DETAIL_PATH.exists(), reason="requires `just qa` output")
def test_live_cluster_row_counts_match_summary():
    rows = load_warn_fail_details(get_settings())
    assert sum(1 for _ in rows) == 9916
    clusters = cluster_rows(rows)
    edge = next(c for c in clusters if c.root_cause_id == "question_id_mismatch_utterance_part_of")
    assert edge.row_count == 8160


@pytest.mark.skipif(not DETAIL_PATH.exists(), reason="requires `just qa` output")
def test_question_mismatch_evidence_includes_nearby_nodes():
    rows = load_warn_fail_details(get_settings())
    clusters = cluster_rows(rows)
    cluster = next(c for c in clusters if c.root_cause_id == "question_id_mismatch_utterance_part_of")
    settings = get_settings()
    conn = open_duckdb()
    evidence = build_evidence(cluster, settings, conn)
    conn.close()
    assert evidence["parquet_samples"]["missing_question_target"] == "56_commission_105_1"
    nodes = evidence["parquet_samples"]["graph_question_nodes_nearby"]
    assert any(n.get("node_id") == "56_commission_105_0" for n in nodes)


@pytest.mark.skipif(not DETAIL_PATH.exists(), reason="requires `just qa` output")
def test_manifest_written_after_dry_run():
    manifest_path = CORE_ROOT / "data" / "qa" / "reports" / "manifest.json"
    if not manifest_path.exists():
        pytest.skip("run `just qa-triage --dry-run` first")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    assert manifest["detail_rows"] == 9916
    assert manifest["cluster_count"] == 87
