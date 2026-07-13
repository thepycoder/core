# Graph debug explorer

Local debug UI for inspecting `data/graph/*.parquet`, derived report blocks, and source-span provenance. This directory is **tracked** in the core repo (not part of the published site).

## Prerequisites

Build graph data first (from the `core` repo root):

```bash
just scrape-plenary-meetings   # writes votes, report_blocks, source_spans
just build-identity
just normalize-edges
just enrich-external-persons   # optional: MISTRAL_API_TOKEN
just build-graph
just qa
```

## Run

```bash
cd tools/graph-viewer
uv sync
uv run uvicorn app.main:app --reload --port 8765
```

Open http://127.0.0.1:8765

## Features

- Search entities, inspect links, vote breakdown (`Vote` → `result_id` → tallies / casts / reconciliation)
- **Report coverage** mode with session + meeting kind:
  - List meetings: `GET /api/reports/meetings?session_id=56&meeting_kind=plenary`
  - Load report: `GET /api/reports/{session_id}/{meeting_kind}/{meeting_id}`
  - Deep link: `?report={meeting_id}&session=56&kind=plenary&block={index}`
- Overlay filters on report API: `entity_type`, `coverage_kind` (`extraction`|`scope`), `span_role`
- Block inspection: structured vote tables, `has_oraspr`, parser/extractor versions, per-block span badges
- Span validation diagnostics: `wrong_artifact`, `stale_source_content`, `stale_block_parser`, `out_of_bounds`, `invalid_half_open_range`
- Bidirectional navigation: span badge → graph node; node detail → “Open in report” when spans exist
- QA issues with `source_block` deep-link into report mode
- URL-backed navigation (`?type=…&id=…`) with browser history preservation
- Artifact metadata: `GET /api/artifacts/{source_artifact_id}` (`source_content_hash`, versions, cache path)

## Troubleshooting

- **Empty report panel:** run `just scrape-plenary-meetings` to materialize `data/derived/sessions/{session}/plenary/report_blocks.parquet` and `source_spans.parquet`
- **Vote inspector schema errors:** rebuild normalize output (`just normalize-edges`) after vote schema changes; viewer expects typed `confidence` (FLOAT64) and `result_id` on casts
- **Stale span diagnostics:** re-scrape meeting HTML so `source_content_hash` matches spans; re-run plenary scraper after `block_parser_version` bumps
- **Missing `vote_result_members`:** only named roll-call results populate member rows; secret/sitting-standing/quorum results show tallies only
- **DuckDB health warning:** graph parquet under `data/graph/` must exist (`just build-graph`); derived report files are optional for entity search but required for report mode
