import duckdb

from app.config import Settings
from app.db import Database
from app.queries.browse import fetch_browse, fetch_browse_categories


def _write_fixture_data(tmp_path):
    sessions = tmp_path / "sessions" / "56"
    plenary = sessions / "plenary"
    commission = sessions / "commission"
    written = sessions / "written"
    graph = tmp_path / "graph"
    for d in (plenary, commission, written, graph):
        d.mkdir(parents=True)

    writer = duckdb.connect()
    writer.execute(
        f"""
        COPY (
            SELECT '56' session_id, '42' id, 'Climate bill' title, 'Author A' authors,
                   '20240101' submission_date, 'in progress' status,
                   '' eurovoc_descriptors
        ) TO '{(sessions / "dossiers.parquet").as_posix()}' (FORMAT PARQUET)
        """
    )
    writer.execute(
        f"""
        COPY (
            SELECT '56' session_id, 10::UINTEGER meeting_id, '2024-03-01' date,
                   'morning' time_of_day, '09:00' start_time, '12:00' end_time,
                   '' source_url, '' cache_path
        ) TO '{(plenary / "meetings.parquet").as_posix()}' (FORMAT PARQUET)
        """
    )
    writer.execute(
        f"""
        COPY (
            SELECT '56' session_id, 5::UINTEGER meeting_id, '2024-02-15' date,
                   'afternoon' time_of_day, '14:00' start_time, '17:00' end_time,
                   'Health' commission, 'Chair X' chair, '' source_url, '' cache_path
        ) TO '{(commission / "meetings.parquet").as_posix()}' (FORMAT PARQUET)
        """
    )
    writer.execute(
        f"""
        COPY (
            SELECT '56_plenary_10_1' question_id, '56' session_id, 10 meeting_id,
                   'Alice' questioners, 'Minister' respondents,
                   'Energy prices' topics_nl, '' topics_fr, '' internal_ids,
                   '' source_url, '' cache_path
        ) TO '{(plenary / "questions.parquet").as_posix()}' (FORMAT PARQUET)
        """
    )
    writer.execute(
        f"""
        COPY (
            SELECT '56_commission_5_2' question_id, '56' session_id, 5 meeting_id,
                   'Bob' questioners, 'Secretary' respondents,
                   '' topics_nl, 'Santé publique' topics_fr, '' internal_ids,
                   '' source_url, '' cache_path
        ) TO '{(commission / "questions.parquet").as_posix()}' (FORMAT PARQUET)
        """
    )
    writer.execute(
        f"""
        COPY (
            SELECT 'wq-1' question_id, '56' session_id, 'DOC-1' docname, 'written' kind,
                   '' author_actr_id, 'Carol' author_raw, '20240201' depot_date,
                   '' deadline_date, 'nl' lang, 'Written title' title_nl, '' title_fr,
                   'Body text' text_nl, '' text_fr, '' main_thesa_nl, '' main_thesa_fr,
                   '' oral_refs, '' qrva_route_ids, '' internal_ids,
                   '' source_url, '' cache_path
        ) TO '{(written / "questions.parquet").as_posix()}' (FORMAT PARQUET)
        """
    )
    writer.execute(
        f"""
        COPY (
            SELECT 'v1' vote_id, 'r1' result_id, 56::UINTEGER session_id,
                   10::UINTEGER meeting_id, '2024-03-01' date, 1::UINTEGER seq,
                   'Vote title' title_nl, '' title_fr, 'roll_call' AS "method",
                   'adopted' status, 'yes' outcome, '' dossier_id, '' document_id,
                   '' motion_id, '' source_roll_call_number, false reuses_result,
                   '' source_url, '' cache_path
        ) TO '{(plenary / "votes.parquet").as_posix()}' (FORMAT PARQUET)
        """
    )
    writer.execute(
        f"""
        COPY (
            SELECT 'Person' node_type, 'p1' node_id, 'Alice MP' AS "label",
                   '' source_artifact_id, '' source_url, '' cache_path
            UNION ALL
            SELECT 'Party', 'party-1', 'Green', '', '', ''
            UNION ALL
            SELECT 'Document', 'doc-1', 'Report 12', '', '', ''
        ) TO '{(graph / "nodes.parquet").as_posix()}' (FORMAT PARQUET)
        """
    )


def test_browse_categories_and_lists(tmp_path):
    _write_fixture_data(tmp_path)
    db = Database.open(
        Settings(scraper_data_dir=tmp_path, scraper_cache_dir=tmp_path / "cache")
    )
    conn = db.conn

    categories = fetch_browse_categories(conn, db.settings)
    ids = {c.id for c in categories}
    assert "dossiers" in ids
    assert "plenary-meetings" in ids
    assert "commission-meetings" in ids
    assert "oral-questions" in ids
    assert "written-questions" in ids
    assert "votes" in ids
    assert "persons" in ids

    dossiers = fetch_browse(conn, "dossiers", limit=10, offset=0, settings=db.settings)
    assert dossiers.total == 1
    assert dossiers.items[0].type == "Dossier"
    assert dossiers.items[0].id == "56/42"
    assert "Climate" in dossiers.items[0].label

    plenary = fetch_browse(
        conn, "plenary-meetings", limit=10, offset=0, settings=db.settings
    )
    assert plenary.items[0].id == "plenary_56_10"
    assert plenary.items[0].type == "Meeting"

    oral = fetch_browse(
        conn, "oral-questions", limit=10, offset=0, q="Energy", settings=db.settings
    )
    assert oral.total == 1
    assert oral.items[0].id == "56_plenary_10_1"

    written = fetch_browse(
        conn, "written-questions", limit=10, offset=0, settings=db.settings
    )
    assert written.items[0].id == "wq-1"
    assert "Written title" in written.items[0].label

    persons = fetch_browse(
        conn, "persons", limit=10, offset=0, q="Alice", settings=db.settings
    )
    assert persons.total == 1
    assert persons.items[0].type == "Person"


def test_unknown_browse_category_returns_empty(tmp_path):
    db = Database.open(
        Settings(scraper_data_dir=tmp_path, scraper_cache_dir=tmp_path / "cache")
    )
    result = fetch_browse(db.conn, "not-a-category", limit=10, offset=0, settings=db.settings)
    assert result.total == 0
    assert result.items == []
