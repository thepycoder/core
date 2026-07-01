from pathlib import Path

from fastapi import APIRouter, HTTPException, Query
from fastapi.responses import FileResponse

from app.config import get_settings
from app.db import get_db
from app.models import (
    ArtifactResponse,
    EdgeDetailResponse,
    ExpandRequest,
    HealthFileStatus,
    HealthResponse,
    IssuesResponse,
    NodeDetailResponse,
    NodeLinksResponse,
    SearchResponse,
    StatsResponse,
    SubgraphResponse,
    UnresolvedResponse,
)
from app.queries.issues import fetch_issues
from app.queries.node_detail import fetch_edge_detail, fetch_node_detail, fetch_node_links
from app.queries.search import fetch_search
from app.queries.stats import fetch_stats
from app.queries.subgraph import fetch_expand, fetch_subgraph
from app.queries.unresolved import fetch_unresolved, fetch_unresolved_context

router = APIRouter(prefix="/api")


@router.get("/health", response_model=HealthResponse)
def health() -> HealthResponse:
    db = get_db()
    files = [
        HealthFileStatus(path=path, exists=exists)
        for path, exists in db.file_status()
    ]
    duckdb_ok = db.has_view_data("nodes") or db.has_view_data("edges")
    return HealthResponse(
        data_dir=str(db.settings.data_dir),
        cache_dir=str(db.settings.cache_dir),
        duckdb_ok=duckdb_ok,
        files=files,
        warnings=db.warnings,
    )


@router.get("/stats", response_model=StatsResponse)
def stats() -> StatsResponse:
    return fetch_stats(get_db().conn)


@router.get("/issues", response_model=IssuesResponse)
def issues() -> IssuesResponse:
    return fetch_issues(get_db().conn)


@router.get("/search", response_model=SearchResponse)
def search(
    q: str = Query(default=""),
    type: str | None = Query(default=None, alias="type"),
    limit: int = Query(default=20, ge=1, le=100),
) -> SearchResponse:
    return fetch_search(get_db().conn, q, type, limit)


@router.get("/subgraph", response_model=SubgraphResponse)
def subgraph(
    seed_type: str = Query(...),
    seed_id: str = Query(...),
    hops: int = Query(default=1, ge=1, le=2),
    edge_types: str | None = Query(default=None),
) -> SubgraphResponse:
    types = [t.strip() for t in edge_types.split(",") if t.strip()] if edge_types else None
    return fetch_subgraph(get_db().conn, seed_type, seed_id, hops, types)


@router.post("/subgraph/expand", response_model=SubgraphResponse)
def expand_subgraph(body: ExpandRequest) -> SubgraphResponse:
    if not body.node_ids:
        raise HTTPException(status_code=400, detail="node_ids required")
    return fetch_expand(get_db().conn, body)


@router.get("/node/{node_type}/{node_id}", response_model=NodeDetailResponse)
def node_detail(node_type: str, node_id: str) -> NodeDetailResponse:
    return fetch_node_detail(get_db().conn, node_type, node_id)


@router.get("/node/{node_type}/{node_id}/links", response_model=NodeLinksResponse)
def node_links(
    node_type: str,
    node_id: str,
    direction: str = Query(..., pattern="^(in|out)$"),
    edge_type: str | None = None,
    q: str | None = None,
    limit: int = Query(default=30, ge=1, le=200),
    offset: int = Query(default=0, ge=0),
) -> NodeLinksResponse:
    return fetch_node_links(
        get_db().conn, node_type, node_id, direction, edge_type, q, limit, offset
    )


@router.get("/edge", response_model=EdgeDetailResponse)
def edge_detail(
    edge_type: str,
    from_type: str,
    from_id: str,
    to_type: str,
    to_id: str,
) -> EdgeDetailResponse:
    detail = fetch_edge_detail(
        get_db().conn, edge_type, from_type, from_id, to_type, to_id
    )
    if detail is None:
        raise HTTPException(status_code=404, detail="Edge not found")
    return detail


@router.get("/unresolved", response_model=UnresolvedResponse)
def unresolved(
    bucket: str | None = None,
    reason: str | None = None,
    limit: int = Query(default=50, ge=1, le=500),
    offset: int = Query(default=0, ge=0),
) -> UnresolvedResponse:
    return fetch_unresolved(get_db().conn, bucket, reason, limit, offset)


@router.get("/unresolved/{raw_name}/context", response_model=UnresolvedResponse)
def unresolved_context(raw_name: str) -> UnresolvedResponse:
    return fetch_unresolved_context(get_db().conn, raw_name)


@router.get("/artifact/{source_artifact_id}", response_model=ArtifactResponse)
def artifact(source_artifact_id: str) -> ArtifactResponse:
    row = get_db().conn.execute(
        """
        SELECT source_artifact_id, source_url, cache_path, parser_version, scraped_at
        FROM artifacts
        WHERE source_artifact_id = ?
        LIMIT 1
        """,
        [source_artifact_id],
    ).fetchone()
    if not row:
        raise HTTPException(status_code=404, detail="Artifact not found")
    return ArtifactResponse(
        source_artifact_id=row[0],
        source_url=row[1] or "",
        cache_path=row[2] or "",
        parser_version=row[3] or "",
        scraped_at=row[4] or "",
    )


MIME_TYPES = {
    ".html": "text/html",
    ".htm": "text/html",
    ".pdf": "application/pdf",
    ".txt": "text/plain",
}


@router.get("/cache/{path:path}")
def cache_file(path: str) -> FileResponse:
    settings = get_settings()
    cache_root = settings.cache_dir.resolve()
    requested = Path(path)
    if requested.is_absolute() or ".." in requested.parts:
        raise HTTPException(status_code=400, detail="Invalid cache path")

    full_path = (cache_root / requested).resolve()
    if cache_root not in full_path.parents and full_path != cache_root:
        raise HTTPException(status_code=403, detail="Path outside cache root")
    if not full_path.is_file():
        raise HTTPException(status_code=404, detail="Cache file not found")

    media_type = MIME_TYPES.get(full_path.suffix.lower(), "application/octet-stream")
    return FileResponse(full_path, media_type=media_type)
