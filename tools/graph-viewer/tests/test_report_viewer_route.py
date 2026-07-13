from app.static_assets import STATIC_DIR


def test_report_coverage_template_exists():
    html = (STATIC_DIR / "report-coverage.html").read_text(encoding="utf-8")
    assert "Report coverage" in html
    assert "report-coverage.js" in html
    assert "report-frame" in html


def test_graph_home_links_to_report_viewer():
    html = (STATIC_DIR / "index.html").read_text(encoding="utf-8")
    assert 'href="/reports"' in html
    assert "report-section" not in html
