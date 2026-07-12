# Agent rules

Belgian Chamber (dekamer.be) data pipeline. Full model: `DATA_GRAPH.md`. Staging schemas: `STAGING.md`. Commands: `justfile`.

## Python

Use **uv** (never global `pip install` or system Python for project code).

- `uv sync` then `uv run …` for tools/tests
- `uv add …` / `uv add --dev …` for deps
- `uvx …` for one-off CLIs

## Data stack

- **Parquet is canonical** — source of truth under `data/`; optimize for longevity and queryability.
- **Query engines are derived** — `.duckdb`, `.kuzu`, etc. are rebuildable artifacts only, never canonical.
- **DuckDB** — current `web` build-time consumer; swappable.
- **Embeddings / GraphRAG** — add as derived indexes rebuilt from Parquet, keyed to graph node ids.

## Architecture

- Staging scrapers → identity → normalize → graph. Do not replace raw/staging outputs.
- Route all person links through `resolve_person` / `person_alias` before `SPOKE`, `CAST`, `AUTHORED`, etc.
- **ExternalPerson** (ministers, chairs, experts) is separate from **Person** (MPs).
- Every normalized edge carries `source_url`, `cache_path`, `source_artifact_id`, and `confidence`.
- Prefer **site-native ids** (cvview key, FLWB doc id, dossier `56/N`) over generated hashes.

## Pipeline order

```
just build-identity → just normalize-edges → just build-graph
```

Optional LLM enrichment (`just enrich-external-persons`, summarizers) runs after core graph; Mistral token in `.env`.

## Source data reality

Meeting reports are **handwritten by note-takers** — HTML structure, names, and labels are unreliable. Mistakes are normal.

- Prefer **broad parsers** over narrow regexes; validate fixes against **multiple meetings/reports**, not one failing page.
- When edge cases are unavoidable (specific regexes, name substitutions), **catalog them centrally** — e.g. `typo_corrections()` in `scrapers/identity/src/normalize.rs` — not scattered inline. Keep exceptions visible, named, and easy to extend.
- **Add tests for every edge case you find or fix** — minimal HTML/name snippets in unit tests. Over time this locks in known quirks and prevents fixes from breaking each other.

## Explaining changes

Assume the reader lacks a mental model of this codebase. Before code-level detail, give **brief context**: which pipeline stage (scrape → identity → normalize → graph), which data flows in/out, and why the change matters.

## Refactors and breaking changes

**Rework is cheap here.** Prefer rewriting a whole module over layering cheap fixes on bad structure. Do not preserve awkward code to avoid a larger diff.

**No backwards compatibility.** Cached HTML/PDF and staging Parquet are disposable — re-scrape and re-parse after breaking schema or parser changes. Do not add migration shims, dual code paths, or `ensure_*` upgrade logic for old formats unless explicitly requested.

## When changing code

- Match existing scraper pattern: fetch → parse → write Parquet; cache HTML/PDF under `SCRAPER_CACHE_DIR`.
- New staging columns/schemas: update `STAGING.md`.
- New nodes/edges: update `DATA_GRAPH.md` and wire through identity + graph builder.
- **Data graph canvas:** keep `canvases/data-graph-overview.canvas.tsx` in sync with `DATA_GRAPH.md` whenever you add or change node types, edge types, implementation status, coverage counts, or pipeline stages. Update the inline `NODES` and `EDGES` catalogs (labels, domains, status, id keys, notes) and any summary stats shown in the canvas (built node/edge counts, coverage bar). The repo copy is canonical; if you use the live Cursor canvas beside chat, sync the same file there too.
- Minimize scope for small fixes; for structural problems, refactor properly instead of patching around them.
- When adding exceptions or regexes or other case-specific logic, always add and example reference to a document in comments
