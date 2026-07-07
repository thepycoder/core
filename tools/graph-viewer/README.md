# Graph debug explorer

Local-only debug UI for inspecting `data/graph/*.parquet`. This directory is **gitignored** and not part of the published scraper pipeline.

## Prerequisites

Build graph data first (from the `core` repo root):

```bash
just build-identity
just normalize-edges
just enrich-external-persons   # optional: MISTRAL_API_TOKEN
just build-graph
```

`build-identity` writes Chamber MP tables and bootstraps `ExternalPerson` from all staging name fields (Q&A, dossier authors, org/role seeds). `normalize-edges` links staging rows to those identity tables.

## Environment

Reads `SCRAPER_DATA_DIR` and `SCRAPER_CACHE_DIR` from `core/.env` (defaults: `./data` and `./cache` relative to the core repo root).

## Run

```bash
cd tools/graph-viewer
uv sync
uv run uvicorn app.main:app --reload --port 8765
```

Open http://127.0.0.1:8765

## Features

- Search entities by name, id, title, or body text (utterances, dossiers, documents)
- Filter by `Person` (MPs only) or `ExternalPerson` (ministers, roles, org authors)
- ExternalPerson inspector with LLM bio when `enrich-external-persons` has run
- Inspector preview panel with metadata, excerpts, and source links per entity type
- Filterable paginated link lists (search vote titles, question topics, etc.)
- Vote breakdown with yes/no/abstain member lists (resolved persons are clickable)
- Open source pages on dekamer.be and cached HTML/PDF per entity and per edge
- Question discussion text, vote reconciliation totals, data quality issues

## API

All endpoints are under `/api/` — see `app/routes/api.py` for the full list.
