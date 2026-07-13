from app.config import get_settings
from app.db import Database
from app.queries.entity_preview import fetch_entity_preview


def test_dossier_preview_tolerates_alphanumeric_subdocument_ids():
    db = Database.open(get_settings())
    preview = fetch_entity_preview(db.conn, "Dossier", "56/1663", get_settings())
    assert preview is not None
    assert preview.title
    assert preview.content
    assert "Subdocuments" in preview.content
