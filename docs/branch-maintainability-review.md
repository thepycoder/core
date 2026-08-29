# Branch maintainability review and handoff

Review date: 2026-07-19  
Repository: `core`  
Reviewed branch: `vote-rewrite-for-blocks`  
Reviewed HEAD: `3e100236cc83884ba5e3b229793bf9cf587167c4`  
Upstream reference: `upstream/main` at `56cc537eba89d819fed02687fd2f5ce3db846e55`  
Merge base: `29a2dbe16c9551ff3df8b5b297e364d5a3865a55`

## Purpose

This document is a handoff for finishing the branch without allowing its large,
LLM-assisted changeset to become a permanent maintenance burden. It focuses on
duplication, accidental complexity, weak test guarantees, multiple sources of
truth, and code that is difficult for a human to review or safely modify.

The review compares the branch to the merge base with `upstream/main`, not just
to the branch's previous commit. At the review snapshot, the branch was 63
commits ahead of and 11 commits behind `upstream/main`. The merge-base diff
contained 239 changed files, 56,754 insertions, and 2,178 deletions.

## Executive decision

**Do not merge the branch as-is.**

The branch contains useful architecture and working behavior, and its test
suites pass in the current developer checkout. However, the change is too large
to review confidently in one unit, it is already behind upstream work in the
same areas, several tests silently skip their assertions on a clean checkout,
and important paths have acquired parallel implementations that are already
diverging.

The branch should be made mergeable through a sequence of narrow cleanup
commits. The immediate blockers are:

1. integrate the current `upstream/main` carefully;
2. make tests independent of ignored local cache data;
3. establish formatting, lint, and diff checks as a single repeatable gate;
4. consolidate the duplicated meeting ingestion runners;
5. break the vote assembler's implicit state machine into explicit state and
   small event handlers.

The remaining items are important maintainability work, but can follow in
separate commits after the blockers are addressed.

## Instructions for the next agent

### Before changing code

- Read [`../AGENTS.md`](../AGENTS.md) in full. Its data-pipeline and fixture
  rules are part of the acceptance criteria for every task below.
- Treat the SHAs above as a historical review snapshot. Fetch and record the
  latest upstream state before starting:

  ```sh
  git fetch upstream main
  git status --short --branch
  git rev-parse HEAD upstream/main
  git merge-base HEAD upstream/main
  git rev-list --left-right --count upstream/main...HEAD
  ```

- Confirm the worktree is clean or identify which existing changes belong to
  the user. Do not discard or overwrite unrelated work.
- Work through the tasks in dependency order. Use one focused commit per task,
  or split a task further when the diff is still difficult to review.
- Add characterization tests before changing behavior-heavy code. A refactor
  should not alter produced Parquet schemas, graph edge provenance, ordering,
  or normalization semantics unless that change is explicitly documented.
- Do not solve conflicts with blanket `ours` or `theirs` choices. Both sides
  contain data-model work that must survive integration.
- Do not commit caches, generated scrape output, credentials, or local absolute
  paths. Commit only deliberately minimized test fixtures.
- Do not add backwards-compatibility shims. The repository guidance explicitly
  permits clean module rewrites and coordinated caller updates.

### Repository invariants to preserve

- Parquet remains the canonical data interface between stages.
- Pipeline ordering remains explicit: scrape/crawl, normalization, identity
  resolution, and graph construction must not become implicitly coupled.
- Person links must continue through the central resolver. Do not add local
  identity matching shortcuts.
- Graph edges must preserve their provenance columns and remain auditable.
- New or changed staging tables must be reflected in the data graph
  documentation.
- Broad real-world parsers should remain tolerant, but exceptions and mappings
  must be centralized rather than distributed through callers.
- Edge cases should be represented by small committed fixtures, not by tests
  that depend on a developer's ignored cache.

## Prioritized task list

### P0 — Integrate current upstream without losing either data model

- [ ] **T0.1 Fetch and integrate the latest `upstream/main`.**
- [ ] **T0.2 Resolve every conflict semantically and add regression coverage for
      behavior retained from both sides.**
- [ ] **T0.3 Recompute this branch's diff and update this report if upstream has
      already resolved or materially changed a finding.**

Why this comes first:

The reviewed branch was 11 commits behind upstream. A merge-tree inspection at
the review snapshot found conflicts in `.gitignore`, the README, commission and
plenary scrapers, dossier and member scrapers, the dossier PDF script, and both
dossier/text summarizers. Upstream also contained work absent from this branch,
including member history, question dates/respondents, and dossier original
document/type handling. Refactoring the pre-integration copies first is likely
to waste effort or accidentally erase upstream features.

Detailed procedure:

1. Inspect upstream-only commits and the three-way diff before integrating:

   ```sh
   git log --oneline HEAD..upstream/main
   git diff --stat HEAD...upstream/main
   git diff HEAD...upstream/main -- scrapers summarizers
   git merge-tree "$(git merge-base HEAD upstream/main)" HEAD upstream/main
   ```

2. Prefer a merge or rebase strategy consistent with the repository owner's
   workflow. If that policy is not discoverable locally, stop and ask before
   rewriting published history.
3. For each conflict, write down the behavior supplied by each side before
   editing. Preserve both unless they are truly mutually exclusive.
4. Pay special attention to member history, question metadata, dossier original
   documents/types, cache ignore rules, and summarizer client changes.
5. Run targeted tests after each conflict family rather than waiting until the
   entire integration is complete.

Acceptance criteria:

- No unresolved conflicts or conflict markers remain.
- The upstream-only features listed above are present and covered by tests.
- The branch's vote/block work still passes its committed fixture tests.
- `git diff --check` is clean.
- The final report records the new upstream SHA and integration commit.

### P0 — Make the quality gate reliable and cheap to run

- [ ] **T1.1 Fix all current formatting and whitespace failures.**
- [ ] **T1.2 Fix Clippy warnings instead of suppressing them globally.**
- [ ] **T1.3 Add one documented repository command, preferably `just check`,
      that runs the required Rust and Python checks.**
- [ ] **T1.4 Add the same gate to CI if this repository already has a supported
      CI mechanism; otherwise document the command without inventing new
      infrastructure.**

Evidence from the review snapshot:

- `cargo fmt --all -- --check` failed on changed Rust files.
- `git diff --check upstream/main...HEAD` found trailing whitespace at lines 3
  and 4 of [`../scrapers/written-qa-plan.md`](../scrapers/written-qa-plan.md).
- `cargo clippy --workspace --all-targets --message-format=short -- -D warnings`
  exited with 25 errors. These included unused imports, a collapsible `if`, a
  `new_without_default` implementation, overly complex types, duplicate `if`
  bodies, and functions with 8 to 14 arguments.
- No existing CI definition or `just check` target was found.

Do not "fix" Clippy merely by adding crate-wide allowances. The argument-count
and type-complexity warnings point directly at the maintainability problems in
T3 and T4. A narrow allowance is acceptable only when the API is inherently
fixed and the reason is documented next to it.

Suggested local gate:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --message-format=short -- -D warnings
cargo test --workspace --all-targets
UV_CACHE_DIR=/tmp/partijgedrag-uv-cache uv run --locked --project tools/graph-viewer pytest -q tools/graph-viewer
UV_CACHE_DIR=/tmp/partijgedrag-uv-cache uv run --locked --project tools/qa-triage pytest -q tools/qa-triage
git diff --check upstream/main...HEAD
```

Verify the exact `uv` invocation against each tool's `pyproject.toml`; adjust
working directories if necessary. The command added to the repository must work
from the repository root on a clean checkout.

Acceptance criteria:

- All commands above pass from a clean checkout.
- The common gate is documented in the README or contributor documentation.
- No blanket lint suppression hides the identified design smells.

### P0 — Replace cache-dependent no-op tests with committed fixtures

- [ ] **T2.1 Inventory every test that reads `cache/`, an absolute path, or an
      environment-specific dataset.**
- [ ] **T2.2 Classify each as a deterministic unit/integration test or an
      explicitly opt-in corpus test.**
- [ ] **T2.3 Commit minimized fixtures for deterministic tests and make missing
      fixtures a hard failure.**
- [ ] **T2.4 Move broad corpus checks behind an explicit ignored test or command
      that reports how many files it actually exercised.**
- [ ] **T2.5 Add a clean-checkout verification that proves the core suite cannot
      silently execute zero cases.**

Primary locations:

- [`../scrapers/crawl/src/qa_coverage.rs`](../scrapers/crawl/src/qa_coverage.rs),
  around line 33: fixture discovery returns `Option`, and tests can `continue`
  when data is absent.
- [`../scrapers/crawl/src/agenda_timeline.rs`](../scrapers/crawl/src/agenda_timeline.rs),
  around line 701: the test returns early when a fixture is missing.
- [`../scrapers/crawl/src/written_oral_qa.rs`](../scrapers/crawl/src/written_oral_qa.rs),
  around line 251: the same optional-fixture pattern appears.
- [`../scrapers/crawl/src/proceeding_entities.rs`](../scrapers/crawl/src/proceeding_entities.rs),
  around lines 592–637: tests contain a developer-specific absolute path.
- [`../.gitignore`](../.gitignore), around line 11: `/cache` is ignored.

At review time the local checkout had 567 ignored meeting cache files, while
only 23 vote fixtures were checked in. This explains why the suite passed
locally but would exercise much less behavior on a clean checkout.

Implementation guidance:

1. Search broadly, not just at the locations above:

   ```sh
   rg -n 'cache/|/home/|fixture.*Option|if.*exists|return;|continue;' \
     --glob '*.rs' --glob '*.py' --glob '*test*'
   ```

2. Reduce representative inputs to the smallest HTML/JSON/XML/text fragments
   that reproduce the parser behavior. Remove personal data that is irrelevant
   to the assertion.
3. Store fixtures in a checked-in `tests/fixtures` or module-local `fixtures`
   directory. Resolve them relative to `CARGO_MANIFEST_DIR` or `__file__`, never
   the process working directory or a home directory.
4. Have table-driven tests enumerate named fixture cases and assert the expected
   case count before looping.
5. For optional real-corpus validation, use an explicit environment variable or
   CLI argument and print a summary such as `files=567 decisions=...`. The
   normal test suite must neither depend on nor silently discover that corpus.

Acceptance criteria:

- Deleting or moving the ignored local `cache/` directory does not reduce the
  number of deterministic assertions executed.
- A missing committed fixture fails with a path-rich error.
- No checked-in test contains a developer-specific absolute path.
- Corpus tests are opt-in and fail or clearly report when they process zero
  files.
- All new fixtures are small, documented, and intentionally tracked.

### P1 — Consolidate plenary and commission ingestion orchestration

- [ ] **T3.1 Characterize the intended behavior differences between plenary and
      commission ingestion.**
- [ ] **T3.2 Extract a shared orchestration layer into the `crawl` crate.**
- [ ] **T3.3 Keep source-specific parsing and policy behind small typed hooks or
      configuration.**
- [ ] **T3.4 Delete the duplicated runner code after both binaries use the shared
      implementation.**

Primary locations:

- [`../scrapers/plenary-meetings/src/main.rs`](../scrapers/plenary-meetings/src/main.rs),
  runner beginning around line 290 and duplicated support logic around line 703.
- [`../scrapers/commission-meetings/src/main.rs`](../scrapers/commission-meetings/src/main.rs),
  runner beginning around line 299 and duplicated support logic around line 597.
- [`../scrapers/crawl`](../scrapers/crawl), the intended home for reusable crawl
  behavior.

The two binaries duplicate discovery, downloading, gap handling, manifest
updates, and publishing. They have already diverged: plenary live recovery can
download missing historic cache entries while commission processing aborts;
`record_gap`/`upsert_gap` behavior and timestamp handling also differ. Do not
assume these differences are accidental. Capture them in tests and then decide
which belong in shared policy versus source-specific behavior.

Recommended design:

- Introduce a shared `MeetingIngestionRunner` or a small set of pure workflow
  functions in `crawl`.
- Represent source-specific behavior with a compact `MeetingSource` trait or
  typed configuration. Prefer explicit methods such as `discover`,
  `fetch_missing`, `parse`, and `gap_policy` over callbacks with large tuples.
- Use typed manifest/gap records. Avoid stringly typed status and method fields.
- Keep CLI parsing and user-facing logging in the two binaries.
- Keep source-specific parsers in their existing source crates.
- Return structured errors with meeting/source context rather than logging an
  error deep in the shared layer and continuing implicitly.

Acceptance criteria:

- The end-to-end workflow exists in one implementation, not two copied
  `main.rs` blocks.
- The meaningful plenary/commission differences are named and tested.
- Both binaries produce equivalent manifests and outputs for their existing
  fixtures.
- Failure behavior for a missing historic cache entry is explicit for both
  sources.
- The refactor reduces code size and function argument counts; it must not just
  move duplicated code into two new modules.

### P1 — Turn vote assembly into an explicit, testable state machine

- [ ] **T4.1 Add characterization tests for every committed vote fixture before
      restructuring the assembler.**
- [ ] **T4.2 Introduce a `VoteAssembler` state object with named invariants.**
- [ ] **T4.3 Split event handling into small methods by event type.**
- [ ] **T4.4 Replace stringly typed roles, methods, and statuses with enums where
      values are closed.**
- [ ] **T4.5 Replace long argument lists with domain records and delete obsolete
      transitional helpers.**

Primary location:

- [`../scrapers/crawl/src/vote_assembly.rs`](../scrapers/crawl/src/vote_assembly.rs).
  The central function starts around line 36 and runs for roughly 600 lines.
  `push_span` has about 8 arguments, and `build_decision` around line 849 has 14.

The current function is an implicit state machine: many mutable variables carry
context across a long event loop, while nested conditionals determine when a
decision begins, absorbs spans/results, and closes. This is difficult to audit
because the valid states and transitions exist only in control flow.

Recommended shape:

```text
VoteAssembler
├── current_block / current_decision
├── pending_context and source provenance
├── on_heading(...)
├── on_vote_marker(...)
├── on_roll_call_result(...)
├── on_interruption(...)
├── finish_decision(...)
└── finish(...)
```

Implementation guidance:

1. Snapshot existing fixture outputs, preferably as semantic assertions rather
   than giant opaque snapshots. Include ordering, span boundaries, result
   occurrence, source offsets, and provenance.
2. Define the state struct first and move existing variables into it without
   changing control flow.
3. Extract one event category at a time. Run focused tests after every move.
4. Introduce records such as `DecisionContext`, `SourceSpan`, and
   `DecisionOutcome` so helper calls accept one domain object rather than many
   loosely related scalars.
5. Make illegal transitions return contextual errors or explicit diagnostics.
   Do not silently discard an event merely because it does not fit the expected
   sequence.
6. Keep the broad parser behavior required by `AGENTS.md`; centralize known
   variants instead of narrowing accepted input.

Acceptance criteria:

- No single vote assembly function remains hundreds of lines long.
- State and transition invariants are documented next to their types.
- Helper functions use domain records rather than 8–14 scalar arguments.
- Existing fixtures produce semantically identical outputs unless an intentional
  fix is separately documented and tested.
- A malformed or incomplete sequence produces an actionable diagnostic.

### P1 — Remove duplicate coverage-viewer implementations

- [ ] **T5.1 Extract shared coverage data transformation, filtering, and
      rendering logic into one JavaScript module.**
- [ ] **T5.2 Keep embedded-panel and standalone-report behavior in thin
      adapters.**
- [ ] **T5.3 Remove duplicated CSS injection and duplicated same-named helper
      functions.**
- [ ] **T5.4 Add focused browser/DOM tests for the shared behavior if the current
      test stack supports them.**

Primary locations:

- [`../tools/graph-viewer/app/static/coverage-panel.js`](../tools/graph-viewer/app/static/coverage-panel.js),
  beginning around line 11.
- [`../tools/graph-viewer/app/static/report-coverage.js`](../tools/graph-viewer/app/static/report-coverage.js),
  beginning around line 5.

The files duplicate overlay styles and approximately 25 same-named functions.
That makes fixes likely to land in one UI and not the other.

Acceptance criteria:

- There is one implementation of coverage normalization, status mapping,
  filtering, and common rendering.
- Entry-point files contain only environment-specific bootstrapping and DOM
  attachment.
- Existing embedded and standalone views retain their behavior.
- Shared styles are loaded from one asset rather than inserted as copied string
  literals.

### P1 — Establish one canonical QA check catalog

- [ ] **T6.1 Define a machine-readable canonical catalog for QA check IDs and
      metadata.**
- [ ] **T6.2 Load or generate the Rust and Python representations from it.**
- [ ] **T6.3 Make code pointers reference catalog IDs without copying check
      descriptions.**
- [ ] **T6.4 Add exact set-equality and required-field tests across consumers.**

Current sources of truth:

- [`../scrapers/qa/src/check_catalog.rs`](../scrapers/qa/src/check_catalog.rs),
  around line 11.
- [`../scrapers/qa/src/lib.rs`](../scrapers/qa/src/lib.rs), registration list
  around line 254.
- [`../tools/qa-triage/qa_triage/check_catalog.py`](../tools/qa-triage/qa_triage/check_catalog.py),
  beginning at line 1.
- [`../tools/qa-triage/qa_triage/code_pointers.py`](../tools/qa-triage/qa_triage/code_pointers.py),
  around line 5.

There is already semantic drift. In Rust,
`vote.source_inventory_vs_parquet` describes ordered result occurrences/source
roll-call results; the Python entry describes counting vote rows per meeting.
This is evidence that synchronized handwritten dictionaries are not viable.

Recommended format:

- Use a small checked-in TOML, JSON, or YAML file with stable IDs, title,
  description, severity/category, and optional remediation text.
- Prefer runtime loading when both languages already have a suitable parser and
  startup cost is irrelevant. Otherwise generate source deterministically and
  add a stale-generated-file check.
- Keep executable check registration in Rust, but assert that its ID set exactly
  equals the catalog set.
- Keep implementation pointers separate from user-facing semantics.

Acceptance criteria:

- Editing a description happens once.
- Rust registration IDs, Rust result IDs, Python triage IDs, and catalog IDs
  match exactly; tests fail on missing and extra entries.
- The known `vote.source_inventory_vs_parquet` disagreement is resolved based
  on actual implementation behavior.
- Generated output, if used, is deterministic and checked for staleness.

### P1 — Make graph-viewer queries use its database abstraction

- [ ] **T7.1 Inventory every production `read_parquet` call and hardcoded
      session path in the graph-viewer app.**
- [ ] **T7.2 Define all supported datasets as views in `db.py`.**
- [ ] **T7.3 Make query modules consume views rather than reconstructing file
      paths.**
- [ ] **T7.4 Discover available sessions instead of assuming session 56.**
- [ ] **T7.5 Replace broad `except Exception` query fallbacks with specific,
      observable error handling.**

Primary locations:

- [`../tools/graph-viewer/app/db.py`](../tools/graph-viewer/app/db.py), around
  line 16, already defines DuckDB views.
- [`../tools/graph-viewer/app/queries/browse.py`](../tools/graph-viewer/app/queries/browse.py),
  around line 116, bypasses those views with direct Parquet paths.
- [`../tools/graph-viewer/app/queries/entity_preview.py`](../tools/graph-viewer/app/queries/entity_preview.py),
  around line 1134, parses a session into `_session` and then ignores it while
  using session 56 paths.

At review time there were 43 production references to `sessions/56` under the
graph-viewer app. Direct reads scatter schema and path knowledge through the UI
layer and prevent `db.py` from being a meaningful abstraction.

Implementation guidance:

1. Use `rg -n 'read_parquet|sessions/56' tools/graph-viewer/app` to build the
   inventory and turn it into a checklist in the implementation PR/commit.
2. Centralize root and session discovery in `db.py`. Expose a clear error when
   no compatible dataset is present.
3. Normalize schemas in views when files vary by session; query code should see
   stable columns.
4. Parameterize values through DuckDB bindings. Paths and identifiers that
   cannot be bound should come only from validated central configuration.
5. Catch missing-table/file and query errors specifically. Do not return an
   empty result for every exception, because that makes a broken query look like
   valid absent data.

Acceptance criteria:

- Query modules do not contain hardcoded session numbers or direct staging-file
  paths.
- Selecting a non-56 available session changes all relevant query results.
- The entity preview uses its parsed session.
- Missing data and programmer/query errors are distinguishable to users and in
  logs.
- Existing graph-viewer tests pass, with added coverage for multiple sessions.

### P2 — Finish adoption of the shared Mistral client

- [ ] **T8.1 Compare behavior in both local client implementations with the new
      shared crate.**
- [ ] **T8.2 Add missing shared-client capabilities and typed errors.**
- [ ] **T8.3 Migrate dossier and text summarizers.**
- [ ] **T8.4 Delete their duplicate rate limiter and completion functions.**

Primary locations:

- [`../summarizers/mistral-client/src/lib.rs`](../summarizers/mistral-client/src/lib.rs),
  shared `RateLimiter` around line 13 and client call around line 74.
- [`../summarizers/dossier-summarizer/src/main.rs`](../summarizers/dossier-summarizer/src/main.rs),
  local limiter around line 20 and `mistral_complete` around line 349.
- [`../summarizers/text-summarizer/src/main.rs`](../summarizers/text-summarizer/src/main.rs),
  local limiter around line 21 and `mistral_complete` around line 490.

Before deleting code, compare retry policy, status handling, request/response
shape, rate limiting, timeouts, error context, and logging. Move intentional
differences into typed shared options. Avoid an overly generic client with many
boolean flags; model coherent policies as enums or configuration records.

Acceptance criteria:

- Both summarizers call the shared client.
- There is one rate-limiting and HTTP/retry implementation.
- Errors distinguish transport, status, decoding, and response-content failure
  and retain relevant request context without leaking secrets.
- Tests cover rate limiting and representative failure responses without live
  network calls.

### P2 — Generate data-graph documentation from one catalog

- [ ] **T9.1 Choose or create one canonical catalog for graph nodes, tables,
      stages, and edges.**
- [ ] **T9.2 Generate the Markdown, HTML, and canvas data from that catalog.**
- [ ] **T9.3 Add a stale-output check to the common quality gate.**

Duplicated locations:

- [`../DATA_GRAPH.md`](../DATA_GRAPH.md).
- [`data-graph-overview.html`](data-graph-overview.html), catalog around line 417.
- [`../canvases/data-graph-overview.canvas.tsx`](../canvases/data-graph-overview.canvas.tsx),
  catalog around line 42.

Do not make a UI artifact the canonical source. Prefer a small declarative data
file that can be validated for unique IDs, valid references, stage ordering,
and required provenance metadata. Generated artifacts should include a header
that names their source and regeneration command.

Acceptance criteria:

- A table, node, or edge is described once.
- All three artifacts are deterministic products of the same catalog.
- The generator rejects dangling edges and duplicate identifiers.
- The normal quality gate fails when generated documentation is stale.

## Suggested landing sequence

Keep commits independently reviewable and avoid combining mechanical movement
with behavior changes:

1. Integrate `upstream/main` and add regression tests for conflict resolutions.
2. Fix whitespace/formatting and establish the shared check command.
3. Commit deterministic fixtures and remove cache-dependent no-op tests.
4. Consolidate shared crawl infrastructure and migrate both meeting runners.
5. Refactor vote assembly behind characterization tests.
6. Consolidate identity/normalization/graph changes that remain in the large
   branch diff, without mixing in UI work.
7. Canonicalize QA metadata.
8. Refactor graph-viewer database access and shared coverage UI code.
9. Complete Mistral client adoption.
10. Generate graph documentation from a canonical catalog.

If the branch must be reviewed as multiple pull requests, preserve this order
and use dependency branches. A reasonable conceptual split is:

1. shared crawl infrastructure, meeting parser migration, and upstream
   integration;
2. identity, normalization, and graph model changes;
3. QA checks plus committed fixtures;
4. graph viewer;
5. LLM enrichment and generated documentation.

Do not use the split as a reason to duplicate code temporarily across multiple
long-lived branches. Land the shared foundation first.

## Validation record at the review snapshot

These results describe the reviewed checkout, which included ignored local
cache data:

| Check | Result | Notes |
| --- | --- | --- |
| `cargo test --workspace --all-targets` | Passed | 287 test executions, but some tests no-op without local cache data. |
| Graph-viewer `pytest -q` | Passed | 62 passed, 2 skipped. |
| QA-triage `pytest -q` | Passed | 7 passed, 1 skipped. |
| `cargo check --workspace --all-targets` | Passed with warnings | Compilation alone does not meet the quality bar. |
| `cargo fmt --all -- --check` | Failed | Changed Rust files require formatting. |
| `git diff --check upstream/main...HEAD` | Failed | Trailing whitespace in `scrapers/written-qa-plan.md`. |
| Clippy with `-D warnings` | Failed | 25 errors, including structural complexity warnings. |

The passing test count must not be used as proof of clean-checkout coverage
until T2 is complete.

## Final definition of done

The branch is ready for human review only when all of the following are true:

- [ ] Latest upstream is integrated and upstream-only behavior is preserved.
- [ ] All P0 tasks are complete.
- [ ] Every remaining P1/P2 item is either complete or moved to a separately
      tracked issue with a named owner and a clear reason it does not block this
      branch.
- [ ] The documented root-level quality command passes on a clean checkout.
- [ ] Deterministic tests do not read ignored caches or absolute local paths.
- [ ] The meeting runners share one orchestration implementation.
- [ ] Vote assembly state and transitions are explicit and covered by fixtures.
- [ ] QA metadata and graph documentation each have one source of truth.
- [ ] Graph-viewer queries do not hardcode session 56 or bypass the database
      abstraction.
- [ ] Summarizers use the shared Mistral client.
- [ ] `cargo fmt`, strict Clippy, Rust tests, both Python test suites, generated
      artifact checks, and `git diff --check` all pass.
- [ ] The final diff has been reviewed for code deletion opportunities and no
      obsolete transitional implementation remains.

When handing off again, update the snapshot SHAs, checkboxes, validation table,
and any intentional behavior changes. Do not mark a task complete based only on
code movement; verify its acceptance criteria and record the validating command.
