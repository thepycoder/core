# Meeting Report QA Crosscheck Plan

## Data Snapshot

Data inspected from `/home/victor/Projects/partijgedrag-parent/partijgedrag/core/data`; this worktree still has no local `data/data` target for `web/src/data`.

- Staging scale: 133 plenary meetings, 66 commission meetings, 766 plenary questions, 725 commission questions, 1,394 plenary votes, 313 propositions, 180 notices.
- Existing Stage 0 QA (`data/qa/checks.parquet`): 99 checks, 97 pass, 2 warn. These are mostly schema hygiene (non-empty columns, unique keys, row-count deltas, FK checks). Only two checks touch meeting-report semantics today.
- Known semantic warnings:
  - `commission_questions.dossier_ids_mislabel` — stale; staging now uses `internal_ids` (normalize blocks on `dossier_ids`).
  - `plenary_votes.vote_total_reconciliation` — reports 1 mismatch, but `normalized/vote_reconciliation.parquet` has **6** unreconciled votes. Summary and detail outputs must be derived from the same source.
- Vote reconciliation failures today (`56_129_4`, `56_133_18`–`21`, `56_133_32`): headline totals disagree with parsed member-name counts. Meeting 129 is instructive: compact tables list votes 1,2,3,5,6,7,8,9,10 (vote 4 missing), appendix headers list 1–10 complete, parquet has 16 vote rows (vote 9 reused via paragraph copy).
- Utterance coverage: 7,975 normalized utterances, all Q&A-derived. Full-session speech (debates, hearings, vote explanations) is invisible to utterance QA. Fixture gaps: `ip019x` (84 source markers, 0 utterances), `ip129x` (95, 0), `ic001x` (51, 0), `ic015x` (16, 0).
- Graph-level signals already surfaced manually in `tools/graph-viewer`: 145 orphan `VOTED_ON` edges, 1,154 utterance nodes without `SPOKE`, 0 unresolved vote names.

## Goal

Build source-level and parquet-level **crosschecks** that catch subtle scraper drift: missing votes, wrong roll-call parsing, dropped agenda items, incomplete discussions, broken dossier links, and impossible person/activity combinations.

The plan below is a **ranked catalog** of crosscheck opportunities. Checks are ordered by impact: how likely they are to catch real data loss or wrong parliamentary facts, and how independently they verify extraction.

## Impact Tiers

| Tier | Meaning | Default severity | Examples |
|------|---------|------------------|----------|
| **S — Critical** | Independent recount of parliamentary facts; catches silent extraction loss | `fail` once baseline reviewed | Vote totals across compact table, appendix bucket, and parsed names |
| **A — High** | Strong structural crosscheck; catches parser drift affecting graph/web | `warn` → `fail` | Source agenda heading count vs parquet row count |
| **B — Medium** | Referential integrity and linkage correctness | `warn` | Dossier ref exists in `dossiers.parquet` |
| **C — Low** | Schema/regression hygiene; necessary but weak semantic signal | `pass`/`info` | Row-count delta, required non-empty |
| **D — Infrastructure** | Source availability and encoding; prerequisite, not correctness proof | `info` | HTTP 200, cache file exists, Word metadata |

**Principle:** Word-export metadata (`o:Words`, `o:Paragraphs`) is tier D. Comparing vote counts in compact result tables, appendix bucket headers, and parsed name lists is tier S — same source document, three independent representations of the same fact.

## Output Shape

Keep `data/qa/checks.parquet` as the summary artifact. **Derive summary rows from a detail artifact** so counts cannot drift (fixes the 1-vs-6 vote reconciliation mismatch).

Add `data/qa/meeting_report_check_details.parquet`:

| Column | Purpose |
|--------|---------|
| `check_id` | Stable id from catalog below |
| `severity` | `fail` / `warn` / `info` |
| `status` | `pass` / `warn` / `fail` |
| `session_id`, `meeting_kind`, `meeting_id` | Meeting scope |
| `entity_type`, `entity_id` | Vote, question, agenda item, etc. |
| `expected`, `actual` | Comparable values (counts, ids, totals) |
| `message` | Human-readable explanation |
| `source_url`, `cache_path`, `source_block` | Traceability |
| `created_at` | Run timestamp |

Aggregate to `checks.parquet`: `table`, `check`, `status`, `count`, `detail`, `examples` (top N detail rows).

Optional debug artifact: `data/qa/report_blocks.parquet` — ordered `h1`/`h2`/`p`/`table` blocks per meeting for reproducing source-side counts (shared with utterance extraction plan).

---

## Ranked Crosscheck Catalog

### Tier S — Critical

These should be implemented first. Each compares two or more independent representations of the same fact inside the integraal verslag or across staging → normalized layers.

#### S1. Vote compact totals vs parsed member lists
- **Check id:** `vote.compact_total_vs_member_names`
- **Compares:** Per vote row: `yes`/`no`/`abstain` columns vs `len(split_csv(members_*))`.
- **Why high impact:** Direct arithmetic on roll-call data. Already partially implemented (`plenary_votes.vote_total_reconciliation`); extend to all meetings and wire through detail artifact. Currently 6 failures.
- **Signals today:** `votes.parquet`, `vote_reconciliation.parquet`.
- **Example:** `56_129_4`: `yes=81`, `members_yes_count=131`.

#### S2. Appendix bucket count vs collected voter names
- **Check id:** `vote.appendix_bucket_vs_collected_names`
- **Compares:** Re-parse source HTML: appendix table bucket count (`Oui/Ja`, `Non/Nee`, `Abstentions/Onthoudingen` td) vs paragraph-collected names vs staging `members_*` vs `vote_casts` per bucket.
- **Why high impact:** Catches the fragile `extract_voter_names` sibling-walk parser independently of compact tables. A bucket count of 81 with 131 parsed names means name collection ran into the wrong paragraph block.
- **Signals today:** Cached HTML + `votes.parquet` + `vote_casts.parquet`.
- **Source anchors:** `Vote nominatif - Naamstemming: N` span → three bucket tables → following `<p>` name paragraphs.

#### S3. Compact vote tables vs appendix headers
- **Check id:** `vote.compact_tables_vs_appendix_headers`
- **Compares:** Distinct `Stemming/vote N` table headers vs `Naamstemming - Vote nominatif: N` (and FR variant) per meeting.
- **Why high impact:** Detects missing compact tables when appendix still exists. Meeting 129: compact has no vote 4, appendix has votes 1–10.
- **Signals today:** Cached HTML only (independent of scraper output).
- **Default severity:** `warn` (some meetings legitimately reuse prior results via paragraph copy).

#### S4. Source vote inventory vs parquet vote rows
- **Check id:** `vote.source_inventory_vs_parquet`
- **Compares:** Per meeting: count of distinct source vote numbers (from S3 + paragraph-reuse votes) vs `votes.parquet` row count; flag votes in source without parquet row and vice versa.
- **Why high impact:** Catches dropped or duplicated votes at meeting level before graph assembly.
- **Signals today:** Cached HTML + `votes.parquet`.

#### S5. Source agenda entity counts vs parquet rows
- **Check id:** `agenda.entity_count_vs_parquet`
- **Compares:** Per meeting and section (`mondelinge vragen`, `(wets)voorstel`, `mededelingen`, `naamstemmingen`, commission hearings): source heading/table inventory vs parquet row counts.
- **Why high impact:** Catches section-boundary drift — the most common silent failure mode (stop parsing too early, miss last question, skip hearing).
- **Signals today:** Cached HTML + `questions.parquet` / `propositions.parquet` / `notices.parquet` / `votes.parquet`.
- **Section parsers to mirror:** `extract_questions`, `extract_propositions`, `extract_notices`, `extract_votes`, commission hearing skip at `hoorzitting`/`audition`.

#### S6. Source speaker markers vs normalized utterances
- **Check id:** `utterance.source_markers_vs_normalized`
- **Compares:** Per meeting: `speaker_regex` marker count (including `De voorzitter`/`Le président`) vs `utterances.parquet` row count.
- **Why high impact:** Only check that proves full-session speech coverage once utterance plan lands. Today it exposes the Q&A-only blind spot (`ip019x`: 84 markers, 0 rows).
- **Signals today:** Cached HTML + `utterances.parquet`.
- **Blocked until:** Full-session utterance extraction (see `meeting-report-utterances-plan.md`). Run in `warn` mode against current Q&A baseline.

#### S7. Discussion JSON entries vs source speaker markers
- **Check id:** `utterance.discussion_vs_source_markers`
- **Compares:** Per question: `len(discussion JSON)` vs speaker-marker count in the question's source text span (from heading through `Het incident is gesloten` / `L'incident est clos`).
- **Why high impact:** Catches partial `get_discussion_json` extraction without waiting for full-session utterances. Directly validates the current Q&A pipeline.
- **Signals today:** `questions.discussion` + cached HTML.
- **Example:** `ic057x` question `56_57_5`: raw row exists, `discussion=[]`, source has markers.

#### S8. QA summary count vs detail rows
- **Check id:** `qa.summary_vs_detail`
- **Compares:** `checks.parquet` warning count per `check_id` vs `count(detail rows)` for same check.
- **Why high impact:** Meta-check preventing the tooling from lying. Fixes 1-vs-6 vote reconciliation reporting gap.
- **Signals today:** `checks.parquet`, `meeting_report_check_details.parquet`.

---

### Tier A — High

Strong crosschecks that catch parser drift, bilingual pairing errors, and roundtrip inconsistencies.

#### A1. Grouped question internal refs preserved
- **Check id:** `question.grouped_internal_ids_complete`
- **Compares:** All `Q…C`/`Q…P` refs in grouped heading text vs `internal_ids` column.
- **Why:** Grouped questions are the hardest heading parse; missing a sub-ref loses a site-native link.

#### A2. Heading question refs appear exactly once
- **Check id:** `question.heading_refs_unique`
- **Compares:** Each `Q…` ref in source headings maps to exactly one `questions.parquet` row's `internal_ids`.
- **Why:** Detects duplicate flushes or missed `incident is gesloten` boundaries.

#### A3. NL/FR question ref parity
- **Check id:** `bilingual.question_refs_match`
- **Compares:** `internal_ids` derivable from NL heading text equals refs from paired FR heading.
- **Why:** Catches lang-attribute swaps without failing extraction.

#### A4. NL/FR agenda number pairing
- **Check id:** `bilingual.agenda_number_paired`
- **Compares:** Paired `h2` headings share agenda digit span and section (`01`, `18`, etc.).
- **Why:** Proposition/notice pairing uses position-based NL/FR split; mis-pairing corrupts `title_nl`/`title_fr` linkage.

#### A5. NL/FR dossier/document ref parity
- **Check id:** `bilingual.dossier_ref_match`
- **Compares:** `(501/1-2)` in NL vote/proposition title matches FR sibling.
- **Why:** Dossier refs are the spine for `VOTED_ON` and proposition vote attachment.

#### A6. Meeting date source vs parquet
- **Check id:** `meeting.date_source_vs_parquet`
- **Compares:** Date from first report table vs `meetings.parquet.date`.
- **Why:** Wrong date breaks session-window membership checks and web date display.

#### A7. Opening/closing times source vs parquet
- **Check id:** `meeting.times_source_vs_parquet`
- **Compares:** `extract_start_time` / `extract_end_time` re-run on cache vs `start_time`/`end_time` columns. Support `14.18 uur`, `14:18 uur`, FR variants, midnight rollover.
- **Why:** Duration and attendance reasoning depend on correct times.

#### A8. Commission chair source vs parquet
- **Check id:** `meeting.chair_source_vs_parquet`
- **Compares:** `voorgezeten door …` / `présidé par …` opening paragraph vs `meetings.chair`.
- **Why:** Chair is the only commission role edge wired today (`holds_role`).

#### A9. Person in multiple vote buckets
- **Check id:** `vote.duplicate_person_across_buckets`
- **Compares:** No `person_id` appears in more than one of yes/no/abstain for the same `vote_id` (after resolution).
- **Why:** CSV join bugs and appendix paragraph bleed create impossible casts.

#### A10. Vote cast count vs headline after normalization
- **Check id:** `vote.cast_count_vs_headline`
- **Compares:** `count(vote_casts by position)` vs `yes`/`no`/`abstain` per vote.
- **Why:** Catches resolver dropping names that staging captured, distinct from S1 (which checks staging self-consistency).

#### A11. Discussion roundtrip from utterances
- **Check id:** `utterance.roundtrip_discussion`
- **Compares:** Rebuild `questions.discussion` JSON from `utterances.parquet` per question; hash/compare to staging `discussion`.
- **Why:** Proves utterance layer is a superset of current Q&A extraction before replacing `discussion` in production.

#### Speech character coverage (tier A — bridge QA)
- **Check id:** `utterance.speech_char_coverage`
- **Compares:** Per meeting: whole cached HTML word count (all `h1`/`h2`/`p`/`table` block text via `parse_report_blocks`) vs one saved word total (utterances + questions + plenary votes/propositions/notices + commission chair).
- **Why:** Single symmetric ratio — “how much of the report did we persist anywhere?” Flags volume loss from dropped paragraphs, missed sections, or parser regressions.
- **Signals today:** Cached HTML + staging `utterances.parquet`, `questions.parquet`, `votes.parquet`, `propositions.parquet`, `notices.parquet`, `meetings.parquet` (commission chair).
- **Thresholds:** `warn` on kind p5 outlier (≥10 meetings) or ratio &lt; 85% of committed per-meeting baseline (`speech_coverage_baseline.parquet`). No absolute fail threshold in v1.
- **Blind spots:** Bilingual duplication in source widens denominator; wrong speaker with valid label (ratio stays high); label drift merged into prior open turn (S6 better).
- **Accepted low coverage (2026-07-12):** Do not chase coverage on constitutive / organizational plenary sessions (e.g. plenary 2–4). These reports lack standard `NN.NN Speaker:` turn markers; most missing words are procedural (oath legal text, committee/delegation name lists, bureau notices) or ceremonial (chair eulogies). That content is low value for utterance-based politics analysis (`SPOKE`, stance, Q&A, votes) and the wrong abstraction as `Utterance` rows. Notices already capture agenda-item titles; committee membership belongs in structured `MEMBER_OF` scrapers, not integraal prose. Keep `speech_char_coverage` as a regression sentinel for normal debate meetings; p5 outliers on early-session plenaries are expected, not parser bugs.

#### A12. Web `allVotes` assembly
- **Check id:** `web.allVotes_vs_staging`
- **Compares:** `meeting.allVotes.length` in `web/src/_data/meetings.js` assembly vs staging vote rows per meeting; votes attached to propositions must still appear in `allVotes`.
- **Why:** Catches double-index or dropped votes at the last mile.

#### A13. Vote number sequence continuity
- **Check id:** `vote.number_sequence`
- **Compares:** Per meeting, vote numbers form a continuous sequence except documented paragraph-reuse cases.
- **Why:** Gap detection complementary to S3; meeting 129 vote 4 gap would fire here.

---

### Tier B — Medium

Referential integrity, graph linkage, and plausibility constraints.

#### B1. Dossier refs exist
- **Check id:** `dossier.ref_exists`
- **Compares:** Non-empty `dossier_id` on votes/propositions exists in `dossiers.parquet` (`{session}/{id}`).
- **Why:** Reduces orphan `VOTED_ON` edges (145 today).

#### B2. Document refs exist
- **Check id:** `document.ref_exists`
- **Compares:** Non-empty `document_id` exists in `subdocuments.parquet` for parent dossier.
- **Why:** Document ids in vote titles are often partial (`1-2`); check both full and partial patterns, flag separately.

#### B3. Vote dossier_id matches title parse
- **Check id:** `vote.dossier_id_matches_title`
- **Compares:** `dossier_id`/`document_id` columns vs re-parse of `title_nl` through `extract_vote_data` regexes.
- **Why:** Catches title/field divergence when vote title is paragraph-format vs `h2`-format.

#### B4. CAST person was chamber member at vote date
- **Check id:** `person.cast_membership_at_vote`
- **Compares:** Each `vote_casts.person_id` has `MEMBER_OF` party edge active on vote `date`.
- **Why:** Flags wrong person resolution or guest voters.

#### B5. Graph referential integrity
- **Check id:** `graph.edge_endpoints_exist`
- **Compares:** All `edges.from`/`edges.to` resolve in `nodes.parquet` (promote `tools/graph-viewer` `orphan_from`/`orphan_to` checks).
- **Why:** Prevents broken inspector and GraphRAG traversal.

#### B6. Utterance SPOKE edge coverage
- **Check id:** `graph.utterance_spoke_resolved`
- **Compares:** Utterance nodes with resolvable non-chair speakers should have `SPOKE` edge (1,154 without today).
- **Why:** Separates expected chair/unresolved from resolver regressions.

#### B7. Commission hearing headings flagged
- **Check id:** `agenda.hearing_not_extracted`
- **Compares:** Source `hoorzitting`/`audition` headings exist but no entity rows (commission scraper deliberately skips).
- **Why:** Tracks known coverage gap; becomes `fail` when hearing extraction ships.

#### B8. Meeting FK integrity
- **Check id:** `fk.questions_votes_to_meetings`
- **Compares:** All `questions`/`votes`/`propositions`/`notices` `meeting_id` exist in respective `meetings.parquet`.
- **Why:** Basic join hygiene; partially covered by existing FK checks.

#### B9. Per-vote total plausibility
- **Check id:** `vote.total_plausibility`
- **Compares:** `yes + no + abstain <= 150`; typical range 120–150; `150 - total` = implied absentees.
- **Why:** Catches double-counting and table misreads; weaker than S1–S2 but good backstop.

#### B10. Speaker activity vs vote presence
- **Check id:** `person.speaker_without_vote`
- **Compares:** MP speaks in plenary (utterance resolved) but appears in no vote appendix that day.
- **Why:** Soft signal for arrival/departure; `warn` only (members can leave chamber).

#### B11. Duplicate utterance ids
- **Check id:** `utterance.unique_ids`
- **Compares:** `utterance_id` unique in `utterances.parquet` and graph Utterance nodes.
- **Why:** Promote graph-viewer check; seq collisions indicate question-id regressions.

#### B12. Motion id presence when title references motion
- **Check id:** `vote.motion_id_when_referenced`
- **Compares:** Vote titles matching motion regex should have non-empty `motion_id`.
- **Why:** 1,352/1,394 votes have empty `motion_id` — distinguish legitimate absence from parse miss.

---

### Tier C — Low

Schema hygiene and regression baselines. Keep from Stage 0 QA; do not mistake passes here for semantic correctness.

| Check id | Compares | Notes |
|----------|----------|-------|
| `schema.table_loaded` | Parquet readable | Already exists |
| `schema.row_count_delta` | Row count vs last run | Already exists; threshold configurable per table |
| `schema.required_non_empty` | Column null/empty rate | Already exists |
| `schema.unique_keys` | Primary keys | Already exists |
| `schema.date_format` | Date columns parse | Already exists |
| `schema.fk_*` | Cross-table FK | Extend to plenary meetings, propositions, notices |
| `schema.internal_ids_present` | Commission questions have `internal_ids` | Normalize already enforces; promote to QA |
| `normalize.unresolved_persons_by_bucket` | Unresolved counts by bucket/reason | Info signal; 870 unresolved today |
| `bilingual.lang_attr_anomaly` | `lang` attribute vs content heuristic | Log only; do not fail extraction |

---

### Tier D — Infrastructure

Prerequisites and rough signals. Run, but never substitute for S/A checks.

| Check id | Compares | Notes |
|----------|----------|-------|
| `source.cache_exists` | Every staging `cache_path` resolves on disk | Cheaper than HTTP |
| `source.http_available` | Official HTML/PDF URL returns 200 + content type | Optional network; rate-limit |
| `source.encoding_bytes` | Invalid Windows-1252 bytes in cache | `warn`; parser should tolerate |
| `source.word_metadata_present` | `o:Words`, `o:Paragraphs`, `o:Characters` in HTML | Presence only |
| `source.word_metadata_growth` | Word count vs previous cache snapshot | **Low semantic value** — coarse truncation hint; large drift warrants manual review, not auto-fail |
| `source.report_heading_present` | At least one `h1`/`h2` | Trivially true for real reports |
| `artifact.scraped_at_populated` | `source_artifacts.scraped_at` non-empty | Graph provenance gap |

---

## Implementation Phases

Ordered by impact, not document section order.

### Phase 1 — Vote truth layer (S1–S4, S8, A9–A10)
1. Build shared HTML re-parser for vote inventory (compact tables, appendix headers, bucket counts, name paragraphs).
2. Emit all vote mismatches to `meeting_report_check_details.parquet`.
3. Derive `checks.parquet` from details; delete hand-maintained summary counts.
4. Fix known failures (`56_129_4`, `56_133_*`) with appendix parser tests.

### Phase 2 — Agenda completeness (S5, A1–A5, B1–B3, B7)
1. Build agenda timeline / block stream (shared with utterance plan).
2. Per-section entity count checks.
3. Bilingual pairing and ref completeness checks.
4. Dossier/document referential checks.

### Phase 3 — Speech coverage (S6–S7, A11, B6, B10–B11)
1. Speaker-marker counter on cached HTML (reuse `speaker_regex`).
2. Per-question discussion completeness.
3. After full-session utterances: meeting-level marker reconciliation and roundtrip check.

### Phase 4 — Metadata and assembly (A6–A8, A12–A13, B4–B6)
1. Meeting date/time/chair source crosschecks.
2. Web assembly verification.
3. Graph integrity promotion from graph-viewer.

### Phase 5 — Baseline hygiene (C*, D*)
1. Keep Stage 0 schema checks.
2. Add cache existence and encoding checks.
3. Word metadata as info-only regression signal.

---

## Regression Fixtures

Prioritize meetings/checks that represent each failure class:

| Fixture | Exercises |
|---------|-----------|
| `ip129x.html` (`56_129_4`) | S1/S2 appendix name bleed; compact vs appendix vote 4 gap |
| `ip133x.html` (`56_133_18`–`21`, `56_133_32`) | Multiple bucket/total mismatches |
| `ip019x.html` | S6 full-session speech gap (no Q&A section) |
| `ip117x.html` | High vote volume (150 votes); sequence and inventory stress |
| `ic001x.html`, `ic015x.html` | Commission speech without questions |
| `ic017x.html` | `toegevoegde vragen` grouped-heading variant |
| `ic057x.html` (`56_57_5`) | S7 empty `discussion` with source speech |
| `ic002x.html` | Legacy `internal_ids` / grouped commission questions |

---

## Implementation Notes

- Avoid relying on DuckDB JSON extension autoloading in restricted environments; parse `discussion` JSON in Rust (already done in normalize).
- Re-parse checks should use the same `clean_text` / selector helpers as scrapers, but must be **independent code paths** — copying scraper output into both sides of a crosscheck defeats the purpose.
- Promote `warn` → `fail` per check only after fixture baseline review.
- First concrete fixes: derive `checks.parquet` from details (S8), implement S1–S4 vote layer, add S7 discussion-vs-markers, then S5 agenda counts.
- `commission_questions.dossier_ids_mislabel` can be retired once QA regenerates against `internal_ids` schema.
