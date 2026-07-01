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
| **Person**                   | stable `person_id` from `cvview_key` + per-session mandate rows | Member list + [cvview/cvview54](https://www.dekamer.be/kvvcr/) detail pages                                                                                           | Hash-by-name is only a fallback; site gives `cvview*.cfm?key=` links. Name variants need alias table.                     |
| **Party**                    | slug / official name                                            | Member list, vote appendix, dossier authors                                                                                                                           | Fraction names drift; time-bounded membership required.                                                                   |
| **Commission**               | name / enum                                                     | [LstCom.cfm](https://www.dekamer.be/kvvcr/showpage.cfm?section=/none&language=nl&cfm=/site/wwwcfm/comm/LstCom.cfm)                                                    | Already scraped; link to meetings and dossier trajectories.                                                               |
| **Meeting**                  | `{session_id, kind, meeting_id}`                                | Plenary HTML `PCRI/ip{N}x.html`; commission HTML `CCRI/ic{N}x.html`                                                                                                   | Already scraped (metadata). Gaps in commission IDs.                                                                       |
| **AgendaItem**               | `{meeting_id, seq}`                                             | Meeting report headings (`h1`/`h2`, agenda numbers)                                                                                                                   | NL/FR pairs, mis-tagged `lang` attrs; section boundaries are heuristic.                                                   |
| **Utterance**                | `{meeting_id, seq}` or span ref                                 | Integraal verslag (plenary + commission); beknopt verslag; dossier PDFs                                                                                               | Q&A speaker blocks partially parsed into JSON blobs today. Full debates (`01.07 — Name (Party):`) are not normalized yet. |
| **Question**                 | internal id (`Q…P` / `Q…C`)                                     | Oral: meeting reports; written: QRVA bulletins / FLWB                                                                                                                 | Oral partially scraped. Written Q&A database not scraped.                                                                 |
| **Answer**                   | linked to Question                                              | Same as Question                                                                                                                                                      | Often merged into discussion text; minister ≠ MP entity resolution.                                                       |
| **Dossier**                  | `{session_id}/{number}`                                         | [flwbn.cfm](https://www.dekamer.be/kvvcr/showpage.cfm?section=/flwb&language=nl&cfm=/site/wwwcfm/flwb/flwbn.cfm) and static `/flwb/html/{session}/N/{doc}.html` pages | Already scraped only for dossier ids discovered from plenary refs; full FLWB browse/search discovery is still missing.    |
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
| `SPOKE`             | Person → Utterance                 | Integraal verslag speaker lines                     | Core gap: no `person_id` link, no general-debate extraction.   |
| `PART_OF`           | Utterance → Meeting                | Parent report                                       |                                                                |
| `PART_OF`           | Utterance → AgendaItem             | Section context                                     | Requires reliable section parser.                              |
| `ASKED`             | Person → Question                  | Question header (“Vraag van …”)                     | Oral scraped; written not yet.                                 |
| `ANSWERED`          | Person → Question                  | Question header (“… aan minister X”)                | Respondents are title strings, not IDs.                        |
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
| **Canonicalized and working**              | Identity seed tables: Person, Party, Commission, party memberships, commission memberships, person aliases, and unresolved-person report; normalized edges (`vote_casts`, `authored`, `asked`, `holds_role`, `utterances`) and graph Parquet (`nodes`, `edges`, `source_artifacts`) |
| **Parsed but not normalized**              | General plenary debate utterances (outside Q&A `discussion` JSON), respondent/minister titles (`ANSWERED` not wired), commission role edges beyond chair                                              |
| **Parser exists but source is incomplete** | LobbyOrg                                                                                                                                                                                          |
| **Not scraped / not normalized**           | Written Question/Answer, general plenary Utterances, Motion, Interpellation/Hearing, InterventionAnalysis, MediaRecording, Meeting↔Dossier calendar, Beknopt verslag, full FLWB dossier discovery |

## Implementation state

Verified against the repo on 2026-07-01:

- **Done and working:** Stage 0 staging contract is documented in `STAGING.md`; the workspace has an `identity` crate and `just build-identity`; identity tests pass; `cargo run --bin identity` writes `data/identity/persons.parquet`, `parties.parquet`, `person_aliases.parquet`, `commissions.parquet`, `memberships.parquet`, `unresolved_persons.parquet`, and `unresolved_report.md`.
- **Current identity quality:** 175 persons, 13 parties, 175 party memberships, 1127 resolved commission memberships, 21 unresolved commission-member occurrences. The only unresolved raw name currently reported is the placeholder `N .`, so real commission-member names are resolving.
- **Stage 0 QA is working as a soft report:** `data/qa/summary.md` reports 97 passes and 2 issues: generated commission-question data still shows the old `dossier_ids` mislabel signal, and one plenary vote total mismatch remains in meeting 129. Regenerated staging passes the normalize staging check (`internal_ids` present, no `dossier_ids`); re-run Stage 0 QA to clear the stale schema warning.
- **Step 2 is working:** `just normalize-edges` routes vote roll-calls, document authors, questioners, commission chairs, and Q&A discussion speakers through the identity resolver into `data/normalized/*.parquet` (189 400 vote casts, 1 851 authored, 2 693 asked, 67 holds_role, 7 975 utterances; 870 unresolved names, mostly speakers and respondent titles). One vote reconciliation mismatch remains (`56_129_4`, same as Stage 0 QA). Minister/government resolution and `ANSWERED` edges are not started yet — respondent titles only land in `unresolved_persons`.
- **Step 3 is working:** `just build-graph` writes `data/graph/nodes.parquet` (10 652 nodes), `edges.parquet` (215 991 edges: MEMBER_OF, CAST, AUTHORED, ASKED, HOLDS_ROLE, SPOKE, PART_OF, VOTED_ON, SUBMITTED, TAGGED_WITH), and `source_artifacts.parquet` (660 artifacts). Referential integrity is clean on `from` endpoints; 141 `VOTED_ON` edges point at dossier ids not present as nodes (partial vote-title refs like `1-5`, not full `56/N` dossiers). Edges carry `source_artifact_id`, `source_url`, `cache_path`, and `confidence`; `scraped_at` on artifacts is still empty. CAST is Person→Vote (no separate VoteCast node).
- **Not done yet:** minister/government resolution, graph-level QA (Step 5), and the missing parliamentary sources in Step 4.


## Scrutiny

This is a good target model, but it should be treated as a graph normalization layer above the current scrapers, not as a replacement for the raw/staging outputs. The Chamber site is old, multilingual and inconsistent; keeping raw HTML/PDF provenance beside every normalized edge is not optional.

What is strong:

- The core spine is right: `Person` ↔ `Meeting` ↔ `Utterance` ↔ `Question` / `Vote` ↔ `Dossier` / `Document`.
- The plan correctly prefers site-native ids over generated hashes. Live member and dossier pages expose `cvview*.cfm?key=...`, FLWB document ids, dossier ids, question ids and PDF paths.
- The emphasis on aliasing and confidence is necessary. The code already has typo maps, name reordering, reversed email strings, mis-tagged `lang` attributes and CSV person lists.

What is missing:

- A first-class `Membership` / `Mandate` shape. Party and commission membership need `role`, `start_date`, `end_date`, `source`, `active`, and sometimes replacement/permanent status; an edge with only a time range will become too thin.
- `Motion`, `Interpellation`, `Hearing`, and probably `Notice` / procedural agenda entries. The plenary scraper already extracts propositions and notices, vote parsing references `motion_id`, and the commission scraper deliberately skips hearings.
- A source/provenance table for raw artifacts: report HTML, dossier HTML, PDF, converted markdown, search result page. Edge-level `source_url` is good, but reproducible graph building needs `source_artifact_id`, parser version and extraction timestamp.
- A canonical bilingual text strategy. Many entities have NL and FR titles/topics; some source `lang` attributes are wrong. The graph should keep language-tagged text variants rather than picking one string per entity.
- Validation gates: row counts, referential integrity, unmatched names, duplicate site ids, and expected deltas per scrape run.
- Stable, site-native ids for `Question` and `Vote`. Today `question_id` / `vote_id` are sequential integers assigned per scrape run, and the plenary scraper re-scrapes every meeting `1..=last` on each run, so the ids are not reproducible. Graph nodes need ids derived from the source (meeting + agenda/vote number, or the site's own reference) or every edge breaks on the next scrape.
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

Step 0 is done at the contract/source level, Step 1 is working for the first identity slice, and Steps 2–3 are working end-to-end (`just normalize-edges` → `just build-graph`). Continue at Step 4 (missing sources) and Step 5 (graph-level QA). Re-run Stage 0 QA once to clear the stale `dossier_ids` warning.

**0. Freeze the staging contract. Done in contract/source; data regeneration check pending**

- Write down every current Parquet schema exactly as produced, then fix the known mislabels: commission meetings write the meeting id under `commission_id` (rename to `meeting_id`); commission `questions.dossier_ids` actually holds `Q…` question refs, not FLWB dossier numbers; replace per-run sequential `question_id` / `vote_id` with ids derived from meeting + agenda/vote number so they survive a re-scrape.
- Add `source_url` + cache-path columns wherever raw HTML/PDF provenance is not yet addressable from staging rows.

**1. Canonical identity + one resolver everything routes through. Partly done / working**

- Build `persons` (seeded from `cvview*.cfm?key=`), `person_aliases` (seed from the existing questioner typo map and the "Last First"→"First Last" reorder rules), `parties`, `commissions`, and `memberships` (role, start/end, permanent vs replacement, source, active).
- Expose a single `resolve_person(raw_name, context) -> person_id | Unresolved` used by every downstream transform. No `SPOKE` / `CAST` / `AUTHORED` edge is committed for an `Unresolved`.
- Current gap: the resolver exists and identity tables build, but most downstream scrapers/transforms do not consume it yet. Treat `N .` commission placeholders as vacancies or source placeholders rather than alias candidates.

**2. Normalize the high-value edges you already have (no new scraping yet). Done and working**

- `just normalize-edges` routes vote roll-calls, document authors, commission chairs, questioners, and Q&A discussion speakers through the identity resolver into `data/normalized/`.
- Unresolved names aggregate to `data/normalized/unresolved_persons.parquet`; vote reconciliation is in `vote_reconciliation.parquet` (1 mismatch: `56_129_4`).
- **Remaining in this step:** minister/government resolution (portfolio-title→person-over-time table and `ANSWERED` edges); utterance ids can collide when the same question appears in both plenary and commission extracts (~1 273 duplicate `utterance_id`s today).

**3. Stand up the graph builder early (not last). Done and working**

- `just build-graph` emits deterministic `data/graph/nodes.parquet`, `edges.parquet`, and `source_artifacts.parquet` from identity, staging, and normalized outputs.
- Edges carry `source_artifact_id`, `source_url`, `cache_path`, and `confidence`; artifacts carry `parser_version`.
- **Remaining in this step:** populate `scraped_at` on artifacts; decide whether to add explicit `VoteCast` nodes (today CAST is Person→Vote); fix or flag orphan `VOTED_ON` targets (141 edges to dossier refs not in the graph).

**4. Add the missing parliamentary core sources — each behind the resolver + QA + graph edges.**

- Full FLWB dossier discovery from the browse/full-text databank, not only ids seen in plenary refs.
- Written Q&A from the QRVA bulletins (`/QRVA/pdf/{session}/…`) / search database.
- `Motion`, `Interpellation`, `Hearing`, and procedural `Notice` handling (INQO / report sources; the commission scraper currently drops hearings).
- Parse the dossier fiche calendar into `DISCUSSED_IN`, `SUBMITTED`, `AUTHORED`, and rapporteur/chair role edges.

**5. Now that the architecture exists, stand up QA over it.**

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

