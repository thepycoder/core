import pytest

from app.db import Database
from app.queries.issue_navigation import format_sample_label, resolve_issue_navigation
from app.queries.issues import fetch_issues


@pytest.fixture
def conn():
    db = Database.open()
    yield db.conn


def test_format_sample_label_prefers_message():
    label = format_sample_label(
        {"message": "vote 56_129_4 headline yes=81/no=0/abstain=0 vs members 131/0/0"}
    )
    assert "56_129_4" in label


def test_graph_edge_endpoints_navigates_to_existing_utterance(conn):
    data = {
        "check_id": "graph.edge_endpoints_exist",
        "entity_type": "edge",
        "entity_id": "Utterance:56_commission_105_01_01_01->Question:56_commission_105_1",
        "message": "edge PART_OF missing to node Question:56_commission_105_1",
    }
    nav = resolve_issue_navigation(conn, data["check_id"], data)
    assert nav["action"] == "node"
    assert nav["node_type"] == "Utterance"
    assert nav["node_id"] == "56_commission_105_01_01_01"


def test_vote_result_check_navigates_to_vote_result(conn):
    data = {
        "check_id": "vote.cast_count_vs_headline",
        "entity_type": "vote_result",
        "entity_id": "56-2-r1",
        "message": "result 56-2-r1 cast count mismatch",
    }
    nav = resolve_issue_navigation(conn, data["check_id"], data)
    assert nav["action"] == "node"
    assert nav["node_type"] == "VoteResult"
    assert nav["node_id"] == "56-2-r1"


def test_source_block_navigates_to_report(conn):
    data = {
        "check_id": "vote.compact_total_vs_member_names",
        "entity_type": "vote",
        "entity_id": "56-129-4",
        "meeting_kind": "plenary",
        "meeting_id": "129",
        "session_id": "56",
        "source_block": "42",
        "message": "vote 56-129-4 headline mismatch at block 42",
    }
    nav = resolve_issue_navigation(conn, data["check_id"], data)
    assert nav["action"] == "report"
    assert nav["meeting_id"] == "129"
    assert nav["source_block"] == "42"
    assert nav["session_id"] == "56"
    assert nav["meeting_kind"] == "plenary"


def test_vote_check_navigates_to_vote(conn):
    data = {
        "check_id": "vote.compact_total_vs_member_names",
        "entity_type": "vote",
        "entity_id": "56_129_4",
        "message": "vote 56_129_4 headline yes=81/no=0/abstain=0 vs members 131/0/0",
    }
    nav = resolve_issue_navigation(conn, data["check_id"], data)
    assert nav["action"] == "node"
    assert nav["node_type"] == "Vote"
    assert nav["node_id"] == "56_129_4"


def test_meeting_check_navigates_to_meeting(conn):
    data = {
        "check_id": "meeting.date_source_vs_parquet",
        "entity_type": "",
        "entity_id": "",
        "meeting_kind": "commission",
        "meeting_id": "1",
        "message": "meeting 1 date 2024-07-17 not verbatim in source",
    }
    nav = resolve_issue_navigation(conn, data["check_id"], data)
    assert nav["action"] == "node"
    assert nav["node_type"] == "Meeting"
    assert nav["node_id"] == "commission_56_1"


def test_unresolved_bucket_navigation():
    data = {
        "check_id": "normalize.unresolved_persons_by_bucket",
        "entity_type": "bucket",
        "entity_id": "respondents:not_in_index",
        "message": "94 unresolved in bucket respondents (not_in_index)",
    }
    nav = resolve_issue_navigation(None, data["check_id"], data)
    assert nav["action"] == "unresolved_bucket"
    assert nav["unresolved_bucket"] == "respondents"
    assert nav["unresolved_reason"] == "not_in_index"


def test_graph_voted_on_orphan_targets_prefers_vote(conn):
    row = conn.execute(
        """
        SELECT check_id, entity_type, entity_id, message
        FROM read_parquet('../../data/qa/meeting_report_check_details.parquet')
        WHERE check_id = 'graph.voted_on_orphan_targets'
        LIMIT 1
        """
    ).fetchone()
    if not row:
        pytest.skip("no voted_on orphan samples in local QA data")

    check_id, entity_type, entity_id, message = row
    nav = resolve_issue_navigation(
        conn,
        check_id,
        {
            "check_id": check_id,
            "entity_type": entity_type,
            "entity_id": entity_id,
            "message": message,
        },
    )
    assert nav["action"] == "node"
    assert nav["node_type"] == "Vote"


def test_commission_meeting_gap_navigates_to_meeting(conn):
    data = {
        "check_id": "meeting.gaps",
        "entity_type": "meeting",
        "entity_id": "67",
        "meeting_kind": "commission",
        "meeting_id": "67",
        "message": "commission meeting 67 gap: not_found",
    }
    nav = resolve_issue_navigation(conn, data["check_id"], data)
    assert nav["action"] == "node"
    assert nav["node_id"] == "commission_56_67"


def test_fetch_issues_enriches_samples(conn):
    from app.config import get_settings

    settings = get_settings()
    checks = settings.data_dir / "qa" / "checks.parquet"
    if not checks.exists():
        pytest.skip("run `just qa` first")

    resp = fetch_issues(conn, settings.data_dir)
    edge_issue = next(
        (i for i in resp.issues if i.samples and i.samples[0].action == "node"),
        None,
    )
    if edge_issue is None:
        pytest.skip("no node-action issue samples in current QA output")
    assert edge_issue.samples
    sample = edge_issue.samples[0]
    assert sample.label
    assert sample.action == "node"
    assert sample.node_type
