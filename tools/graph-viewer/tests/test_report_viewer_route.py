from app.static_assets import STATIC_DIR


def test_report_coverage_template_exists():
    html = (STATIC_DIR / "report-coverage.html").read_text(encoding="utf-8")
    assert "Report coverage" in html
    assert "report-coverage.js" in html
    assert "report-frame" in html


def test_graph_home_embeds_coverage_panel():
    html = (STATIC_DIR / "index.html").read_text(encoding="utf-8")
    assert "coverage-drawer" in html
    assert "coverage-panel.js" in html
    assert 'href="/reports"' not in html
