# Data graph

Canonical model for Belgian Chamber (dekamer.be) data. Staging may stay Parquet/JSONL; this is the target graph.

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
| **ExternalPerson**           | `external_person_id` slug (`ext:person:…`, `ext:role:…`, `ext:org:…`) | `just build-identity` (staging scan); bio via `enrich-external-persons` | Ministers, experts, procedural roles (Voorzitter), institutional authors. Separate from MPs. |
| **Party**                    | slug / official name                                            | Member list, vote appendix, dossier authors                                                                                                                           | Fraction names drift; time-bounded membership required.                                                                   |
| **Commission**               | name / enum                                                     | [LstCom.cfm](https://www.dekamer.be/kvvcr/showpage.cfm?section=/none&language=nl&cfm=/site/wwwcfm/comm/LstCom.cfm)                                                    | Already scraped; link to meetings and dossier trajectories.                                                               |
| **Meeting**                  | `{session_id, kind, meeting_id}`                                | Plenary HTML `PCRI/ip{N}x.html`; commission HTML `CCRI/ic{N}x.html`                                                                                                   | Already scraped (metadata). Gaps in commission IDs.                                                                       |
| **AgendaItem**               | `{meeting_id, seq}`                                             | Meeting report headings (`h1`/`h2`, agenda numbers)                                                                                                                   | NL/FR pairs, mis-tagged `lang` attrs; section boundaries are heuristic.                                                   |
| **Utterance**                | `{meeting_id, seq}` or span ref                                 | Integraal verslag (plenary + commission); beknopt verslag; dossier PDFs                                                                                               | Q&A speaker blocks partially parsed into JSON blobs today. Full debates (`01.07 — Name (Party):`) are not normalized yet. |
| **Question**                 | `{session}_{kind}_{meeting}_{seq}` (+ site ref in `internal_ids`) | Oral: meeting reports; written: QRVA bulletins / FLWB                                                                                                                 | Oral partially scraped. Graph `question_id` is meeting-scoped and kind-scoped; site refs (`Q…P` / `Q…C`) live in staging `internal_ids`. Written Q&A not scraped. |
| **Answer**                   | linked to Question                                              | Same as Question                                                                                                                                                      | Often merged into discussion text; minister ≠ MP entity resolution.                                                       |
| **Dossier**                  | `{session_id}/{number}`                                         | [flwbn.cfm](https://www.dekamer.be/kvvcr/showpage.cfm?section=/flwb&language=nl&cfm=/site/wwwcfm/flwb/flwbn.cfm) and static `/flwb/html/{session}/N/{doc}.html` pages | Scraped via plenary refs plus full FLWB browse (`ListDocument.cfm` → `ListFromTo.cfm`).    |
| **Document**                 | FLWB doc id (`56K1280004`)                                      | Dossier subdocuments (PDF/HTML)                                                                                                                                       | Already scraped (metadata). Body text via PDF→markdown pipeline.                                                          |
| **Amendment**                | document id + dossier                                           | Dossier subdocuments typed `AMENDEMENT`                                                                                                                               | Same as Document; authors are structured on site.                                                                         |
| **Report**                   | document id + dossier                                           | Subdocuments typed `VERSLAG`; commission integraal                                                                                                                    | PDF-heavy; rapporteur named on dossier page, speech inside PDF.                                                           |
| **Motion**                   | motion id + meeting/dossier context                             | Motions database; vote titles                                                                                                                                         | Referenced by vote parsing today but not modelled as a node.                                                              |
| **Interpellation / Hearing** | oral-control id or `{meeting_id, agenda_seq}`                   | Commission/plenary reports; INQO search database                                                                                                                      | Needed because commission reports are not only questions; hearings are explicitly skipped today.                          |
| **Vote**                     | `{meeting_id, vote_id}`                                         | Plenary integraal (tables + appendix)                                                                                                                                 | Already scraped. Roll-call names are CSV strings, not person links.                                                       |
| **VoteCast**                 | `{vote_id, person_id, position}`                                | Vote appendix tables                                                                                                                                                  | Name→person matching; “Nee” block parsing is fragile.                                                                     |
| **Topic**                    | Eurovoc id + label                                              | Dossier fiche; optional NLP on utterances                                                                                                                             | Eurovoc on dossiers scraped. Utterance tagging not done.                                                                  |
| **LobbyOrg**                 | name                                                            | Lobby register HTML                                                                                                                                                   | Parser exists, but live download/source URL is not wired; no links to persons/dossiers yet.                               |
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
| `PART_OF`           | Utterance → Meeting                | Parent report                                       |                                                                |
| `PART_OF`           | Utterance → AgendaItem             | Section context                                     | Requires reliable section parser.                              |
| `ASKED`             | Person → Question                  | Question header (“Vraag van …”)                     | Oral scraped; written not yet.                                 |
| `ANSWERED`          | Person / ExternalPerson → Question                  | Question header respondents                         | Wired in normalize; same MP-preferring policy.                         |
| `ABOUT`             | Question → Topic                   | Question title text                                 | Free text; summarizer exists.                                  |
| `LINKED_TO`         | Question → Dossier                 | Question internal id / FLWB cross-ref               | Commission questions carry dossier ids.                        |
| `AUTHORED`          | Person → Document                  | Dossier subdocument author list                     | Structured on site; stored as CSV today.                       |
| `REFERENCES`        | Meeting → Dossier                  | Proposition/vote titles `(297/10)`                  | Regex extraction; already collected as sidecar ids.            |
| `DISCUSSED_IN`      | Dossier → Meeting                  | Dossier fiche calendar (commission + plenary steps) | Rich HTML; not ingested as edges.                              |
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
| Commission integraal       | `/doc/CCRI/html/{session}/ic{N}x.html`        | Meeting, Utterance, Question                             |
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
| **Scraped, flat**                          | Session, Person, Commission, Meeting, Question (oral), Vote, Dossier, Document (meta), Remuneration                                                                                               |
| **Canonicalized and working**              | Identity seed tables: Person, ExternalPerson, Party, Commission, party memberships, commission memberships, person/external aliases; normalized edges (`vote_casts`, `authored`, `asked`, `answered`, `holds_role`, `utterances`) and graph Parquet (`nodes`, `edges`, `source_artifacts`) |
| **Parsed but not normalized**              | General plenary debate utterances (outside Q&A `discussion` JSON), commission role edges beyond chair |
| **Enrichment (optional LLM)**              | `external_person_bios.parquet` via `just enrich-external-persons` (Mistral Agents API + `web_search`) |
| **Parser exists but source is incomplete** | LobbyOrg                                                                                                                                                                                          |
| **Not scraped / not normalized**           | Written Question/Answer, general plenary Utterances, Motion, Interpellation/Hearing, InterventionAnalysis, MediaRecording, Meeting↔Dossier calendar, Beknopt verslag |
| **Debug tooling (local only)**             | `tools/graph-viewer` — search, inspector, vote breakdown, unresolved-person triage, data-quality issues over `data/graph/*.parquet` (not part of the scraper pipeline) |

## Implementation state

Verified against branch `stage-viz` on 2026-07-06. Counts below are from the last full pipeline run (2026-07-01); re-run scrapers → `normalize-edges` → `build-graph` after dossier discovery to refresh graph size.

### Pipeline (Steps 0–3)

- **Stage 0 contract (partially done):** `STAGING.md` documents every current Parquet schema and ID conventions. Major scrapers now emit `source_url` + `cache_path` on staging rows. Question ids include meeting kind (`{session}_{plenary\|commission}_{meeting}_{seq}`) via `composite_scoped_id`, with `ensure_question_id` upgrading legacy rows at normalize/graph time; site-native refs stay in `internal_ids`. Still open: commission `meeting_id` vs `commission_id` rename, vote ids still meeting-scoped composites (not site-native), idempotent per-meeting incremental scraping.
- **Stage 0 QA (soft report):** `data/qa/summary.md` last reported 97 passes and 2 issues: stale commission-question `dossier_ids` mislabel signal, and one plenary vote total mismatch in meeting 129. Regenerated staging passes the normalize staging check (`internal_ids` present, no `dossier_ids`); re-run Stage 0 QA to clear the stale schema warning.
- **Step 1 identity (working):** `just build-identity` writes `data/identity/persons.parquet`, `parties.parquet`, `person_aliases.parquet`, `commissions.parquet`, `memberships.parquet`, `unresolved_persons.parquet`, and `unresolved_report.md`. Last run: 175 persons, 13 parties, 175 party memberships, 1127 resolved commission memberships, 21 unresolved commission-member occurrences (placeholder `N .` only).
- **Step 2 normalize (working):** `just normalize-edges` routes vote roll-calls, document authors, questioners, commission chairs, and Q&A discussion speakers through the identity resolver into `data/normalized/*.parquet` (189 400 vote casts, 1 851 authored, 2 693 asked, 67 holds_role, 7 975 utterances; 870 unresolved names, mostly speakers and respondent titles). One vote reconciliation mismatch remains (`56_129_4`). Minister/government resolution and `ANSWERED` edges not started — respondent titles only land in `unresolved_persons`.
- **Step 3 graph (working):** `just build-graph` writes `data/graph/nodes.parquet` (10 652 nodes), `edges.parquet` (215 991 edges: MEMBER_OF, CAST, AUTHORED, ASKED, HOLDS_ROLE, SPOKE, PART_OF, VOTED_ON, SUBMITTED, TAGGED_WITH), and `source_artifacts.parquet` (660 artifacts). Referential integrity is clean on `from` endpoints; 141 `VOTED_ON` edges point at dossier ids not present as nodes (partial vote-title refs like `1-5`, not full `56/N` dossiers). Edges carry `source_artifact_id`, `source_url`, `cache_path`, and `confidence`; `scraped_at` on artifacts is still empty. CAST is Person→Vote (no separate VoteCast node).

### Branch additions since 2026-07-01

- **Dossier discovery (Step 4, done):** `just scrape-dossiers` unions plenary-derived ids from `dossier_ids.txt` with all session dossiers discovered via FLWB `ListDocument.cfm` → `ListFromTo.cfm`, then downloads `flwbn.cfm` fiches. Expect more Dossier/Document nodes after a full re-scrape and graph rebuild.
- **Graph debug viewer (`tools/graph-viewer`, local only):** FastAPI + DuckDB UI over `data/graph/*.parquet`. Search entities; inspector with metadata, excerpts, and source links; paginated edge lists; vote reconciliation totals; **vote breakdown** (yes/no/abstain member lists with clickable resolved persons); unresolved-person and data-quality issue panels; open dekamer.be source pages and cached HTML/PDF. Run: `cd tools/graph-viewer && uv sync && uv run uvicorn app.main:app --reload --port 8765` (after `just build-graph`). Node routes use `{node_id:path}` so dossier ids like `56/297` resolve correctly.

### Not done yet

Minister/government resolution, automated graph-level QA (Step 5), and remaining Step 4 sources (written Q&A, motions/hearings/notices, dossier calendar edges). Graph viewer is manual inspection only — it does not replace the `qa` binary described in Step 5.


## Scrutiny

This is a good target model, but it should be treated as a graph normalization layer above the current scrapers, not as a replacement for the raw/staging outputs. The Chamber site is old, multilingual and inconsistent; keeping raw HTML/PDF provenance beside every normalized edge is not optional.

What is strong:

- The core spine is right: `Person` ↔ `Meeting` ↔ `Utterance` ↔ `Question` / `Vote` ↔ `Dossier` / `Document`.
- The plan correctly prefers site-native ids over generated hashes. Live member and dossier pages expose `cvview*.cfm?key=...`, FLWB document ids, dossier ids, question ids and PDF paths.
- The emphasis on aliasing and confidence is necessary. The code already has typo maps, name reordering, reversed email strings, mis-tagged `lang` attributes and CSV person lists.

What is missing:

- A first-class `Membership` / `Mandate` shape. Party and commission membership need `role`, `start_date`, `end_date`, `source`, `active`, and sometimes replacement/permanent status; an edge with only a time range will become too thin.
- `Motion`, `Interpellation`, `Hearing`, and probably `Notice` / procedural agenda entries. The plenary scraper already extracts propositions and notices, vote parsing references `motion_id`, and the commission scraper deliberately skips hearings.
- A source/provenance table for raw artifacts: report HTML, dossier HTML, PDF, converted markdown, search result page. **Partially addressed:** `source_artifacts.parquet` + edge-level `source_artifact_id`/`source_url`/`cache_path` exist; staging rows now carry `source_url`/`cache_path`. Still missing: `scraped_at` on artifacts, PDF/markdown artifact registration, parser version on every source type.
- A canonical bilingual text strategy. Many entities have NL and FR titles/topics; some source `lang` attributes are wrong. The graph should keep language-tagged text variants rather than picking one string per entity.
- Validation gates: row counts, referential integrity, unmatched names, duplicate site ids, and expected deltas per scrape run.
- Stable, site-native ids for `Question` and `Vote`. **Partially addressed for questions:** ids are now `{session}_{meeting_kind}_{meeting_id}_{seq}` (deterministic given meeting content order) with site refs in `internal_ids`; plenary/commission meeting-number collisions are fixed. **Still open for votes** and for using site-native question refs as primary graph ids; the plenary scraper still re-scrapes every meeting `1..=last` on each run.
- A `Government` / minister-mandate concept. Respondents are portfolio title strings (`de minister van …`), not persons, and portfolios change hands mid-session. Resolving "who answered" needs a title→person-over-time table, not the MP `Person` table alone.
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

Steps 1–3 are working end-to-end (`just build-identity` → `just normalize-edges` → `just build-graph`). Step 4 has one item done (FLWB dossier discovery). **Immediate next work:** re-scrape dossiers and rebuild the graph to pick up the expanded dossier set; then continue Step 4 (written Q&A, motions/hearings, calendar edges), Step 2 remainder (minister resolution), and Step 5 (automated QA). Re-run Stage 0 QA once to clear the stale `dossier_ids` warning.

**0. Freeze the staging contract. Partly done**

- ~~Write down every current Parquet schema exactly as produced (`STAGING.md`).~~ Done.
- ~~Add `source_url` + `cache_path` on staging rows.~~ Done for sessions, members, commissions, plenary/commission meetings, dossiers, lobby, remunerations.
- ~~Scope question ids by meeting kind.~~ Done (`composite_scoped_id` / `ensure_question_id`).
- Fix known mislabels: commission meetings write the meeting id under `commission_id` (rename to `meeting_id`); commission question refs belong in `internal_ids`, not a `dossier_ids` column (fixed in scraper; regenerate + re-run QA).
- Stable vote ids and idempotent per-meeting incremental scraping still open.

**1. Canonical identity + one resolver everything routes through. Working for normalize pipeline**

- ~~Build identity seed tables and memberships.~~ Done (`just build-identity`).
- ~~Route high-value edges through `resolve_person`.~~ Done in `just normalize-edges` (votes, authors, questioners, chairs, Q&A speakers).
- Remaining: wire minister/government title resolution; treat `N .` commission placeholders as vacancies rather than alias candidates; extend resolver to new Step 4 sources as they land.

**2. Normalize the high-value edges you already have (no new scraping yet). Done and working**

- `just normalize-edges` routes vote roll-calls, document authors, commission chairs, questioners, and Q&A discussion speakers through the identity resolver into `data/normalized/`.
- Unresolved names aggregate to `data/normalized/unresolved_persons.parquet`; vote reconciliation is in `vote_reconciliation.parquet` (1 mismatch: `56_129_4`).
- **Remaining in this step:** minister/government resolution (portfolio-title→person-over-time table and `ANSWERED` edges). Utterance ids derive from scoped `question_id` (`{question_id}_{seq}`); re-run normalize after the 2026-07-02 question-id fix to confirm plenary/commission collisions are gone.

**3. Stand up the graph builder early (not last). Done and working**

- `just build-graph` emits deterministic `data/graph/nodes.parquet`, `edges.parquet`, and `source_artifacts.parquet` from identity, staging, and normalized outputs.
- Edges carry `source_artifact_id`, `source_url`, `cache_path`, and `confidence`; artifacts carry `parser_version`.
- **Remaining in this step:** populate `scraped_at` on artifacts; decide whether to add explicit `VoteCast` nodes (today CAST is Person→Vote); fix or flag orphan `VOTED_ON` targets (141 edges to dossier refs not in the graph).

**4. Add the missing parliamentary core sources — each behind the resolver + QA + graph edges.**

- ~~Full FLWB dossier discovery from the browse/full-text databank, not only ids seen in plenary refs.~~ **Done (2026-07-06):** `scrapers/dossiers` crawls `ListDocument.cfm?legislat={session}` and each `ListFromTo.cfm` range page, unions ids with plenary refs, then downloads `flwbn.cfm` fiches. **Next:** `just scrape-dossiers` → `just build-graph` to materialize new dossier/document nodes and reduce orphan `VOTED_ON` targets where full dossier ids were missing.
- Written Q&A from the QRVA bulletins (`/QRVA/pdf/{session}/…`) / search database.
- `Motion`, `Interpellation`, `Hearing`, and procedural `Notice` handling (INQO / report sources; the commission scraper currently drops hearings).
- Parse the dossier fiche calendar into `DISCUSSED_IN`, `SUBMITTED`, `AUTHORED`, and rapporteur/chair role edges.

**5. Now that the architecture exists, stand up QA over it.**

- **Interim:** `tools/graph-viewer` already surfaces vote reconciliation, unresolved persons, alias-style triage, and structural issue panels for manual inspection; promote the same checks into an automated `qa` binary.
- Create a `qa` binary that loads every parquet plus the graph output and runs structural + referential + domain checks: vote totals vs counted names, `meeting_id` foreign keys from questions/votes, document-id regex, dates inside the session window. The data is now stable enough to check properly.
- Wire the resolver outputs: `unresolved_persons` and `alias_candidates` on every run, plus a soft signal for any new `fraction` string not already in `parties`.
- Cross-check the normalized edges: every `VoteCast` person should be `MEMBER_OF` a party at the vote date — flag casts by non-members as oddities; include vote-total arithmetic in the issue report.
- Run the referential and domain checks over the actual graph output, not just staging, and add each source's row-count/delta check and unresolved-name bucket.

**6. Only then enrich.**

- Confirm the dossier "Analyse van de tussenkomsten" actually exists and is structured before planning around it; if so, ingest it before any NLP utterance tagging.
- Add media recordings once meeting/date/title matching is measurable (report the match rate).
- Wire the lobby register download and remuneration matching once person identity is stable; both rely on weaker name matching, so they must surface their match/unmatch rates in QA.

## Conventions

- Every edge carries `source_url`, `scraped_at`, and `confidence` (exact parse vs inferred vs NLP).
- `Person` resolution goes through `person_alias` before any `SPOKE` / `CAST` / `AUTHORED` edge is committed.
- Prefer site-native ids (FLWB doc id, question id, cvview key) over generated hashes.

