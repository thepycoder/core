from qa_triage.cluster import cluster_rows
from qa_triage.models import DetailRow


def _row(check_id: str, entity_id: str = "", meeting_id: str = "", **kwargs) -> DetailRow:
    return DetailRow(
        check_id=check_id,
        severity=kwargs.get("severity", "warn"),
        status=kwargs.get("status", "warn"),
        session_id="56",
        meeting_kind=kwargs.get("meeting_kind", ""),
        meeting_id=meeting_id,
        entity_type=kwargs.get("entity_type", ""),
        entity_id=entity_id,
        expected=kwargs.get("expected", ""),
        actual=kwargs.get("actual", ""),
        message=kwargs.get("message", ""),
        source_url="",
        cache_path="",
        source_block="",
        created_at="",
    )


def test_edge_endpoint_clusters_question_off_by_one():
    rows = [
        _row(
            "graph.edge_endpoints_exist",
            entity_id="Utterance:56_commission_105_01_01_01->Question:56_commission_105_1",
            severity="fail",
            status="fail",
        ),
        _row(
            "graph.edge_endpoints_exist",
            entity_id="Utterance:56_commission_200_01_01_01->Question:56_commission_200_2",
            severity="fail",
            status="fail",
        ),
    ]
    clusters = cluster_rows(rows)
    assert len(clusters) == 1
    assert clusters[0].root_cause_id == "question_id_mismatch_utterance_part_of"
    assert clusters[0].row_count == 2


def test_cast_count_splits_identity_and_reconciliation():
    recon_vote = "56_129_4"
    rows = [
        _row("vote.compact_total_vs_member_names", entity_id=recon_vote),
        _row("vote.cast_count_vs_headline", entity_id=recon_vote),
        _row("vote.cast_count_vs_headline", entity_id="56_6_0"),
        _row("vote.cast_count_vs_headline", entity_id="56_11_0"),
    ]
    clusters = cluster_rows(rows)
    ids = {c.root_cause_id for c in clusters}
    assert "vote_identity_unresolved_names" in ids
    assert f"vote_cast_reconciliation_{recon_vote.replace('/', '_')}" in ids
    assert f"vote_appendix_{recon_vote.replace('/', '_')}" in ids


def test_speech_coverage_one_cluster_per_meeting():
    rows = [
        _row("utterance.speech_char_coverage", meeting_id="2", meeting_kind="plenary"),
        _row("utterance.speech_char_coverage", meeting_id="3", meeting_kind="plenary"),
    ]
    clusters = cluster_rows(rows)
    assert len(clusters) == 2
    assert {c.root_cause_id for c in clusters} == {
        "speech_coverage_plenary_2",
        "speech_coverage_plenary_3",
    }
