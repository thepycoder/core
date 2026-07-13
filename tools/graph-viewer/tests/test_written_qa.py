from app.config import get_settings
from app.db import Database
from app.queries.entity_preview import fetch_entity_preview
from app.queries.search import fetch_search


def test_written_question_body_search():
    db = Database.open(get_settings())
    phrase = "Graag een overzicht, onderverdeeld per bevoegdheid"
    response = fetch_search(db.conn, phrase, "Question", 10)
    ids = [row.id for row in response.results]
    assert "56_written_2024202504603" in ids


def test_written_question_preview_has_body_and_routes():
    db = Database.open(get_settings())
    preview = fetch_entity_preview(
        db.conn,
        "Question",
        "56_written_2024202504603",
        get_settings(),
    )
    assert preview is not None
    assert preview.title
    assert preview.content
    assert "programmatorische overheidsdienst" in preview.content.lower()
    assert any(
        field.label == "Kind" and field.value == "written" for field in preview.fields
    )
    assert any(
        field.label == "Author" and "Van Tigchelt" in field.value
        for field in preview.fields
    )
    assert any(related.type == "ExternalPerson" for related in preview.related)
    assert "Ministerial routes:" in preview.content


def test_written_answer_preview_links_question():
    db = Database.open(get_settings())
    row = db.conn.execute(
        """
        SELECT answer_id
        FROM answers
        WHERE question_id LIKE '56_written_%'
          AND coalesce(text_nl, '') != ''
        LIMIT 1
        """
    ).fetchone()
    assert row is not None
    preview = fetch_entity_preview(db.conn, "Answer", row[0], get_settings())
    assert preview is not None
    assert preview.content
    assert any(related.type == "Question" for related in preview.related)
