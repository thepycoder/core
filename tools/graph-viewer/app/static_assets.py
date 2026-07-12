from pathlib import Path

STATIC_DIR = Path(__file__).resolve().parent / "static"


def asset_version() -> str:
    stamps = []
    for name in ("app.js", "style.css", "index.html"):
        path = STATIC_DIR / name
        if path.exists():
            stamps.append(path.stat().st_mtime)
    return str(int(max(stamps))) if stamps else "0"
