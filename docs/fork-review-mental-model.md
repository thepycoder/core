# Fork review mental model

Reviewer-facing map of `thepycoder/core` (`vote-rewrite-for-blocks`) versus `partijgedrag/core` (`upstream/main`). Companion cleanup list: [`branch-maintainability-review.md`](branch-maintainability-review.md).

Snapshot (2026-07-19):

| | SHA / count |
|---|---|
| Branch HEAD | `3e10023` |
| Merge base | `29a2dbe` (also current `origin/main`) |
| `upstream/main` | `56cc537` |
| Ahead / behind | **63** / **11** |
| Diff | **239 files**, **+56 754 / −2 178** (213 new files) |

Do not review this as one pull request of 239 files. Review it as a **new data pipeline** sitting on top of the old scrapers.

## One-sentence model

Upstream scrapers still fetch dekamer.be and write **staging Parquet**. This fork adds **identity → normalize → graph**, then **QA** that checks the graph against the cached HTML. The graph is the product; the scrapers are the intake.

```
dekamer.be HTML/PDF
        │  cache under SCRAPER_CACHE_DIR
        ▼
 staging Parquet          STAGING.md          (source of truth on disk)
        │
        ├─► identity      persons, aliases, ExternalPerson
        │         │
        │         ▼
        ├─► normalize     typed edges with provenance + confidence
        │         │
        │         ▼
        └─► graph         nodes.parquet + edges.parquet
                  │
                  ├─► just qa          checks.parquet
                  └─► graph-viewer     local debug UI (not the public app)
```

`web` consumes graph Parquet at build time. It is out of scope for this core PR.

## What upstream is

A Cargo workspace of **independent scraper binaries** plus two Mistral summarizers.

Each scraper: fetch (or read `cache/`) → parse HTML/PDF → write a flat `.parquet` file. There is no identity layer, no graph, no `DATA_GRAPH.md`, no `just qa`. Shared HTTP/path helpers live in `scrapers/crawl` (client, paths, utils) but crawl is **not** a workspace member on upstream.

## What this fork adds

Four new pipeline crates, one new source scraper, and two local tools:

| Layer | Crate / tool | Job |
|---|---|---|
| Shared parsers | `scrapers/crawl` | Meeting-report block stream, votes, utterances, agenda, provenance helpers. Plenary and commission binaries call into it. |
| New source | `scrapers/qrva` | Written questions/answers from the QRVA API. |
| Step 1 | `scrapers/identity` | Canonical `Person` (MPs) and `ExternalPerson` (ministers, chairs, experts). One resolver. |
| Step 2 | `scrapers/normalize` | Person-bearing edges (`CAST`, `ASKED`, `SPOKE`, …) through that resolver. |
| Step 3 | `scrapers/graph` | Deterministic `data/graph/{nodes,edges,source_artifacts}.parquet`. |
| Step 5 | `scrapers/qa` | ~78 checks over staging + graph vs cached HTML. |
| Debug | `tools/graph-viewer` | FastAPI + DuckDB inspector. **Local only.** |
| Triage | `tools/qa-triage` | Cluster QA fails for fix-agent briefs. |
| Optional LLM | `external-person-enricher`, `mistral-client` | Bios after the graph exists. |

Existing scrapers were rewritten in place (especially plenary/commission meetings, dossiers, lobby-as-PDF, remunerations) so they emit `source_url` + `cache_path` and use the crawl parsers.

## Invariants (review against these, not commit messages)

The 63 commit subjects are mostly “Enhance …”. Ignore them. These rules are the actual design:

1. **Parquet is canonical.** DuckDB, Kuzu, and the graph-viewer session are rebuildable.
2. **Staging is not replaced.** Identity/normalize/graph read staging; they do not become the new scrape output.
3. **Every person link goes through `Resolver` / `ActorResolver`.** No local name-matching in graph or QA.
4. **`Person` ≠ `ExternalPerson`.** MPs are cvview-backed. Ministers, chairs, and experts are a separate node type.
5. **Every normalized edge carries provenance:** `source_url`, `cache_path`, `source_artifact_id`, `confidence`.
6. **Prefer site-native ids** (cvview key, FLWB `56K1280004`, dossier `56/N`) over hashes. Vote ids are still generated composites — known gap.
7. **Meeting reports are handwritten.** Broad parsers + central exception catalogs (`typo_corrections()`, vote fixtures). Do not expect clean HTML.
8. **No backwards compatibility.** Re-scrape after schema/parser changes. Dual paths and `ensure_*` shims are a smell unless documented.

Spec docs: [`DATA_GRAPH.md`](../DATA_GRAPH.md), [`STAGING.md`](../STAGING.md), [`AGENTS.md`](../AGENTS.md).

## What to look at first

Read in this order. Stop after each step and decide “accept / discuss / reject the model” before opening more files.

### 1. Spec, not code (30–45 min)

- [`DATA_GRAPH.md`](../DATA_GRAPH.md) — nodes, edges, what is built vs not.
- [`STAGING.md`](../STAGING.md) — ID conventions and table schemas.
- [`AGENTS.md`](../AGENTS.md) — pipeline order and parser philosophy.
- [`docs/meeting-report-corpus-policy.md`](meeting-report-corpus-policy.md) — what is *deliberately* not extracted as speech.
- [`docs/meeting-report-vote-taxonomy.md`](meeting-report-vote-taxonomy.md) — vote shapes and fixtures.

Accept the model here or the rest of the review is wasted.

### 2. Identity (the architectural load-bearing wall)

- `scrapers/identity/src/resolver.rs` — MP lookup, aliases, ambiguity.
- `scrapers/identity/src/actor_resolver.rs` — Person vs ExternalPerson.
- `scrapers/identity/src/normalize.rs` — `typo_corrections()` and name cleanup.

Ask: can every later edge (`SPOKE`, `CAST`, `ASKED`, `AUTHORED`) only exist after this crate? If someone added a shortcut matcher in graph or a scraper, that is a defect.

### 3. Meeting parse (the largest correctness risk)

Start at `scrapers/crawl/src/meeting_parse.rs` (`parse_plenary_meeting_report` / `parse_commission_meeting_report`). That function is the orchestrator:

HTML → report blocks → agenda timeline → utterances / hearings / interpellations / votes → source spans.

Then sample, do not linear-read:

- `report_blocks.rs` — the typed HTML stream everything else consumes.
- `vote_assembly.rs` — implicit state machine (~1 470 lines). Review via [`meeting-report-vote-taxonomy.md`](meeting-report-vote-taxonomy.md) and `scrapers/crawl/tests/fixtures/votes/`, not by tracing the loop.
- `agenda_timeline.rs`, `utterance_segment.rs`, `proceeding_entities.rs`.

Plenary and commission `main.rs` are download/orchestration wrappers. They currently **duplicate** a lot of that orchestration. Treat duplication as a follow-up (see maintainability review T3), not as a reason to reject the parsers.

### 4. Normalize + graph (is the spec implemented?)

- `scrapers/normalize/src/lib.rs` — one module per edge family.
- `scrapers/graph/src/build.rs` — `NodeRow` / `EdgeRow` / `ArtifactRow`. Confirm provenance columns and that person ids only come from identity.

Walk `DATA_GRAPH.md` edges and tick whether `normalize` + `graph` actually emit them. Gaps called out in the spec (Motion node, `DISCUSSED_IN`, commission roll-calls, `DECLARES_INTEREST`) are expected.

### 5. QA as the test suite you did not know you had

`scrapers/qa/src/lib.rs` `registered_check_ids()` is the catalog (~78 checks). Vote, span, graph FK, speaker, and schema checks are the ones that protect the model. Graph-viewer is how a human *sees* a failing check on the source HTML. Neither is the data product.

### 6. Upstream-only work (must not be dropped)

Eleven commits on `upstream/main` since the merge base. They collide with this branch in:

`.gitignore`, `README.md`, `plenary-meetings`, `commission-meetings`, `dossiers`, `members`, both summarizers.

Preserve at least: member start/end dates and full member scrape, question date/respondent vs questionee, dossier original document + naturalisatie type, dossier hopping fix, summarizer cache-dir handling. Integrate **before** landing, not after a “clean” refactor of the pre-merge copies.

## Strategy to keep the review under control

**Split by layer, not by file.** Execution plan for stacked PRs (P0 then A–E), including maintainer constraints and acceptance criteria: [`upstream-pr-split.md`](upstream-pr-split.md).

A reasonable conceptual PR sequence (even if it stays one branch until cleaned up):

| PR | Scope | ~diff weight | Merge bar |
|---|---|---|---|
| A | Staging contract + crawl parsers + meeting binaries + QRVA | crawl 12k + scraper rewrites | Fixture tests; `STAGING.md` matches output |
| B | Identity + normalize + graph | ~10k | Person links only via resolver; provenance columns present |
| C | QA crate + committed fixtures | qa 9k | Checks run on a **clean checkout** (no local `cache/`) |
| D | graph-viewer + qa-triage | viewer 13k | Optional / later; local debug |
| E | LLM enrichers + generated docs HTML/canvas | small | Optional |

**Review ~25k of the 57k first.** graph-viewer is the single largest insertion block and the least important for accepting the architecture.

**Treat commit history as noise.** Three weeks, 63 commits, several merge-from-self. Review crate boundaries and the spec, then sample implementations.

**Do not merge as-is.** The maintainability review’s P0s are still the landing blockers: integrate upstream, make tests fail when fixtures are missing, add a real `just check` gate. Tests that `continue` when `cache/` is absent will pass on a clean clone without proving anything.

**When a file is huge, ask “what fixture covers this?”** Vote assembly, agenda, and utterance parsers are justified by committed HTML snippets plus QA checks. A 1 470-line function without a fixture for a claimed behaviour is the thing to push back on — not the existence of a long parser for dirty parliamentary HTML.

## Hard spots (budget extra time)

| Spot | Why it is hard | How to review it |
|---|---|---|
| `vote_assembly.rs` | Implicit state machine; many vote *methods* (roll-call, secret, sitting/standing, no-quorum, language-group, result reuse) | Taxonomy table + fixtures; confirm exclusions (quoted tables, unanimity prose) |
| Plenary vs commission `main.rs` | Duplicated ingestion; already diverged (gap/cache-miss policy) | Characterize differences, do not assume they are bugs |
| `ActorResolver` vs `Resolver` | Two resolvers, easy to wire the wrong one | MP-only edges must reject ExternalPerson (`graph.external_on_mp_only_edges`) |
| Question ids | Composite `{session}_{kind}_{meeting}_{seq}` plus site refs in `internal_ids` | Collision plenary/commission was a real bug; `ensure_question_id` is a staging-upgrade shim — flag if it becomes permanent |
| Session hardcoded `56` | Graph builder and graph-viewer still assume session 56 | Accept for this legislature; do not expand the hardcoded surface |

## What to skip on the first pass

- `tools/graph-viewer/**` except a 10-minute click-through after `just qa` (report overlay, vote inspector, issue deep-link).
- `docs/data-graph-overview.html` and `canvases/data-graph-overview.canvas.tsx` — generated-ish catalogs that can drift from `DATA_GRAPH.md`.
- `summarizers/**` except the upstream conflict (title summaries, cache dirs).
- Clippy argument-count warnings, duplicated coverage JS, formatting — real, but they are cleanup, not model review.
- Plan markdowns under `scrapers/*-plan.md` — useful archaeology, not the contract.

## Suggested review session plan

1. **Session 1 (spec):** this file + `DATA_GRAPH.md` + `STAGING.md`. Outcome: written list of “I buy / I question” model decisions.
2. **Session 2 (identity + one meeting):** resolver + `meeting_parse` + one plenary fixture (e.g. meeting 60 roll-call) and one commission hearing/interpellation path.
3. **Session 3 (graph contract):** normalize modules ↔ `DATA_GRAPH` edges; sample `nodes.parquet` / `edges.parquet` if a local build exists.
4. **Session 4 (QA honesty):** pick five `fail`/`warn` checks, read the check + one detail row, open the cited HTML in graph-viewer or the cache.
5. **Session 5 (upstream merge):** three-way the eight conflict files; write down retained behaviour from each side.

After session 1 you should know whether this should land as stacked PRs or stay a fork until the P0 cleanup in the maintainability review is done.
