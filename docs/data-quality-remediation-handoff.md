# Data quality remediation handoff

## Purpose

This document turns the July 2026 source-to-Parquet review into implementation work for a follow-up agent. It is a handoff only: no production fix described below was applied while writing it.

The affected pipeline is:

```text
raw HTML/XML/PDF cache
  -> staging Parquet
  -> identity
  -> normalized relations
  -> graph Parquet
  -> advisory QA
  -> graph viewer
```

Follow the repository pipeline order after any staging change:

```text
just build-identity
just normalize-edges
just build-graph
just qa
```

`just qa` must remain advisory. Deterministic defects may be represented by `status=fail`, but ordinary findings must not make the default command return nonzero. `just qa-strict` remains the optional regression gate.

## Snapshot

The counts below were rechecked against the reparsed data on 2026-07-14. `data/qa/summary.md` was still generated before that reparse, on 2026-07-13 at 20:27 UTC, so direct Parquet queries are authoritative for this snapshot.

| Area | Current result | Disposition |
|---|---:|---|
| Commission question prefixes | 0 / 6,289 | Verified fixed by reparse |
| Commission questions without resolved `ASKED` | 0 / 6,289 | Verified fixed by reparse |
| Unresolved commission questioners | 0 | Verified fixed by reparse |
| Published QRVA answers with no text | 622 rows / 579 questions | Fix parser and add QA |
| Interpellation utterances with noncanonical `item_id` | 287 / 647 rows, 20 IDs | Fix IDs and add QA |
| Remunerations above EUR 1,000,000 | 1,946 / 6,977 | Fix decimal parsing and add QA |
| Remuneration logical duplicate groups | 176 | Preserve evidence; add QA |
| Lobby interests containing URL fragments | 212 / 299 | Fix PDF parsing and add QA |
| Chair/subchair overlaps | 0 / 35 commissions | Verified fixed by reparse |
| Graph artifacts with blank `scraped_at` | 15,050 / 15,050 | Fix provenance |
| Current commission source gaps | 8 | Plan source completeness work |

## Recommended order

| Order | Work package | Reason |
|---:|---|---|
| 1 | QRVA answer extraction | Confirmed silent loss of published answer bodies |
| 2 | Interpellation canonical IDs | Confirmed normalized referential defect |
| 3 | Remuneration, lobby, and commission role parsers | Confirmed source-to-staging corruption |
| 4 | QA checks for packages 1-3 | Lock in each repaired edge case |
| 5 | Entity-level warning contract and graph-viewer display | Preserve real source contradictions for consumers |
| 6 | Normalized provenance | Make warnings and graph facts auditable against transform-time source content |
| 7 | Source completeness and freshness | Larger structural change; implement from the phased plan below |

Do not update the QA baseline until the new checks have been reviewed against regenerated data.

---

## 1. Commission question identity after reparse

### Status

Verified resolved. No parser repair remains for the originally reported issue.

The reparsed snapshot has:

```text
commission questions                         6,289
questioners containing "Vraag van"/"Question de"  0
questions without a resolved ASKED relation       0
unresolved questioner occurrences                 0
```

The cleanup is implemented in `scrapers/crawl/src/oral_questions.rs:57-76`, where the agenda number and question-label prefix are removed before identity resolution.

### TODO

- [x] Add a regression assertion to QA so a future stale or regressed staging build cannot look healthy only after manual inspection.
- [x] Use check ID `question.questioner_resolved` or extend the existing unresolved-person check with a per-question cardinality assertion.
- [x] Require every commission question to have at least one resolved `ASKED` relation unless the staging questioner field is explicitly empty and separately warned.
- [x] Include `question_id`, raw questioner field, source URL, and cache path in the detail row.
- [x] Add a fixture based on commission report 157, where headings begin with forms such as `02 Vraag van Marijke Dillen`.

### Acceptance criteria

- The reparsed data produces zero findings.
- Reintroducing a numbered `Vraag van` or `Question de` prefix in a fixture produces a finding before graph construction.
- A question cannot pass merely because another question in the same meeting resolves.

---

## 2. Published QRVA answers lose text around nested XML elements

### Confirmed defect

`data/sessions/56/written/answers.parquet` contains 622 `publicated` answer rows with both `text_nl` and `text_fr` empty. They belong to 579 logical questions.

The source XML is not empty. For example:

```text
route:       56_qrva_293066
cache:       sessions/56/qrva/detail/56-B001-3-0001-0000202400004.xml
source NL:   3,783 normalized characters
source FR:   3,913 normalized characters
Parquet NL:  0 characters
Parquet FR:  0 characters
```

The answer bodies are visible at `cache/sessions/56/qrva/detail/56-B001-3-0001-0000202400004.xml:39-44`.

### Root cause

`parse_qrva_xml` keeps only one `current_tag`. At `scrapers/qrva/src/xml.rs:19-28`, any nested start element other than `br` replaces the outer field name and clears accumulated text. Actual QRVA answer bodies contain inline elements such as `<a>`. Closing that element stores an `a` field, clears parser state, and causes the remainder and closing `TEXTA1N`/`TEXTA1F` event to be ignored.

The current fixture at `scrapers/qrva/src/xml.rs:102-128` covers `<br>` but no nested formatting or links.

### Parser TODO

- [x] Replace the single-tag state with outer-field state plus nested depth, or an explicit stack.
- [x] Treat direct children of `QRVADOC` as field boundaries.
- [x] Accumulate all descendant text while a field is open.
- [x] Preserve `<br>` as a line break.
- [x] Do not create output fields for inline tags such as `a`, `b`, `i`, `strong`, or `span`.
- [x] Close and store a field only when the matching direct child of `QRVADOC` closes.
- [x] Preserve existing whitespace normalization unless a fixture proves it loses meaningful separation.
- [x] Add a regression fixture with an `<a>` inside `TEXTA1N` and `TEXTA1F`.
- [x] Reference `56-B001-3-0001-0000202400004.xml` in the test comment as the originating document.
- [ ] Reparse QRVA staging after the fix.
- [ ] Rebuild normalized answers and the graph.

### QA TODO

- [x] Add `written.published_answer_text_present` in `scrapers/qa/src/written.rs`.
- [x] Register the check in `registered_check_ids()` and `check_catalog.rs`.
- [x] Mirror it in `tools/qa-triage/qa_triage/check_catalog.py` and `code_pointers.py`.
- [x] Read staging `written/answers.parquet`, because it retains answer slot and publication metadata.
- [x] Emit one detail when `kind=written`, `source_kind=qrva`, status is `publicated`/`published`, and both language texts are blank.
- [x] Use `entity_type=Answer` and `entity_id=answer_id`.
- [x] Carry route, slot, publication reference, source URL, and cache path in the detail.
- [x] Use deterministic failure semantics: `severity=error`, `status=fail`. Default `just qa` must still exit 0.

### Tests

- `publicated` plus both blank -> finding.
- Mixed-case `published` plus whitespace-only text -> finding.
- Non-published lifecycle state plus blank -> no finding.
- Either NL or FR populated -> no finding.
- [x] Inline oral-written answer -> not evaluated by this QRVA-specific check.
- [x] Nested `<a>` text survives XML parsing.
- [x] Multiple nested inline elements do not split or overwrite the field.

### Acceptance criteria

- The current 622 blank published rows become zero after reparse.
- The sample route retains substantial NL and FR answer text.
- Answer row count does not drop merely to make QA pass.
- Every published answer remains linked to its route and logical question.

---

## 3. Deliberate exclusion of constitutive and procedural plenary text

### Policy decision

Do not expand utterance extraction solely to capture constitutive formalities, oath wording, credentials verification, ceremonial tributes, institutional appointments, or administrative communications. These documents remain available as cached HTML and `report_blocks.parquet`; they are deliberately not all modeled as political speech.

This is a corpus policy, not a parser failure.

### Fully or primarily constitutive reports

| Meeting | Date | Source | Contents | Coverage treatment |
|---:|---|---|---|---|
| 1 | 2024-07-04 | `cache/sessions/56/meetings/plenary/56-1.html` | Opening extraordinary session; verification of credentials; agenda adoption | Mark `constitutive`; no speech-coverage warning required |
| 2 | 2024-07-10 | `cache/sessions/56/meetings/plenary/56-2.html` | Credentials reports for electoral districts, admission votes, constitutional oaths, voting-system instructions | Mark `constitutive`; current 0.489 ratio is expected |
| 3 | 2024-07-16 | `cache/sessions/56/meetings/plenary/56-3.html` | Two-minute admission, credentials verification, and oath report | Mark `constitutive`; current 0.335 ratio is expected |
| 4 | 2024-07-18 | `cache/sessions/56/meetings/plenary/56-4.html` | Admissions and oaths, two funeral tributes, group/Bureau/commission appointments, administrative communications | Mark `constitutive_administrative`; current 0.237 ratio is expected by policy |

Meeting 2 does not use normal `h2` structure for its main agenda. Its report blocks explicitly contain `Onderzoek van de geloofsbrieven en eedafleggingen` / `Vérification des pouvoirs et prestations de serment`, so classification must not rely only on heading tags.

### Mixed reports containing oath or credentials agenda items

These reports contain a formal oath/credentials item but must not be exempted as whole documents:

```text
7, 15, 24, 35, 63, 69, 93, 119
```

Meeting 24 is the clearest counterexample. It contains successor oaths, but also the government declaration and confidence motion. Its low 0.644 speech-coverage ratio must continue to be reviewed rather than suppressed as procedural.

Meeting 22 contains a political question about President Trump's oath. It is not a parliamentary-oath document and must not be classified by a naive keyword search.

### Other low speech-coverage shapes

| Meeting | Shape | Policy |
|---:|---|---|
| 67 | Opening ordinary session, Bureau and commission appointments, chair address | Classify separately as `session_opening`; review whether chair address is in scope before suppressing |
| 79 | Vote-dominated sitting with confidence and motion votes | Speech coverage is not the right completeness metric; vote QA remains mandatory |

### Documentation TODO

- [x] Create `docs/meeting-report-corpus-policy.md` from this section.
- [x] Document that raw reports and report blocks remain canonical evidence even when text is intentionally not promoted to Utterance.
- [x] Document the difference between whole-report classification and isolated procedural agenda items.
- [x] Add the document to `README.md` near the meeting-report pipeline description.

### QA TODO

- [x] Add one central, explicit corpus-classification catalog rather than hardcoding `meeting_id <= 4` in the coverage calculation.
- [x] Catalog at least `constitutive`, `constitutive_administrative`, `mixed`, `session_opening`, and `vote_dominated`.
- [x] Downgrade expected low speech coverage for meetings 1-4 to `info` with a policy reference.
- [x] Do not suppress meeting 24.
- [x] Keep vote, agenda, source-span, and cache checks active for every classified report.
- [x] Add a QA test proving that a mixed report with one oath heading still receives normal speech-coverage evaluation.
- [x] Add a negative fixture based on meeting 22 so ordinary political use of the word “oath” does not trigger corpus classification.

### Acceptance criteria

- Meetings 2, 3, and 4 no longer appear as unexplained speech-loss warnings.
- Their raw HTML and report blocks remain queryable.
- Meeting 24 remains visible as a low-coverage mixed report.
- Meeting 79 is evaluated by vote completeness rather than treated as successfully covered because it has few speeches.

---

## 4. Interpellation utterances use noncanonical item IDs

### Confirmed defect

`data/normalized/utterances.parquet` contains 647 interpellation utterances. Of these, 287 rows reference 20 `item_id` values that do not exist in combined plenary/commission interpellation staging.

Every current bad ID can be mapped uniquely through the site-native interpellation reference in `question_ids`, but the normalized FK itself is wrong. Examples:

| Meeting | Actual `item_id` | Site ref | Canonical interpellation ID |
|---:|---|---|---|
| 34 | `56_plenary_34_9` | `56000027I` | `56_plenary_34_4` |
| 51 | `56_plenary_51_17` | `56000092I` | `56_plenary_51_13` |
| 69 | `56_plenary_69_5` | `56000158I` | `56_plenary_69_2` |
| 97 | `56_plenary_97_20` | `56000242I` | `56_plenary_97_14` |
| 118 | `56_plenary_118_7` | `56000268I` | `56_plenary_118_3` |

The timeline assigns sequence-based IDs in `scrapers/crawl/src/agenda_timeline.rs:263-317` and `:393-408`. The graph currently compensates with site-ref and single-candidate fallbacks in `scrapers/graph/src/build.rs:1283-1326`. That fallback must not conceal staging/normalized ID drift.

### Parser/normalization TODO

- [ ] Add a minimal fixture from plenary 69 containing the bilingual/grouped interpellation shape.
- [x] Trace where the same logical interpellation receives different sequence positions between timeline utterances and proceeding entities.
- [x] Pair bilingual headings by site-native `...I` reference before assigning sequence IDs.
- [x] Assign one canonical interpellation ID per logical site reference.
- [x] Ensure utterances and `interpellations.parquet` receive the same ID from the same `AgendaItem` instance.
- [x] Do not repair this only in graph loading.
- [x] Remove or narrow the graph’s single-candidate fallback after canonical data is regenerated.
- [ ] Reparse plenary reports, normalize utterances, and rebuild the graph.

### QA TODO

- [x] Add `fk.utterance_interpellation` in `scrapers/qa/src/agenda_checks.rs`.
- [x] Require each interpellation utterance to resolve to exactly one interpellation in the same session, meeting kind, and meeting.
- [x] Accept direct canonical ID as the normal path.
- [x] Use site-native refs only to diagnose the expected target, not to silently pass a wrong `item_id`.
- [ ] Add `utterance.interpellation_item_id_canonical` if a separate warning is useful during migration.
- [x] Group details by distinct bad reference rather than emitting 287 repetitive turn-level rows.
- [x] Include sample utterance ID, actual ID, canonical ID, site ref, source block range, URL, and cache path.
- [x] Use `status=fail` for missing/ambiguous targets and `status=warn` for uniquely resolvable but noncanonical IDs.

### Tests

- [ ] Direct canonical ID passes.
- [ ] Wrong ID plus one matching site ref produces the canonical-ID finding.
- [ ] No matching site ref produces an FK failure.
- [ ] Two candidates for one site ref produce an ambiguity failure.
- [ ] Candidate in another meeting or meeting kind does not satisfy the FK.
- [ ] Repeated utterances with one bad reference produce one grouped detail.

### Acceptance criteria

- All 647 current interpellation utterances use an existing canonical `item_id`.
- Both hard FK and noncanonical-ID checks reach zero after reparse.
- Graph construction no longer needs a same-meeting single-candidate guess.

---

## 5. Remuneration amounts and logical duplicates

### Confirmed defect

The source displays European-formatted amounts such as:

```text
279 463,46 EUR
```

The staging value is:

```text
27946346
```

The parser removes commas at `scrapers/remunerations/src/main.rs:252-256` before attempting decimal conversion. The value is therefore interpreted as cents while the staging contract says EUR.

Current impact:

```text
rows                           6,977
amounts above EUR 1,000,000    1,946
maximum amount                 27,946,346
logical duplicate groups         176
extra rows in those groups        176
```

`dedupe_remunerations` at `scrapers/remunerations/src/main.rs:232-244` includes amount values in the key, so two rows with the same person/year/mandate/institute but different amounts both survive.

### Parser TODO

- [ ] Parse locale amounts without deleting the decimal separator.
- [ ] Remove currency symbols and grouping whitespace/nonbreaking spaces.
- [ ] Treat comma as the decimal separator for current source values.
- [ ] Parse ranges only after normalizing each endpoint independently.
- [ ] Keep `Niet bezoldigd` as exact zero.
- [ ] Store canonical decimal EUR, not cents encoded as integer-looking strings.
- [ ] Decide and document an Arrow numeric type. Prefer `DECIMAL` if supported consistently; otherwise use `FLOAT64` with explicit currency/unit documentation.
- [ ] Add tests for `279 463,46 EUR`, `1,00 - 6 129,00 EUR`, unpaid values, malformed text, and ranges.
- [ ] Reference `remunerations/Clarinval-David-2024.html` in the large-value fixture comment.
- [ ] Reparse the full remuneration table.

### Duplicate TODO

- [ ] Investigate the 176 groups against source rows before deleting anything.
- [ ] Define the business key as normalized person, year, mandate, and institute.
- [ ] Determine whether duplicate rows represent source corrections, date segments, or parser duplication.
- [ ] If source records are semantically distinct, add a site-native occurrence/record identifier or period fields.
- [ ] If they are duplicate renderings, deduplicate only after preserving enough evidence to prove equivalence.
- [ ] Do not select an arbitrary minimum or maximum amount.

### QA TODO

- [ ] Add `remuneration.amount_valid`: numeric, finite, nonnegative, and `min <= max`.
- [ ] Add `remuneration.amount_scale`: conservative warning when annual maximum exceeds EUR 1,000,000.
- [ ] Add `remuneration.duplicate_mandate`: one warning per logical duplicate group, listing all amount ranges.
- [ ] Add `remunerations.parquet` to schema/table-loaded QA.
- [ ] Carry person name, year, mandate, institute, values, URL, and cache path in details.

### Acceptance criteria

- `279 463,46 EUR` becomes approximately `279463.46`, not `27946346`.
- No valid source amount is silently blanked.
- Scale warnings fall to zero or to reviewed genuine exceptions.
- Every remaining duplicate group has an explicit semantic explanation or stable occurrence ID.

---

## 6. Lobby PDF column bleed

### Confirmed defect

The cached PDF is a 27-page landscape register. `pdftotext -layout` produces readable columns, but `lobby.parquet` mixes fields. For Agoria, contact, interest, and URL fragments are interleaved. Across the table, 212 of 299 `interests` values contain `www.` fragments.

The parser uses fixed byte/character cuts at `scrapers/lobby/src/main.rs:182-205`, merges continuation rows at `:208-225`, and then finds a URL across the whole raw entry at `:227-234` without removing URL fragments from other fields.

### Parser TODO

- [x] Add source fixtures copied from the PDF layout for Agoria, Air Cargo Belgium, A&T Efficiency, and at least one accented contact.
- [x] Reference `cache/lobby/lobbyregister.pdf` and the source row in fixture comments.
- [x] Replace fragile byte cuts with character-position or whitespace-gap column detection.
- [x] Detect the header’s actual column boundaries where possible rather than assuming one global byte layout.
- [x] Preserve wrapped contacts, interests, and URLs in their originating column.
- [x] Treat a nonempty first column as a new organisation only when the line aligns with the organisation column.
- [x] Normalize one or more wrapped URL lines into the URL field.
- [x] Remove URL tokens from contact/interest output only when they were positively classified as URL-column content.
- [x] Do not use longest-field merging across duplicate organisations unless all fields come from the same logical source entry.
- [x] Reparse `lobby.parquet` and manually compare the fixture organisations to the PDF text.

### QA TODO

- [x] Add `lobby.url_placement` for URL/domain tokens outside the URL column or prose/multiple URLs inside the URL column.
- [x] Add `lobby.column_bleed` for truncated URL fragments in contact/interests that match a prefix of the canonical URL.
- [x] Include organisation, offending field/token, source URL, and PDF cache path.
- [x] Add source-side parser diagnostics if broader linguistic bleed cannot be detected from final Parquet.

### Acceptance criteria

- The named fixture organisations match the `pdftotext -layout` source columns.
- URL fragments no longer appear in `interests` for correctly parsed rows.
- Accented names do not shift column boundaries.
- Wrapped multi-line interests and contacts remain complete.

---

## 7. Commission subchairs are also parsed as chairs

### Confirmed defect

The Justice source page separates:

```text
Voorzitter(s): Ismaël Nuino
Ondervoorzitters: Steven Matheï, Kristien Van Vaerenbergh
```

See `cache/commissions/details/justitie.html:419-437`.

Staging records all three as chairs. `extract_members` at `scrapers/commissions/src/main.rs:196-220` checks whether the bold label contains the requested role. `Ondervoorzitters` therefore matches `Voorzitter`.

Current impact is 51 overlapping names across 25 of 35 commissions.

### Parser TODO

- [x] Normalize the first bold label to a canonical role token.
- [x] Match complete role labels, not substrings.
- [x] Parse `Voorzitter(s)` and `Ondervoorzitters` independently.
- [x] Add fixtures for Justice and one commission without subchairs.
- [x] Reference the source detail page in test comments.
- [x] Re-scrape or reparse `commissions.parquet`.
- [x] Rebuild identity memberships and the graph.

### QA TODO

- [x] Add `commission.chair_subchair_overlap`.
- [x] Split, trim, and case-fold both lists, then emit one detail per overlapping person.
- [x] Use `status=fail`; these roles are semantically disjoint in the source layout.
- [x] Add `commissions.parquet` to schema/table-loaded QA.
- [x] Preserve accents when comparing names.

### Acceptance criteria

- Justice has exactly one chair and two subchairs.
- All 51 current overlaps disappear after reparse.
- Empty role lists do not produce empty-name findings.

---

## 8. Entity-level data warnings for downstream consumers

### Goal

Real source contradictions and known data anomalies must travel with the affected graph entity. The graph viewer must show the warning where a user consumes the fact, rather than requiring them to inspect a global QA page.

Examples include:

- Vote result `56-135-r16`, where compact and appendix claims disagree.
- Vote result `56-135-r21`, where headline count and retained names disagree.
- Dossiers whose source page reports submission after vote/end dates.
- Future source dates such as subdocument `56K1489004` with `30/06/2029`.

Warnings must preserve source claims. They must not “correct” source values without authoritative evidence.

### Architecture decision

Use `data/qa/meeting_report_check_details.parquet` as the canonical warning store and extend its entity-target contract. Do not add warning columns to every graph node/edge, and do not make graph construction depend on QA. The current order is graph first, QA second.

Current integration points:

- QA detail type: `scrapers/qa/src/types.rs:3-93`
- QA detail writer: `scrapers/qa/src/io.rs:9-55`
- Viewer QA view: `tools/graph-viewer/app/db.py:316-334`
- Global issues API/query: `tools/graph-viewer/app/queries/issues.py`
- Node detail query: `tools/graph-viewer/app/queries/node_detail.py:52-91`
- Node detail route: `tools/graph-viewer/app/routes/api.py:133-135`
- Node detail rendering: `tools/graph-viewer/app/static/app.js:820-864`

### Warning schema TODO

- [x] Extend `CheckDetail` and QA Parquet with `warning_id`.
- [x] Make `warning_id` deterministic over check, subject, values, artifact, and source block; exclude `created_at`.
- [x] Add `warning_kind` with a closed initial vocabulary:

```text
source_conflict
source_anomaly
source_gap
extraction
integrity
coverage
```

- [x] Add exact `graph_node_type` and `graph_node_id` fields.
- [x] Add canonical `source_artifact_id`.
- [x] Keep existing `entity_type`/`entity_id` as the check subject, which may be a source-local occurrence rather than a graph node.
- [x] Add builders such as `with_graph_node()` and `with_warning_kind()`.
- [x] Compute artifact IDs through `crawl::artifact_id`; do not duplicate hashing logic.
- [x] Validate every nonempty graph target against `graph/nodes.parquet`.
- [x] Validate every nonempty source artifact against `graph/source_artifacts.parquet`.
- [x] Update `STAGING.md` with the warning contract.

### Warning production TODO

- [x] Target vote reconciliation/source contradictions to `VoteResult`, not a source-local appendix number.
- [x] Let a `Vote` inherit warnings from its `HAS_RESULT` target in the viewer.
- [x] Preserve reused-result semantics: all Votes using one result see the same warning once.
- [x] Add `dossier.date_chronology` and target canonical `Dossier:{session}/{id}`.
- [x] Classify source-backed impossible chronology as `source_anomaly`, not extraction failure, unless parser field scoping is proven wrong.
- [x] Include both independently observed vote claims in `expected`/`actual` or structured warning properties.
- [x] Add source block ranges to unresolved vote events; current QA drops available `block_start`/`block_end` in `scrapers/qa/src/vote_source.rs:778-800`.
- [x] Fix numeric dossier-reference QA while touching this area; current normal bare dossier IDs must be normalized to `{session}/{id}` rather than skipped.

### Viewer TODO

- [x] Extend the typed `qa_details` view in `tools/graph-viewer/app/db.py`.
- [x] Add a `DataQualityWarning` API model.
- [x] Add `data_quality_warnings` to node detail responses.
- [x] Query actionable rows by exact graph type and ID; do not parse messages to infer navigation.
- [x] For Vote nodes, include warnings attached to linked VoteResult nodes.
- [x] Deduplicate by `warning_id`.
- [x] Render warning cards immediately below the node header and above previews/vote details.
- [x] Display severity, warning kind, check ID, message, both claims, and source navigation.
- [x] Label `source_conflict` as conflicting source claims rather than declaring one correct.
- [x] Keep missing QA safe: node detail should return an empty warning list.
- [x] Prefer this shared warning payload over the vote view’s current one-off mismatch label.
- [x] Do not add warning aggregation to every search result in the first implementation.

### Tests

- Direct Dossier warning appears once.
- VoteResult warning appears directly.
- Linked Vote inherits the result warning.
- Two reused Votes inherit one shared result warning each.
- Unrelated nodes have no warnings.
- `info` rows are not shown as actionable warnings.
- Missing QA Parquet does not break node detail.
- A warning with an invalid graph target fails QA.
- Source-local IDs such as `1#1` are never presented as graph IDs.

### Acceptance criteria

- Opening `VoteResult/56-135-r16` displays the source conflict and source location.
- Opening a Vote linked to that result displays the same warning exactly once.
- Opening a dossier with source chronology anomalies displays a warning without changing its source values.
- Downstream consumers need no check-specific ID parsing.

---

## 9. Source completeness, cache safety, and freshness plan

### Scope

This is a plan only. Implement it as a dedicated structural change after the parser fixes above.

Current risks:

- Cache-only reparses can skip missing artifacts and overwrite complete Parquet with partial output.
- Commission has eight visible gaps; plenary has no equivalent gap manifest.
- Mutable session/member/commission/lobby/remuneration sources are reused indefinitely once cached.
- Dossier terminal/fingerprint logic can miss output-relevant changes.
- Several scrapers log per-item errors and still publish a smaller snapshot.

### Absence policy

| Condition | Required behavior |
|---|---|
| Top-level index/list/archive/PDF is missing, malformed, throttled, or unavailable | Abort before publishing Parquet |
| Cache-only run lacks its expected source manifest | Abort |
| Cache-only run lacks an artifact previously known as present | Abort and preserve prior outputs |
| Interior meeting URL is confirmed 404 | Record explicit `not_found` gap |
| Trailing 404 used to find discovery boundary | Stop signal only; not a gap |
| Meeting URL returns valid but unsupported PDF | Retain artifact and record `unsupported_format` |
| HTTP 200 expected HTML fails parser invariants | Abort; do not convert parser regressions into source gaps |
| Member/commission detail linked from authoritative index is missing | Abort |
| Regimand explicitly says no result | Record valid `no_result` |
| Any other non-404 network error | Abort |

Remote absence may be a declared gap. Local cache absence is an incomplete snapshot and must abort.

### Phase A: safe publication and source manifests

- [x] Add a small shared cache metadata helper under `scrapers/crawl`.
- [x] Store source URL, content type, raw SHA-256, `fetched_at`, and `checked_at` beside cached artifacts.
- [x] Do not mutate cache metadata in cache-only mode.
- [x] Write candidate cache/data files to temporary sibling paths and rename only after validation.
- [x] Publish each scraper’s output bundle atomically; preserve the previous canonical snapshot on failure.
- [x] Add canonical `data/source_manifests/<source>.parquet` files.
- [x] Include source, session, item kind, native item ID, URL, cache path, status, row count, content type/hash, timestamps, run mode, and detail.
- [x] Use manifest status values `parsed`, `no_result`, `not_found`, and `unsupported_format`.
- [x] Require a live scrape to establish the new manifest contract; add no compatibility shim for old caches.

### Phase B: meeting gap parity

- [x] Generalize the commission gap schema and add `plenary/meeting_gaps.parquet`, including a valid zero-row file.
- [x] Use the same columns for both kinds: session, kind, meeting ID, reason, detail, URL, cache, hash, fetched/checked timestamps.
- [x] Remove `parse_failed` as an accepted published source condition; parser failures abort.
- [x] Reconcile each ID through the confirmed discovery boundary to exactly one parsed row or accepted gap.
- [x] Require parsed and gap ID sets to be disjoint.
- [x] Publish meeting tables, child entities, derived blocks/spans, dossier ID discovery, and gap manifest as one bundle.

Current commission gaps should be reclassified as:

```text
not_found:          67, 101, 106, 263, 398, 414
unsupported_format: 117, 160
```

Plenary currently has complete cached and parsed IDs 1-135, so its first gap file should be empty.

### Phase C: refresh mutable sources

- [ ] In live mode, refresh/verify session, member, commission, lobby, and remuneration sources even when cache files exist.
- [ ] Reserve no-network behavior for `SCRAPER_CACHE_ONLY=1`.
- [ ] Members: build expected native keys from active/all indexes and require one detail/result per key.
- [ ] Commissions: require output keys to equal index keys.
- [ ] Lobby: validate HTTP success and PDF signature before replacing the previous cache; parse candidate before promotion.
- [ ] Remunerations: separate browser fetching from cache parsing, require the complete member/year query matrix, and distinguish explicit no-result from missing cache.
- [ ] Include the latest completed year rather than ending permanently at 2024.

### Phase D: dossier freshness

- [ ] Use the authoritative discovered dossier ID set, not directory scanning, as cache-only inventory.
- [ ] Remove permanent exemption for terminal dossiers; use a finite slower refresh interval.
- [ ] Separate `checked_at` from raw-file mtime.
- [ ] Include every output-relevant field in the fingerprint: title, authors, dates, type, status, Eurovoc, and all subdocument IDs/dates/types/authors/file URLs.
- [ ] Sort multi-value fields before hashing so source-order-only changes are stable.
- [ ] Retain old raw versions instead of deleting them.
- [ ] Select the newest cached version deterministically.
- [ ] Use timestamp/content-hash suffixes to avoid same-day collisions.
- [ ] Require discovered IDs, manifest IDs, dossier rows, and subdocument FKs to reconcile before publication.

### QA TODO

- [x] Generalize `commission.meeting_gaps` to both meeting kinds.
- [x] Add `source.manifest_complete`.
- [x] Add `source.cache_metadata` for cache existence/hash and timestamp ordering.
- [ ] Add `source.freshness` against documented intervals.
- [x] Fail QA details for duplicate manifest keys, unknown statuses, or incomplete inventories.
- [x] Keep accepted source gaps as `info`.
- [x] Add commissions, remunerations, subdocuments, both gap files, and source manifests to schema QA.

### Tests

- [x] Interior 404 is retained while discovery continues.
- [x] Trailing 404 is not a gap.
- [x] PDF response is `unsupported_format`.
- [ ] 500, timeout, or malformed HTML aborts.
- [x] Missing expected cache in cache-only mode leaves canonical output hashes unchanged.
- [x] Commission IDs reconcile to 412 parsed plus eight gaps through ID 420.
- [x] Plenary 1-135 produces an empty gap file.
- [ ] Live mode refreshes an existing mutable cache.
- [ ] Dossier changes in every formerly omitted field change the fingerprint.
- [ ] Author order alone does not change the fingerprint.

### Acceptance criteria

- No scraper returns success after skipping an unexpected source artifact.
- Failed cache-only reparses preserve previous canonical Parquet byte-for-byte.
- Every expected source item is represented as parsed, explicit no-result, or accepted remote gap.
- Mutable source manifests expose when data was fetched and last checked.
- Terminal dossiers are eventually rechecked.
- A complete cache deterministically rebuilds all staging outputs.

---

## 10. Complete normalized provenance

### Current gap

Vote casts implement the intended provenance contract in `scrapers/normalize/src/vote_casts.rs`. Most other normalized relations retain only URL/cache and string confidence. `oral_written_links.parquet` has no source provenance. The graph then hashes whatever is currently at the cache path in `scrapers/graph/src/build.rs:88-113`, which can associate a stale normalized relation with newer cache bytes.

`source_artifacts.parquet` also has blank `scraped_at` for all 15,050 current rows.

### Canonical normalized provenance contract

Every source-derived normalized row should carry:

```text
source_url
cache_path
source_artifact_id
source_content_hash
block_parser_version   # report-derived rows; empty only when not applicable
extractor_version
confidence             # FLOAT64 in [0,1]
```

Artifact ID must be the canonical hash of URL plus cache path. Content hash must represent bytes used when the normalized row was produced, not bytes observed later during graph construction.

### Producer TODO

- [ ] Extract the proven vote-cast provenance pattern into a small shared normalize helper.
- [ ] Migrate `asked`, `answered`, `answered_by`, `authored`, `holds_role`, `invited`, `interpellated`, `interpellation_responded`, `written_asked`, `addressed_to`, and `oral_written_links`.
- [ ] Apply the same contract to normalized `utterances`, `answers`, and unresolved-person diagnostics where source-derived.
- [ ] Decide explicitly whether derived `vote_reconciliation` is outside the source-derived contract.
- [ ] Replace categorical confidence strings such as `exact`/`parsed` with documented numeric values.
- [ ] Populate artifact/hash/version fields when an unresolved row is emitted; do not use `..Default` to erase known provenance.
- [ ] Add source provenance to oral-written links rather than emitting graph edges with empty URL/cache.
- [ ] Populate `scraped_at` from cache metadata/source manifests when graph source artifacts are built.

### Graph TODO

- [ ] Read artifact ID/content hash/version from normalized rows.
- [ ] Validate them against `graph/source_artifacts.parquet`.
- [ ] Stop recomputing provenance from current cache content for migrated relations.
- [ ] Reject or warn on transform-time hash mismatch rather than silently relabeling the edge.
- [ ] Preserve source provenance on every normalized graph edge.

### QA TODO

- [ ] Add `normalize.provenance_columns` for table-level schema requirements.
- [ ] Add `normalize.provenance_complete` for row-level required values.
- [ ] Add `normalize.provenance_artifact_id` for canonical ID and graph-artifact joins.
- [ ] Add `normalize.confidence_typed` for `FLOAT64` and range `[0,1]`.
- [ ] Use an explicit normalized-table catalog with each table’s row ID column.
- [ ] Validate empty table schemas as well as populated tables.

### Documentation TODO

- [ ] Update every normalized schema in `STAGING.md`.
- [ ] Update provenance implementation status in `DATA_GRAPH.md`.
- [ ] Synchronize `canvases/data-graph-overview.canvas.tsx` if implementation status or coverage counts change.

### Tests

- Missing provenance column produces one table-level finding.
- Blank row value produces row-level finding.
- Valid canonical artifact ID passes.
- Wrong hash or artifact ID fails.
- Artifact missing from graph provenance fails.
- Confidence outside `[0,1]`, NaN, or wrong Arrow type fails.
- Changing cache bytes after normalization is detected as stale rather than silently accepted.

### Acceptance criteria

- Every in-scope normalized row has transform-time artifact provenance.
- Every artifact ID joins to graph source artifacts.
- Every content hash reflects bytes used by the normalizer.
- All confidence columns use one documented numeric representation.
- Graph warnings can link to exact source artifacts without inference.

---

## 11. QA expansion while preserving advisory execution

### Required behavior

Do not change:

- `just qa` to invoke `--strict`;
- default exit behavior for warn/fail detail rows;
- baseline-regression semantics of `just qa-strict`;
- exit code 2 for operational read/write/execution errors.

Data-quality findings must be `CheckDetail` rows, not returned `Err` values. This allows the advisory command to write a complete report and exit 0.

### New check catalog

| Check ID | Default status when present | Purpose |
|---|---|---|
| `question.questioner_resolved` | `fail` | Prevent recurrence of malformed/unresolved commission askers |
| `written.published_answer_text_present` | `fail` | Published QRVA answer has neither language body |
| `fk.utterance_interpellation` | `fail` | Interpellation utterance has no unique same-meeting target |
| `utterance.interpellation_item_id_canonical` | `warn` | Site ref resolves, but stored ID is noncanonical |
| `remuneration.amount_valid` | `fail` | Non-numeric, negative, non-finite, or reversed range |
| `remuneration.amount_scale` | `warn` | Conservative implausible annual amount threshold |
| `remuneration.duplicate_mandate` | `warn` | Same logical mandate has multiple rows |
| `lobby.url_placement` | `warn` | URL token appears in wrong field |
| `lobby.column_bleed` | `warn` | URL fragment demonstrates fixed-column bleed |
| `commission.chair_subchair_overlap` | `fail` | Semantically disjoint role lists overlap |
| `dossier.date_chronology` | `warn` | Source-backed chronology anomaly |
| `qa.warning_graph_target` | `fail` | Entity warning points to missing graph node/artifact |
| `normalize.provenance_columns` | `fail` | Normalized table lacks required provenance schema |
| `normalize.provenance_complete` | `fail` | Normalized row lacks provenance values |
| `normalize.provenance_artifact_id` | `fail` | Artifact identity/hash mismatch |
| `normalize.confidence_typed` | `fail` | Confidence type/domain is not canonical |
| `source.manifest_complete` | `fail` | Expected source inventory is incomplete |
| `source.cache_metadata` | `fail` | Cache/hash/timestamp contract is inconsistent |
| `source.freshness` | `warn` | Mutable source has exceeded its refresh interval |

### QA implementation TODO

- [ ] Add all IDs to `registered_check_ids()` so zero-finding checks produce explicit pass rows.
- [ ] Add descriptions to `scrapers/qa/src/check_catalog.rs`.
- [ ] Add triage descriptions and code pointers.
- [ ] Preserve summary/detail count equality through `qa.summary_vs_detail`.
- [ ] Include source URL/cache and source block range wherever available.
- [ ] Add prerequisite table checks so an absent input cannot become an implicit pass.
- [ ] Include commissions, remunerations, subdocuments, source manifests, and both meeting-gap tables in schema QA.
- [ ] Update corpus coverage reporting to annotate expected procedural exclusions instead of deleting them from the overview.

### Exit-code tests

- Known warn/fail details plus successful execution -> `just qa` exits 0 and writes artifacts.
- Same newly introduced regression under `just qa-strict` -> exit 1.
- Missing/unreadable mandatory input or output write failure -> exit 2.
- Triage dry-run consumes all new warn/fail IDs.

### Acceptance criteria

- Every deterministic defect above is visible in detail Parquet and summary output.
- Zero-count checks remain visible as passes.
- The default QA command remains advisory.
- Graph viewer can consume entity warnings from the same detail artifact.

---

## Verification checklist

Run focused tests while implementing each package, then the full sequence:

```bash
cargo fmt --all -- --check

cargo test -p crawl
cargo test -p qrva
cargo test -p commission-meetings
cargo test -p plenary-meetings
cargo test -p commissions
cargo test -p lobby
cargo test -p remunerations
cargo test -p dossiers
cargo test -p normalize
cargo test -p graph
cargo test -p qa

just reparse-scrapers
just build-identity
just normalize-edges
just build-graph
just qa
just qa-triage -- --dry-run
```

Run graph-viewer tests from its own environment according to its project setup. At minimum, verify database view registration, node-detail warning payloads, Vote-to-VoteResult warning inheritance, missing-QA behavior, and UI rendering.

Do not run `just qa-update-baseline` until:

- all new check counts have been inspected;
- accepted source anomalies are classified rather than hidden;
- procedural-report coverage policy is documented;
- graph warning targets resolve;
- source gaps have reviewed reason codes.

## Completion definition

The remediation is complete when:

1. Item 1 remains at zero under QA.
2. Published QRVA answer text is retained.
3. Procedural-report exclusions are explicit and mixed political reports remain checked.
4. Interpellation utterance IDs are canonical without graph guesses.
5. Remuneration values use documented EUR semantics.
6. Lobby fields match PDF columns for regression fixtures.
7. Commission chair/subchair roles are disjoint.
8. Real source anomalies appear on affected graph-viewer entities.
9. Incomplete caches cannot replace complete canonical snapshots.
10. Normalized rows carry transform-time provenance.
11. QA covers these contracts while remaining advisory by default.
