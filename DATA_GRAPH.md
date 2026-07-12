# Data graph

Canonical model for Belgian Chamber (dekamer.be) data. Staging may stay Parquet/JSONL; this is the target graph.

Interactive overview (Cursor Canvas): [`canvases/data-graph-overview.canvas.tsx`](canvases/data-graph-overview.canvas.tsx) — open beside the chat to explore nodes, edges, and implementation status.

## Data stack

Decisions for anyone (human or LLM) extending this repo:

- **Canonical storage = Parquet** (open, portable, engine-neutral). It is the source of truth and the data product; optimize it for longevity and universal queryability.
- **Query/serving engines are swappable and derived** — never the canonical form. Do not promote an engine-specific database file (`.duckdb`, `.kuzu`, etc.) to source of truth; emit such files only as rebuildable convenience artifacts.
- **DuckDB is the current query engine** (used by the `web` consumer at build time), but treated as one swappable consumer, not a commitment.
- **Future LLM/GraphRAG use cases** (e.g. "standpoint of person X on topic Y") need hybrid retrieval: embeddings + vector search + graph traversal + structured filters. Add these as derived indexes (DuckDB `vss`/`fts`, or Lance/LanceDB for embedding-heavy tables) rebuilt from Parquet, keyed to graph node ids. The hard part is modeling (topic links + embeddings on utterances/questions, stance signals, minister resolution), not the container format.

## Nodes


| Node                         | ID key                                                          | Source                                                                                                                                                                | Notes                                                                                                                     |
| ---------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| **Session**                  | `session_id`                                                    | [cvlist54](https://www.dekamer.be/kvvcr/showpage.cfm?section=/depute&language=nl&cfm=/site/wwwcfm/depute/cvlist54.cfm)                                                | Already scraped.                                                                                                          |
| **Person**                   | stable `person_id` from `cvview_key` + per-session mandate rows | Member list + [cvview/cvview54](https://www.dekamer.be/kvvcr/) detail pages                                                                                           | Chamber MPs only (cvview-backed). Query MPs via `node_type = Person`. |
| **ExternalPerson**           | `external_person_id` slug (`ext:person:…`, `ext:role:…`, `ext:org:…`) | `just build-identity` (`external-identity` scan); bio via `enrich-external-persons` | Ministers, experts, procedural roles (Voorzitter), institutional authors. Separate from MPs; 57 entities in graph. |
| **Party**                    | slug / official name                                            | Member list, vote appendix, dossier authors                                                                                                                           | Fraction names drift; time-bounded membership required.                                                                   |
| **Commission**               | name / enum                                                     | [LstCom.cfm](https://www.dekamer.be/kvvcr/showpage.cfm?section=/none&language=nl&cfm=/site/wwwcfm/comm/LstCom.cfm)                                                    | Already scraped; link to meetings and dossier trajectories.                                                               |
| **Meeting**                  | `{session_id, kind, meeting_id}`                                | Plenary HTML `PCRI/ip{N}x.html`; commission HTML `CCRI/ic{N}x.html`                                                                                                   | Already scraped (metadata). Gaps in commission IDs.                                                                       |
| **AgendaItem**               | `{meeting_id, seq}`                                             | Meeting report headings (`h1`/`h2`, agenda numbers)                                                                                                                   | NL/FR pairs, mis-tagged `lang` attrs; section boundaries are heuristic.                                                   |
| **Utterance**                | `{session}_{kind}_{meeting}_{agenda}_{turn}` + `seq`            | Integraal verslag (plenary + commission); staging `utterances.parquet` (42 887 rows)                                                                                  | Full-session extraction: `question`, `general_debate`, `proposition`, `hearing`, `vote`, `notice`. 2 576 rows without `SPOKE` (chairs skipped, unresolved speakers). |
| **Question**                 | `{session}_{kind}_{meeting}_{seq}` (+ site ref in `internal_ids`) | Oral: meeting reports; written: QRVA bulletins / FLWB                                                                                                                 | Oral partially scraped. Graph `question_id` is meeting-scoped and kind-scoped; site refs (`Q…P` / `Q…C`) live in staging `internal_ids`. Written Q&A not scraped. |
| **Answer**                   | linked to Question                                              | Same as Question                                                                                                                                                      | Often merged into discussion text; minister ≠ MP entity resolution.                                                       |
| **Dossier**                  | `{session_id}/{number}`                                         | [flwbn.cfm](https://www.dekamer.be/kvvcr/showpage.cfm?section=/flwb&language=nl&cfm=/site/wwwcfm/flwb/flwbn.cfm) and static `/flwb/html/{session}/N/{doc}.html` pages | Scraped via plenary refs plus full FLWB browse (`ListDocument.cfm` → `ListFromTo.cfm`).    |
| **Document**                 | FLWB doc id (`56K1280004`)                                      | Dossier subdocuments (PDF/HTML)                                                                                                                                       | Already scraped (metadata). Body text via PDF→markdown pipeline.                                                          |
| **Amendment**                | document id + dossier                                           | Dossier subdocuments typed `AMENDEMENT`                                                                                                                               | Same as Document; authors are structured on site.                                                                         |
| **Report**                   | document id + dossier                                           | Subdocuments typed `VERSLAG`; commission integraal                                                                                                                    | PDF-heavy; rapporteur named on dossier page, speech inside PDF.                                                           |
| **Motion**                   | motion id + meeting/dossier context                             | Motions database; vote titles                                                                                                                                         | Referenced by vote parsing today but not modelled as a node.                                                              |
| **Hearing**                  | `{session_id}_{meeting_kind}_{meeting_id}_{seq}` (+ site refs in `internal_ids`) | Commission integraal (`hoorzitting met` / `audition de` h2); plenary rare | Staging `hearings.parquet`; graph Hearing nodes; `item_kind=hearing` utterances linked via `item_id`. |
| **Interpellation**           | `{session_id}_{meeting_kind}_{meeting_id}_{seq}` (+ `56000070I` in `internal_ids`) | Plenary integraal (`Interpellatie van` / `Interpellation de` h2) | Staging `interpellations.parquet`; graph Interpellation nodes; site ref suffix `I`. Commission has no formal interpellation h2s. |
| **Vote**                     | `{meeting_id, vote_id}`                                         | Plenary integraal (tables + appendix) only                                                                                                                            | Already scraped. **Commission integraal has no roll-call vote tables** — do not port plenary `extract_votes`.             |
| **VoteCast**                 | `{vote_id, person_id, position}`                                | Vote appendix tables                                                                                                                                                  | Name→person matching; “Nee” block parsing is fragile.                                                                     |
| **Topic**                    | Eurovoc id + label                                              | Dossier fiche; optional NLP on utterances                                                                                                                             | Eurovoc on dossiers scraped. Utterance tagging not done.                                                                  |
| **LobbyOrg**                 | name                                                            | Lobby register PDF (`lobbyregister.pdf`)                                                                                                                              | Scraped flat (301 orgs); `DECLARES_INTEREST` links to persons not wired yet.                                                |
| **Remuneration**             | `{person, year, mandate}`                                       | [regimand.be](https://public.regimand.be/)                                                                                                                            | External site; matched by name only.                                                                                      |
| **MediaRecording**           | media id                                                        | [media.dekamer.be](https://media.dekamer.be/)                                                                                                                         | Not scraped; tie to meeting via title/date.                                                                               |
| **InterventionAnalysis**     | dossier or meeting ref                                          | Dossier fiche “Analyse van de tussenkomsten”; search DB                                                                                                               | Not scraped; likely pre-structured speaker/topic data.                                                                    |


## Edges


| Edge                | From → To                          | Source                                              | Notes                                                          |
| ------------------- | ---------------------------------- | --------------------------------------------------- | -------------------------------------------------------------- |
| `MEMBER_OF`         | Person → Party                     | Member list (per session)                           | Time range = session or explicit start date on CV.             |
| `MEMBER_OF`         | Person → Commission                | Commission membership lists                         | Permanent vs replacement; scraped as strings.                  |
| `HOLDS_ROLE`        | Person → Meeting / Dossier         | Meeting header (chair); dossier fiche (rapporteur)  | Chair regex exists for commission; rapporteur on dossier page. |
| `ATTENDED`          | Person → Meeting                   | Opening/closing attendance lists in plenary report  | Ministers listed; not fully parsed today.                      |
| `SPOKE`             | Person / ExternalPerson → Utterance                 | Integraal verslag speaker lines                     | MPs preferred when in index; ministers/experts/roles → ExternalPerson. |
| `PART_OF`           | Utterance → Meeting                | Parent report                                       | All utterances link to meeting.                                |
| `PART_OF`           | Utterance → Question               | Q&A agenda item                                     | When `item_kind = question`.                                   |
| `ASKED`             | Person → Question                  | Question header (“Vraag van …”)                     | Oral scraped; written not yet.                                 |
| `ANSWERED`          | Person / ExternalPerson → Question                  | Question header respondents                         | Wired via `ActorResolver` (7 185 edges: 5 525 Person, 1 660 ExternalPerson). Portfolio titles → ExternalPerson; no mandate-over-time table yet. |
| `ABOUT`             | Question → Topic                   | Question title text                                 | Free text; summarizer exists.                                  |
| `LINKED_TO`         | Question → Dossier                 | Question internal id / FLWB cross-ref               | Commission questions carry dossier ids.                        |
| `AUTHORED`          | Person → Document                  | Dossier subdocument author list                     | Structured on site; stored as CSV today.                       |
| `REFERENCES`        | Meeting → Dossier                  | Proposition/vote titles `(297/10)`                  | Regex extraction; already collected as sidecar ids.            |
| `DISCUSSED_IN`      | Dossier → Meeting                  | Dossier fiche calendar (commission + plenary steps) | Rich HTML; not ingested as edges.                              |
| `INTERPELLED`       | Person → Interpellation              | `interpellators` on interpellation header           | Same resolver path as `ASKED`.                                 |
| `RESPONDED`         | Person / ExternalPerson → Interpellation | `respondents` on interpellation header          | Distinct from `ANSWERED` (Question target).                    |
| `INVITED`           | Person / ExternalPerson → Hearing    | `witnesses` on hearing header (when parseable)      | Best-effort; empty witnesses common.                           |
| `VOTED_ON`          | Vote → Dossier / Document / Motion | Vote title                                          | Partially parsed (dossier_id, motion_id).                      |
| `CAST`              | Person → VoteCast → Vote           | Vote appendix                                       | Needs normalization from name lists.                           |
| `TAGGED_WITH`       | Dossier → Topic                    | Eurovoc on dossier fiche                            | Scraped.                                                       |
| `TAGGED_WITH`       | Utterance → Topic                  | NLP / intervention analysis                         | Future; analysis DB may shortcut.                              |
| `SUBMITTED`         | Document → Dossier                 | FLWB hierarchy                                      | Already scraped as subdocuments.                               |
| `DECLARES_INTEREST` | Person → LobbyOrg                  | Lobby register                                      | Not linked yet.                                                |
| `EARNED`            | Person → Remuneration              | regimand.be                                         | Name match only.                                               |
| `RECORDED_IN`       | Meeting → MediaRecording           | media.dekamer.be                                    | Not scraped.                                                   |


## Source map (dekamer.be)


| Bucket                     | URL pattern / entry                           | Maps to                                                  |
| -------------------------- | --------------------------------------------- | -------------------------------------------------------- |
| Plenary integraal          | `/doc/PCRI/html/{session}/ip{N}x.html`        | Meeting, AgendaItem, Utterance, Question, Vote, VoteCast |
| Commission integraal       | `/doc/CCRI/html/{session}/ic{N}x.html`        | Meeting, Utterance, Question, Hearing, Interpellation (rare) |
| Plenary/commission beknopt | via Documenten → Beknopt verslag              | Utterance (summary text)                                 |
| Dossiers                   | `flwbn.cfm?legislat=&dossierID=`              | Dossier, Document, edges to Meeting/Person/Topic         |
| FLWB browse                | `ListDocument.cfm?legislat=` → `ListFromTo.cfm` | Dossier id discovery (union with plenary refs)          |
| FLWB PDFs                  | `/FLWB/PDF/{session}/…`                       | Document body (Report, Amendment, QRVA)                  |
| Written Q&A                | Documenten → Bulletins schriftelijke vragen   | Question, Answer                                         |
| Intervention analysis      | Dossier fiche + search databank               | Utterance, Topic, Person links                           |
| Members                    | `cvlist54.cfm`, `cvview54.cfm`                | Person, Party, Commission membership                     |
| Votes (detail)             | Plenary appendix in same HTML                 | VoteCast                                                 |
| Video                      | media.dekamer.be                              | MediaRecording → Meeting                                 |
| Lobby                      | kvvcr lobby register / cached register export | LobbyOrg                                                 |
| Remunerations              | public.regimand.be                            | Remuneration → Person                                    |


## Current repo coverage


| Status                                     | Nodes / edges                                                                                                                                                                                     |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Scraped, flat**                          | Session, Person, Commission, Meeting, Question (oral), Vote, Dossier, Document (meta), Remuneration, LobbyOrg (301), staging `utterances.parquet`, `hearings.parquet`, `interpellations.parquet` |
| **Canonicalized and working**              | Identity: Person, ExternalPerson (57), Party, Commission, memberships, person/external aliases; normalized edges (`vote_casts`, `authored`, `asked`, `answered`, `interpellated`, `interpellation_responded`, `invited`, `holds_role`, `utterances`); graph Parquet; Hearing + Interpellation nodes with `PART_OF` utterance links |
| **Parsed but not normalized**              | Commission role edges beyond chair; 2 576 utterances without `SPOKE` (unresolved/chair speakers); INQO oral-control id enrichment for interpellations |
| **Enrichment (optional LLM)**              | `external_person_bios.parquet` via `just enrich-external-persons` (Mistral Agents API + `web_search`)                                                                                             |
| **Not scraped / not normalized**           | Written Question/Answer, Motion (as node), InterventionAnalysis, MediaRecording, Meeting↔Dossier calendar (`DISCUSSED_IN`), Beknopt verslag, `DECLARES_INTEREST`, **commission roll-call votes** (not present in integraal verslag HTML) |
| **Debug tooling (local only)**             | `tools/graph-viewer` — search, inspector, vote breakdown, discussion threads, unresolved-person triage; issues panel reads `data/qa/checks.parquet` from `just qa` |

## Implementation state

Verified against branch `stage-viz` on 2026-07-07. Counts below are from the last full pipeline run (`just update`, 2026-07-07).

### Pipeline (Steps 0–3)

- **Stage 0 contract (partially done):** `STAGING.md` documents every current Parquet schema and ID conventions. Major scrapers now emit `source_url` + `cache_path` on staging rows. Question ids include meeting kind (`{session}_{plenary\|commission}_{meeting}_{seq}`) via `composite_scoped_id`, with `ensure_question_id` upgrading legacy rows at normalize/graph time; site-native refs stay in `internal_ids`. Still open: commission `meeting_id` vs `commission_id` rename, vote ids still meeting-scoped composites (not site-native), idempotent per-meeting incremental scraping.
- **Stage 0 QA (superseded):** `just qa` now regenerates `data/qa/checks.parquet` and `summary.md` from detail rows. Stale Jul-1 artifacts replaced.
- **Step 1 identity (working):** `just build-identity` runs `identity` + `external-identity`. Writes `persons.parquet`, `parties.parquet`, `person_aliases.parquet`, `commissions.parquet`, `memberships.parquet`, `external_persons.parquet` (57), `external_person_aliases.parquet` (129), `external_person_contexts.parquet`, `unresolved_persons.parquet`, and `unresolved_report.md`. Last run: 175 persons, 13 parties, 1 302 memberships, 21 unresolved commission-member occurrences (placeholder `N .` only).
- **Step 2 normalize (working):** `just normalize-edges` routes all person-bearing edges through `Resolver` or `ActorResolver` into `data/normalized/*.parquet` (196 059 vote casts, 6 790 authored, 11 765 asked, 7 291 answered, 29 interpellated, 24 interpellation responded, 414 holds_role, 43 431 utterances; 4 997 unresolved names). Six vote reconciliation mismatches remain.
- **Step 3 graph (working):** `just build-graph` writes `data/graph/nodes.parquet` (55 993 nodes including 3 Hearing + 29 Interpellation), `edges.parquet` (355 913 edges: adds `INTERPELLED`, `RESPONDED`, `PART_OF` utterance→proceeding links), and `source_artifacts.parquet` (2 388 artifacts). 145 orphan `VOTED_ON` edges. 2 659 Utterance nodes without `SPOKE`. CAST is Person→Vote (no separate VoteCast node).

### Branch additions since 2026-07-01

- **Dossier discovery (Step 4, done):** `just scrape-dossiers` unions plenary-derived ids with FLWB browse (`ListDocument.cfm` → `ListFromTo.cfm`). Graph now has 1 640 Dossier nodes.
- **Full-session utterances (Step 4, done):** `crawl` report-block stream + agenda timeline + speaker segmentation writes staging `utterances.parquet` for plenary and commission (43 431 rows). Normalized and graphed as Utterance nodes with `SPOKE` / `PART_OF`.
- **Hearings & interpellations (Step 4, done):** shared `proceeding_entities` in `crawl`; staging `hearings.parquet` + `interpellations.parquet`; normalize `INTERPELLED` / `RESPONDED` / `INVITED`; graph Hearing + Interpellation nodes with utterance `PART_OF` links.
- **External identity + ActorResolver (Step 1–2, done):** `external-identity` scans staging for non-MP actors; `ActorResolver` routes speakers, authors, and respondents to Person or ExternalPerson. `ANSWERED` edges wired (7 185). Optional `enrich-external-persons` adds LLM bios.
- **Lobby register (Step 6 partial):** `scrape-lobby` downloads `lobbyregister.pdf`, extracts 301 orgs to `lobby.parquet` with `source_url` + `cache_path`.
- **Commission meeting gaps:** `meeting_gaps.parquet` tracks ids in `1..=last` with no scraped row (10 gaps today).
- **Speaker QA (Step 5 interim):** Broader meeting-report crosschecks catalogued in `meeting-report-qa-plan.md`.
- **QA runner (Step 5, working):** `just qa` (`scrapers/qa`) writes `data/qa/meeting_report_check_details.parquet`, derived `checks.parquet`, `summary.md`, `alias_candidates.parquet`, and `row_counts.json`. Summary counts are derived from detail rows (S8 meta-check). `just qa-strict` exits non-zero on regression vs committed `checks_baseline.parquet`. Last run: 10 763 detail rows, 16 issue checks (6 vote mismatches, 145 orphan `VOTED_ON`, 2 576 speakerless utterances, 2 384 empty `scraped_at`, etc.). `tools/graph-viewer` issues panel reads `data/qa/` (no recompute).
- **Graph debug viewer (`tools/graph-viewer`, local only):** FastAPI + DuckDB UI over `data/graph/*.parquet`. Search entities; inspector with metadata, excerpts, discussion threads, and source links; paginated edge lists; vote reconciliation totals; **vote breakdown** (yes/no/abstain member lists with clickable resolved persons); unresolved-person and data-quality issue panels; open dekamer.be source pages and cached HTML/PDF. Run: `cd tools/graph-viewer && uv sync && uv run uvicorn app.main:app --reload --port 8765` (after `just build-graph`). Node routes use `{node_id:path}` so dossier ids like `56/297` resolve correctly.

### Not done yet

Government/minister mandate-over-time table (portfolio titles resolve to ExternalPerson today, not dated mandates) and remaining Step 4 sources (written Q&A, Motion node, dossier calendar edges, Beknopt verslag, INQO id enrichment). Stage 0 schema-hygiene checks are partially reimplemented in `just qa` (`schema.*` tier).

### Commission roll-call votes (negative finding)

Scanned all cached commission integraal verslag HTML (session 56): **0 files** contain plenary-style roll-call markers (`Stemming`, `DETAIL VAN DE NAAMSTEMMINGEN`, Ja/Nee member tables). `votes.parquet` and `CAST` edges are **plenary integraal only**. Commission adoption is sometimes mentioned in prose (e.g. *wordt unaniem aangenomen*); that is not modelled as Vote/CAST. Future sources: commission PDF verslag (FLWB `VERSLAG`), dossier fiche calendar, or a dedicated roll-call feed if one appears.


## Scrutiny

This is a good target model, but it should be treated as a graph normalization layer above the current scrapers, not as a replacement for the raw/staging outputs. The Chamber site is old, multilingual and inconsistent; keeping raw HTML/PDF provenance beside every normalized edge is not optional.

What is strong:

- The core spine is right: `Person` ↔ `Meeting` ↔ `Utterance` ↔ `Question` / `Vote` ↔ `Dossier` / `Document`.
- The plan correctly prefers site-native ids over generated hashes. Live member and dossier pages expose `cvview*.cfm?key=...`, FLWB document ids, dossier ids, question ids and PDF paths.
- The emphasis on aliasing and confidence is necessary. The code already has typo maps, name reordering, reversed email strings, mis-tagged `lang` attributes and CSV person lists.

What is missing:

- A first-class `Membership` / `Mandate` shape. Party and commission membership need `role`, `start_date`, `end_date`, `source`, `active`, and sometimes replacement/permanent status; an edge with only a time range will become too thin.
- `Motion`, `Interpellation`, `Hearing`, and probably `Notice` / procedural agenda entries. **Hearing and Interpellation nodes are now wired** (staging → normalize → graph). Remaining: Motion node, INQO id enrichment, procedural adoption tagging in commission prose.
- A source/provenance table for raw artifacts: report HTML, dossier HTML, PDF, converted markdown, search result page. **Partially addressed:** `source_artifacts.parquet` + edge-level `source_artifact_id`/`source_url`/`cache_path` exist; staging rows now carry `source_url`/`cache_path`. Still missing: `scraped_at` on artifacts, PDF/markdown artifact registration, parser version on every source type.
- A canonical bilingual text strategy. Many entities have NL and FR titles/topics; some source `lang` attributes are wrong. The graph should keep language-tagged text variants rather than picking one string per entity.
- Validation gates: row counts, referential integrity, unmatched names, duplicate site ids, and expected deltas per scrape run.
- Stable, site-native ids for `Question` and `Vote`. **Partially addressed for questions:** ids are now `{session}_{meeting_kind}_{meeting_id}_{seq}` (deterministic given meeting content order) with site refs in `internal_ids`; plenary/commission meeting-number collisions are fixed. **Still open for votes** and for using site-native question refs as primary graph ids; the plenary scraper still re-scrapes every meeting `1..=last` on each run.
- A `Government` / minister-mandate concept. **Partially addressed:** portfolio titles and named ministers route to `ExternalPerson` via `ActorResolver` (1 660 `ANSWERED` edges). Still missing: title→person-over-time table for mid-session portfolio changes.
- Idempotent, delta-aware scraping. Re-scraping everything and reassigning ids defeats the "expected deltas per run" check; per-meeting incremental writes with stable keys are a prerequisite for catching site changes.

What is unnecessary or lower priority:

- `MediaRecording` should wait until the core person/dossier/vote/question graph is stable. It is useful, but it depends on fuzzy title/date matching.
- NLP `Topic` tagging for utterances should wait. Eurovoc on dossiers and the Chamber intervention-analysis database are higher-signal structured sources.
- `LobbyOrg` and `Remuneration` are valuable enrichment, but they should not block the parliamentary core graph because both require weaker name matching and external/source-specific handling.

## Data quality

The data is dirty in predictable ways: misspelled and renamed people, "Last First" vs "First Last", mis-tagged `lang`, government title strings instead of persons, drifting fraction/commission names, gaps in meeting ids, and PDF column noise. Proper QA needs the canonical identity layer and graph builder in place first, so it lands as its own block (Step 5) once the core architecture exists. Then every transform emits a QA report next to its output, and a single `just qa` runner aggregates them.

Two tiers of signal (report only — the QA runner never fails the process):

- **Issues:** duplicate keys, broken foreign keys, malformed dates, document ids not matching `\d\dK\d{7}` when they look like FLWB ids, impossible vote totals, missing tables, row-count deltas, unresolved linkages, and other oddities.
- **Passes:** checks that came back clean on this run.

Each step writes to `data/qa/`:

- `unresolved_persons.parquet` — name, source bucket (`votes`, `authors`, `speakers`, `questioners`, `respondents`, `commission_members`), example context, count. This is the non-linkage catch.
- `alias_candidates.parquet` — unresolved name, best person match, similarity score, so aliasing is a triage queue rather than guesswork. This is the misspelling/rename catch.
- `<table>_checks.parquet` — per-table row counts, null rates, FK/domain failures, and delta vs last run. This is the oddity/regression catch.

Three questions to keep answering at every step: did anything fail to link, does anything look out of range or duplicated, and did the numbers move more than expected since the last run.

## Next steps to reach the target graph

Steps 1–3 are working end-to-end (`just build-identity` → `just normalize-edges` → `just build-graph`). Step 4 has dossier discovery and full-session utterances done. **Immediate next work:** Step 5 automated QA (promote `meeting-report-qa-plan.md` checks into a `qa` binary; fix 6 vote reconciliation failures); Step 4 remainder (written Q&A, Motion/Hearing nodes, dossier calendar edges); resolve 4 695 unresolved names and 2 576 speakerless utterances; government mandate-over-time table. Re-run Stage 0 QA to clear stale `dossier_ids` warning.

**0. Freeze the staging contract. Partly done**

- ~~Write down every current Parquet schema exactly as produced (`STAGING.md`).~~ Done.
- ~~Add `source_url` + `cache_path` on staging rows.~~ Done for sessions, members, commissions, plenary/commission meetings, dossiers, lobby, remunerations.
- ~~Scope question ids by meeting kind.~~ Done (`composite_scoped_id` / `ensure_question_id`).
- Fix known mislabels: commission meetings write the meeting id under `commission_id` (rename to `meeting_id`); commission question refs belong in `internal_ids`, not a `dossier_ids` column (fixed in scraper; regenerate + re-run QA).
- Stable vote ids and idempotent per-meeting incremental scraping still open.

**1. Canonical identity + one resolver everything routes through. Working for normalize pipeline**

- ~~Build identity seed tables and memberships.~~ Done (`just build-identity`).
- ~~Route high-value edges through `resolve_person`.~~ Done in `just normalize-edges` (votes, authors, questioners, chairs, Q&A speakers).
- Remaining: government mandate-over-time table (portfolio title → person by date); treat `N .` commission placeholders as vacancies rather than alias candidates; extend resolver to new Step 4 sources as they land.

**2. Normalize the high-value edges you already have (no new scraping yet). Done and working**

- `just normalize-edges` routes vote roll-calls, document authors, commission chairs, questioners, and Q&A discussion speakers through the identity resolver into `data/normalized/`.
- Unresolved names aggregate to `data/normalized/unresolved_persons.parquet` (4 695); vote reconciliation is in `vote_reconciliation.parquet` (6 mismatches). Speaker QA in `data/qa/speaker_*.parquet`.
- **Remaining in this step:** government mandate-over-time table; reduce 2 576 utterances without `SPOKE`. Utterance ids are stable per meeting report turn; `seq` is the thread order key within a meeting.

**3. Stand up the graph builder early (not last). Done and working**

- `just build-graph` emits deterministic `data/graph/nodes.parquet`, `edges.parquet`, and `source_artifacts.parquet` from identity, staging, and normalized outputs.
- Edges carry `source_artifact_id`, `source_url`, `cache_path`, and `confidence`; artifacts carry `parser_version`.
- **Remaining in this step:** populate `scraped_at` on artifacts; decide whether to add explicit `VoteCast` nodes (today CAST is Person→Vote); fix or flag orphan `VOTED_ON` targets (145 edges to dossier refs not in the graph).

**4. Add the missing parliamentary core sources — each behind the resolver + QA + graph edges.**

- ~~Full FLWB dossier discovery from the browse/full-text databank, not only ids seen in plenary refs.~~ **Done (2026-07-06):** 1 640 Dossier nodes in graph.
- ~~Full-session utterance extraction from integraal verslagen.~~ **Done (2026-07-07):** staging + normalized `utterances.parquet` (42 887 rows); graph Utterance nodes with `SPOKE`/`PART_OF`.
- Written Q&A from the QRVA bulletins (`/QRVA/pdf/{session}/…`) / search database.
- `Motion`, `Interpellation`, `Hearing`, and procedural `Notice` handling (Motion node; INQO enrichment; optional commission procedural adoption tagging from prose).
- Parse the dossier fiche calendar into `DISCUSSED_IN`, `SUBMITTED`, `AUTHORED`, and rapporteur/chair role edges.

**5. Now that the architecture exists, stand up QA over it.**

- ~~Create a `qa` binary~~ **Done:** `scrapers/qa` + `just qa` / `just qa-strict` / `just qa-update-baseline`.
- ~~Derive `checks.parquet` from detail rows (S8).~~ **Done.**
- ~~Promote graph-viewer issue checks into automated runner.~~ **Done** (`graph.*`, vote, speaker, schema tiers). Viewer reads `data/qa/` only.
- Expand source-level crosschecks per `meeting-report-qa-plan.md` as scraper fixes land; tighten `warn` → `fail` per check after fixture review.
- Wire resolver triage: `alias_candidates.parquet` on every run (done); extend bucket coverage.

**6. Only then enrich.**

- Confirm the dossier "Analyse van de tussenkomsten" actually exists and is structured before planning around it; if so, ingest it before any NLP utterance tagging.
- Add media recordings once meeting/date/title matching is measurable (report the match rate).
- ~~Wire the lobby register download~~ **Done (2026-07-07):** PDF download + 301 orgs scraped. Remaining: `DECLARES_INTEREST` person links and QA match rates.
- Remuneration matching once person identity is stable; both lobby and remuneration rely on weaker name matching, so they must surface their match/unmatch rates in QA.

## Conventions

- Every edge carries `source_url`, `scraped_at`, and `confidence` (exact parse vs inferred vs NLP).
- `Person` resolution goes through `person_alias` before any `SPOKE` / `CAST` / `AUTHORED` edge is committed.
- Prefer site-native ids (FLWB doc id, question id, cvview key) over generated hashes.

