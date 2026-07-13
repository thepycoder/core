"""Shared helpers for written QRVA questions, routes, and answers."""

from __future__ import annotations

import re

from app.config import Settings

_WRITTEN_ID_RE = re.compile(r"^\d+_written_")


def is_written_question_id(question_id: str) -> bool:
    return bool(_WRITTEN_ID_RE.match(question_id or ""))


def written_questions_path(settings: Settings) -> str | None:
    path = settings.parquet_path("sessions/56/written/questions.parquet")
    if not path.exists():
        return None
    return path.as_posix()


def written_routes_path(settings: Settings) -> str | None:
    path = settings.parquet_path("sessions/56/written/routes.parquet")
    if not path.exists():
        return None
    return path.as_posix()


def format_yyyymmdd(value: str) -> str:
    value = (value or "").strip()
    if len(value) == 8 and value.isdigit():
        return f"{value[6:8]}/{value[4:6]}/{value[:4]}"
    return value or "—"


def normalize_person_field(value: str) -> str:
    if not value:
        return ""
    return " ".join(part.strip() for part in value.replace("\n", " ").split() if part.strip())


def qrva_api_url(sdocname: str) -> str:
    if not sdocname:
        return ""
    return f"https://data.lachambre.be/v0/qrva/{sdocname}"


def qrva_chamber_detail_url(docname: str, session_id: str = "56") -> str:
    if not docname:
        return ""
    return (
        "https://www.dekamer.be/kvvcr/showpage.cfm"
        f"?section=/qrva&language=nl&cfm=/site/wwwcfm/qrva/qrva_detail.cfm"
        f"?dossier={docname}&legislat={session_id}"
    )


def clip_text(text: str | None, limit: int = 120) -> str:
    text = (text or "").strip()
    if len(text) <= limit:
        return text
    return text[: limit - 1] + "…"
