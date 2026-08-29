# Upstream merge plan (PRs A–E)

Handoff for an agent or reviewer splitting `thepycoder/core` (`vote-rewrite-for-blocks`) into mergeable PRs against `partijgedrag/core`.

Read first, in order:

1. [`AGENTS.md`](../AGENTS.md) — pipeline invariants
2. [`fork-review-mental-model.md`](fork-review-mental-model.md) — what the fork *is*
3. This file — how to land it
4. [`branch-maintainability-review.md`](branch-maintainability-review.md) — cleanup that must not be forgotten inside each PR

Do **not** open one PR with the whole branch. The upstream maintainer wants to keep a mental model of the code and agreed that the graph is the way forward **if** flat staging Parquet stays a publishable dataset.

## Snapshot (refresh before starting)

Recorded 2026-08-29. Fetch and overwrite these SHAs before you cut branches.

| Ref | SHA |
|---|---|
| Fork HEAD (`vote-rewrite-for-blocks`) | `3e100236cc83884ba5e3b229793bf9cf587167c4` |
| Merge base with `upstream/main` | `29a2dbe16c9551ff3df8b5b297e364d5a3865a55` |
| `upstream/main` | `56cc537eba89d819fed02687fd2f5ce3db846e55` |
| Ahead / behind | 63 / 11 |
| Whole-branch diff | 239 files, +56 754 / −2 178 |

```sh
git fetch upstream main
git rev-parse HEAD upstream/main
git merge-base HEAD upstream/main
git rev-list --left-right --count upstream/main...HEAD
```

## Target end state (maintainer + fork)

```
Rust scrapers  →  staging Parquet          (canonical, publishable, “vlak”)
               →  identity / normalize / graph Parquet   (rebuildable)
               →  just qa                  (parser honesty vs HTML + graph FKs)

staging Parquet → open datasets
graph Parquet   → partijgedrag.be / parlemento / a dry search frontend
```

`tools/graph-viewer` is a local debug UI. It does **not** land in core as a product. Core stays scrape + parse + Parquet (+ optional graph/QA in Rust).

## Constraints (do not violate)

- Staging scrapers still write Parquet. Graph is **derived**, never a replacement.
- Parse + Parquet generation stays **Rust**. Python is allowed for PDF→markdown (PyMuPDF), and for tools that leave core in PR D.
- Graph output is also Parquet: `data/graph/{nodes,edges,source_artifacts}.parquet` with typed `from_id` / `to_id` keys. No engine-specific DB as source of truth.
- Every person link goes through `Resolver` / `ActorResolver`. `Person` (MPs) ≠ `ExternalPerson`.
- Every normalized edge carries `source_url`, `cache_path`, `source_artifact_id`, `confidence`.
- Prefer site-native ids. No backwards-compat shims; re-scrape after schema changes.
- Meeting reports are handwritten — broad parsers, central exception catalogs, committed fixtures.
- Do not solve merge conflicts with blanket ours/theirs. Both sides have data-model work.
- Do not use this split as an excuse for long-lived duplicate branches. Land A, then B on top of A, etc.
- Do not commit `cache/`, generated scrape output, credentials, or absolute local paths.

## Work order

```
P0  integrate upstream/main
A   staging contract + crawl parsers + meeting binaries + QRVA
B   identity + normalize + graph
C   QA crate + committed fixtures  (then align with maintainer TOML validator)
D   remove graph-viewer / qa-triage from core  (or never merge them)
E   optional LLM enrichers + generated docs
```

P0 is not optional. Refactoring pre-merge copies of plenary/dossiers/members wastes work and can drop upstream features.

Inside each PR, keep mechanical moves and behaviour changes in separate commits.

---

## P0 — Integrate `upstream/main`

**Why first:** 11 upstream commits collide with this branch. Upstream-only behaviour must survive.

**Upstream-only commits to preserve** (`git log HEAD..upstream/main`):

- member start/end dates; scrape all members
- question dates; questionee ≠ respondent
- dossier original document as subdocument; `VoorstelVanNaturalisatieAkte`; hopping fix
- dossier title/description summaries; pdf-to-md cache/data dirs

**Conflict files** (merge-tree at the snapshot):

`.gitignore`, `README.md`, `scrapers/plenary-meetings/src/main.rs`, `scrapers/commission-meetings/src/main.rs`, `scrapers/dossiers/src/main.rs`, `scrapers/members/src/main.rs`, `summarizers/dossier-summarizer/src/main.rs`, `summarizers/text-summarizer/src/main.rs`.

**Procedure:** three-way each family; write down retained behaviour from each side; add a regression test per kept upstream feature; run targeted tests after each family.

**Done when:** no conflict markers; listed upstream features present and tested; fork vote/block fixture tests still pass; `git diff --check` clean; this file updated with the new merge-base SHA.

**Out of scope:** vote-assembler rewrite, graph-viewer, QA catalog unification.

---

## PR A — Staging + parsers (no graph required)

**Goal:** richer, provenanced **flat** Parquet. A reviewer who only cares about the publishable dataset can stop here.

**In scope**

- `STAGING.md` as the publish contract (ID conventions, every current table)
- `scrapers/crawl` meeting-report stack: `meeting_parse.rs`, report blocks, agenda, utterances, hearings/interpellations, vote assembly + `tests/fixtures/votes/`
- `scrapers/plenary-meetings`, `scrapers/commission-meetings` as download/orchestration wrappers
- `scrapers/qrva` (written Q&A)
- Provenance columns (`source_url`, `cache_path`) on existing scrapers (sessions, members, commissions, dossiers, lobby, remunerations)
- Corpus / vote policy docs: `docs/meeting-report-corpus-policy.md`, `docs/meeting-report-vote-taxonomy.md`
- `justfile` scrape / `reparse` / `SCRAPER_CACHE_ONLY` targets that do **not** require identity/graph
- Workspace member: `scrapers/crawl` (already present on upstream as a non-member helper crate — make it a real member)

**Out of scope (do not put in A)**

- `scrapers/identity`, `scrapers/normalize`, `scrapers/graph`
- `scrapers/qa`, `tools/graph-viewer`, `tools/qa-triage`
- `DATA_GRAPH.md` as a required review surface (a short pointer is fine)
- LLM summarizers beyond resolving the P0 conflict

**Known smells to acknowledge, not necessarily fix in A**

- Plenary vs commission `main.rs` duplication (gap / cache-miss policy already diverged). Characterize in comments or a follow-up issue; full `MeetingIngestionRunner` extract is maintainability T3.
- `vote_assembly.rs` is a long implicit state machine. Review via taxonomy + fixtures. Do not rewrite it in the same PR as the first landing.

**Acceptance**

- `STAGING.md` matches produced columns on a cache-backed reparse of committed fixtures
- Vote fixtures under `scrapers/crawl/tests/fixtures/votes/` run without `cache/`
- Missing committed fixtures **fail** (no `if !exists { continue }`)
- Plenary and commission binaries still write the same staging tables plus new ones documented in `STAGING.md`
- No identity/graph types leak into scraper public output

**Reviewer ask:** “Can I publish these Parquet files as a flat research dataset?”

**Handoff to B:** staging paths and ID conventions are frozen enough that identity can key off them.

---

## PR B — Identity → normalize → graph

**Goal:** rebuildable graph Parquet on top of A. This is the architecture the maintainer called “the way forward”.

**In scope**

- `scrapers/identity` — `Resolver`, `ActorResolver`, `typo_corrections()`, ExternalPerson
- `scrapers/normalize` — one module per edge family; all person links through the resolvers
- `scrapers/graph` — `data/graph/nodes.parquet`, `edges.parquet`, `source_artifacts.parquet`
- `DATA_GRAPH.md` (nodes/edges/coverage). HTML/canvas catalogs may wait for E
- `just build-identity`, `just normalize-edges`, `just build-graph`

**Out of scope**

- QA binary (C)
- graph-viewer (D)
- LLM bios (E)
- New scrape sources that are not needed for the current graph edges

**Documented gaps (do not invent them as blockers)**

Motion node, `DISCUSSED_IN`, commission roll-calls (not in integraal HTML), `DECLARES_INTEREST`, government mandate-over-time, vote ids still generated composites, session hardcoded `56`.

**Acceptance**

- Person-bearing edges (`SPOKE`, `CAST`, `ASKED`, `AUTHORED`, …) only exist after a resolver call
- `Person` never used for ministers/chairs/experts; those are `ExternalPerson`
- Every normalized edge has provenance + numeric `confidence`
- Graph builder is deterministic from identity + staging + normalized inputs
- Sample `nodes.parquet` / `edges.parquet` can be queried with DuckDB; no `.duckdb` committed as canonical

**Reviewer ask:** walk `DATA_GRAPH.md` edges and tick emit vs documented gap.

**Handoff to C:** graph + staging exist so checks have something honest to compare.

---

## PR C — QA in Rust

**Goal:** parser-honesty and graph integrity in core. Not a replacement for the maintainer’s declarative TOML validator in the private data repo.

**In scope**

- `scrapers/qa` + `just qa` / `qa-strict` / `qa-update-baseline`
- Checks that **re-parse cached HTML** or walk graph FKs / unresolved persons / source spans
- Committed fixtures so the suite cannot silently execute zero cases on a clean clone
- `just check` (fmt, clippy, Rust tests, `git diff --check`) if it does not exist yet

**Out of scope**

- Reimplementing column-null / type / “ja-totaal = aantal ja-namen” rules that belong in the data-repo TOML
- `tools/qa-triage` (D or a later tools repo)
- graph-viewer issues panel

**Overlap with the data-repo validator (do not double-own)**

This check is the same invariant as a typical TOML cross-column vote rule:

`vote.compact_total_vs_member_names` — headline yes/no/abstain vs named appendix members.

**Keep in Rust** only what needs HTML re-parse or graph structure (spans stale after parser change, `graph.edge_endpoints_exist`, speaker resolution, vote *method* invariants). **Prefer TOML** for published-dataset contracts (types, unique ids, simple sums). Note the split in the PR description so the maintainer can open-source the data-repo validator without fighting core.

**Acceptance**

- `just qa` writes `data/qa/checks.parquet` (and documented sidecars) from detail rows
- Deleting ignored `cache/` does not reduce deterministic assertion count
- A missing committed fixture fails with a path
- No developer-absolute paths in tests
- Catalog drift: if both Rust `check_catalog.rs` and a Python copy exist, do not add a third copy. Unifying catalogs is maintainability T6 — only do it here if cheap

**Reviewer ask:** pick five fail/warn checks, read one detail row, open the cited HTML.

**Handoff to D:** QA is the backend; no UI required in core.

---

## PR D — Views leave core

**Goal:** core = scrape + parse + Parquet (+ graph/QA). Frontends are views.

**In scope (one of these, pick explicitly in the PR)**

1. **Do not merge** `tools/graph-viewer` or `tools/qa-triage` into `partijgedrag/core` at all (preferred if the maintainer already has a dry search frontend), **or**
2. Move them to a separate repo / the dry-frontend repo in the same change that deletes them from core.

graph-viewer is FastAPI + DuckDB over `data/graph` + `report_blocks` / `source_spans`. Useful internally (block overlay, vote inspector, QA deep-link). Not the journalist-facing search UI.

**Out of scope**

- Building the maintainer’s dry frontend
- Obsidian-style graph visualisation
- Changing partijgedrag.be

**Acceptance**

- `partijgedrag/core` workspace has no Python web app
- README / `justfile` no longer present graph-viewer as a core product
- If the tool still exists elsewhere, it consumes Parquet only (no scrape)

**Handoff to E:** optional.

---

## PR E — Optional LLM + generated catalogs

**Goal:** enrichment after the graph exists. Easy to skip.

**In scope if landed**

- `summarizers/mistral-client` adopted by dossier/text summarizers (delete duplicate rate limiters — maintainability T8)
- `summarizers/external-person-enricher` (`just enrich-external-persons`)
- Single catalog → `DATA_GRAPH.md` + `docs/data-graph-overview.html` + canvas (T9), **or** drop the HTML/canvas copies and keep Markdown only

**Out of scope:** new model providers, GraphRAG indexes.

**Acceptance:** summarizers call the shared client; no live network in tests; generated docs either gone or stale-checked.

---

## Suggested git mechanics

Prefer stacked PRs on top of an integration branch, not three long-lived forks of the same files.

```text
upstream/main
    └── p0-integrate-upstream
          └── pr-a-staging-parsers
                └── pr-b-identity-graph
                      └── pr-c-qa
                            └── (no D if viewer never added)
                            └── pr-e-optional
```

If the current branch must stay intact until P0: integrate on a new branch from `vote-rewrite-for-blocks`, then carve A by reverting or not committing B–E paths. Do **not** rewrite published history unless the repo owner asks.

Each PR description should state:

- which pipeline stage (scrape → identity → normalize → graph → qa)
- what Parquet is in/out
- what the next PR needs from this one

## Quality gate (add in A or C, keep forever)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --workspace --all-targets
git diff --check
```

Python tests only while those tools still live in this repo. After D they move with the tool.

## Do not do

- One 239-file PR “because the graph needs everything”
- Review or rewrite `vote_assembly.rs` as the first landing change
- Hardcode more `sessions/56` paths
- Add `ensure_*` dual paths for old staging formats
- Treat a green `cargo test` on a developer machine with 500+ cached meetings as proof
- Merge graph-viewer into core “temporarily”
- Duplicate vote-total rules in Rust and TOML without naming a single owner

## When handing off again

Update the snapshot table, checkboxes below, and any SHA that moved. Do not mark a PR done from file moves alone — run its acceptance commands and record them.

- [ ] P0 upstream integrated
- [ ] PR A opened / merged
- [ ] PR B opened / merged
- [ ] PR C opened / merged
- [ ] PR D decided (never merge vs move out)
- [ ] PR E opened / skipped
