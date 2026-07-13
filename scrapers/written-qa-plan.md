# Written Q&A extraction — handoff report

**Date:** 2026-07-12  
**Status:** Implemented (Option B) — see `scrapers/qrva`, inline parser in `crawl::written_oral_qa`, normalize + graph wiring.  
**Primary fixture:** commission meeting **407** (`cache/sessions/56/meetings/commission/56-407.html`)

---

## Task

Design and implement extraction of **written oral questions** (*mondelinge vragen schriftelijk behandeld* / *questions orales traitées par écrit*) from commission integraal verslagen.

Today these reports contain substantive minister answers, but only **question headers** (h2 metadata) reach staging. The question letter body and minister response prose are dropped. A follow-up LLM should work out:

1. Where this content lives in source HTML (integraal vs QRVA bulletins — see scope note below).
2. A parser that is **not** turn-marker-based (`NN.NN Speaker:`).
3. Staging/graph wiring (`questions`, `answers`, `utterances`, or a new artifact).
4. QA checks and fixtures covering meetings like 407.

---

## How this surfaced

`just qa` computes per-meeting **document word coverage** (`saved_words / source_words`). See `utterance.speech_char_coverage` in [`meeting-report-qa-plan.md`](meeting-report-qa-plan.md).

Commission **407** is the lowest-coverage commission meeting: **0.381** (2,481 / 6,507 words). Investigation showed the gap is almost entirely the written oral Q&A section — not a scrape failure (cache is complete, 189 KB HTML).

**Decision already recorded:** low coverage on **procedural plenary** sessions (e.g. plenary 2–4) is accepted and out of scope. Written Q&A is different: missing text is politically substantive and should eventually be captured.

---

## Primary fixture: commission 407

| Field | Value |
| --- | --- |
| `meeting_id` | `407` |
| Date | 2026-06-30 |
| Commission | Consumentenbescherming, Socialefraudebestrijding, Personen met een handicap, … |
| Chair | Nahima Lanjri |
| Source URL | https://www.dekamer.be/doc/CCRI/html/56/ic407x.html |
| Cache | `cache/sessions/56/meetings/commission/56-407.html` |

### Report structure (two parts)

**Part A — live oral debate** (no `h1`, start of document)

- Agendas **01–03** with standard turn markers (`01.01 Caroline Désir (PS):`, etc.).
- **12 turns**, ~2,432 source words → **captured** in `utterances.parquet` (12 rows, `item_kind=question`).
- Question metadata in `questions.parquet` (questioners, respondents, topics).

**Part B — written oral questions** (after section headers)

```
H1: Schriftelijk behandelde mondelinge vragen   (label only, 0 body words)
H1: Questions orales traitées par écrit        (~3,449 words, 0 turn markers)
```

- Agendas **04–08**: Ellen Samyn, Isabelle Hansez, François De Smet, Frieda Gijbels → ministers Beenders / Vandenbroucke.
- Topics: disability access, federal procedures, horeca/black work, economic immigration abuse, social fraud results, independent living.
- **0** `NN.NN Speaker:` markers in this section.
- Question **h2 headings** flushed to `questions.parquet`; **letter + answer body not saved**.

### Written-section HTML shape (agenda 05 example)

Question letter (MP → minister):

```
Monsieur le Ministre,
La presse a récemment révélé que plusieurs hôtels...
1) Pouvez-vous faire le point sur...
2) Cette affaire confirme-t-elle...
Je vous remercie pour vos réponses, Monsieur le Ministre.
```

Answer block:

```
Antwoord - Réponse:
Madame la Représentante,
Question 1
En tant que ministre, je constate que...
Question 2
...
```

Salutation variants observed: `Monsieur le Ministre,`, `Geachte minister,`, `Madame la Représentante,`. Answer delimiter: `Antwoord - Réponse:` (407 has 5 hits).

### Staging today (407)

| Table | Rows | Notes |
| --- | ---: | --- |
| `utterances` | 12 | Live section only (agendas 01–03) |
| `questions` | 15 | Headers only; many half-empty NL/FR pair rows |
| `discussion` | — | Empty `[]` on all 15 questions (including live ones with utterances) |

Example question rows: `56_commission_407_5` has `topics_nl` + questioners/respondents but no answer text; `56_commission_407_6` is FR-only header with empty NL fields (bilingual h2 flush artefact).

---

## Contrast case (out of scope): plenary 4

Plenary **4** (0.222 coverage) is a **constitutive/organizational** session: oaths, committee name lists, delegations, eulogies. Missing words are procedural or ceremonial — low value as `Utterance` rows. Do not solve written Q&A by loosening turn-marker rules globally; that would ingest noise from cases like plenary 4.

| | Plenary 4 | Commission 407 |
| --- | --- | --- |
| Missing content | Rosters, legal boilerplate, chair monologue | Minister written answers |
| Turn markers | ~10 total | 12 live + **0** written |
| Political value | Low | **High** |
| Right fix | Ignore / structured rosters elsewhere | **Dedicated written-Q&A parser** |

---

## Prevalence

Rough scan of cached session-56 commission HTML (2026-07-12):

- **410** commission reports cached
- **~30–32** contain `Questions orales traitées par écrit` / `Schriftelijk behandelde mondelinge vragen`

Other sample ids with written section: 328, 330, 337, 340, 352, 356, 368, 371, …

407 is a strong fixture (5 written questions, mixed ministers, bilingual headings, numbered sub-questions in answers). Add 1–2 more meetings after validating pattern stability.

---

## What works today (do not break)

### Question headers — `scrapers/commission-meetings/src/main.rs`

`extract_questions()` walks `h2` elements, pairs NL/FR headings, parses questioner/respondent/topic/`internal_ids` from header text via `extract_question_data()`. Commission reports are treated as “questions from the start” (no section keyword gate).

Relevant shared logic: [`scrapers/crawl/src/question_boundaries.rs`](../crawl/src/question_boundaries.rs) (`classify_question_heading_text`, `starts_new_question_unit`).

### Live oral utterances — `scrapers/crawl/`

- `meeting_report.rs` → `build_agenda_timeline()` + `segment_utterances()`
- Turn detection: [`speaker_parse.rs`](../crawl/src/speaker_parse.rs) (`detect_turn_start` — `NN.NN Label:`, `De voorzitter:`, named chair)
- Paragraphs without a turn start are only kept if appended to an **open** turn; written sections never open a turn.

### Coverage QA — `scrapers/qa/src/speech.rs`

Saved word count sums: utterances + questions (topics/questioners/respondents) + commission chair + plenary votes/propositions/notices. Written answer prose is **not** in any of these buckets today.

---

## What's missing

1. **Question body text** — MP letter with numbered sub-questions (before `Antwoord - Réponse:`).
2. **Answer body text** — minister response, often structured as `Question 1`, `Question 2`, …
3. **Linking** — map bodies to existing `question_id` / `internal_ids` (site refs like `Q56016772C` when present in headers).
4. **`discussion` JSON roundtrip** — live oral utterances exist but `questions.discussion` stays `[]`; written content has no utterance rows at all. See `utterance.roundtrip_discussion` in QA plan.
5. **Graph `ANSWERED` edges** — `DATA_GRAPH.md` lists Answer as a node; minister answers should eventually resolve via `ActorResolver`.

---

## Scope note: integraal vs QRVA

`DATA_GRAPH.md` mentions written Q&A from **QRVA bulletins** (`/QRVA/pdf/{session}/…`) as a separate source not yet scraped.

This handoff focuses on content **already embedded in commission integraal HTML** under *traitées par écrit* — because that is what drives coverage gaps like 407. Follow-up work should decide:

- **Integraal-only** — parse inline `Antwoord - Réponse:` blocks (faster, same cache).
- **QRVA bulletins** — canonical written answers, cross-ref by `internal_ids`.
- **Both** — integraal for discovery + QRVA for full text; needs dedup strategy.

Do not assume one source is always complete; compare 407 integraal text against QRVA if implementing bulletins.

---

## Suggested implementation directions

Not prescriptive — starting points for the follow-up LLM.

### 1. Section detection

Gate on `h1` text:

- `schriftelijk behandelde mondelinge vragen`
- `questions orales traitées par écrit`

Everything after until next unrelated `h1` (or EOF) is written-Q&A zone. Live oral content **before** this `h1` keeps current turn-based path.

### 2. Per-question block segmentation

Within the zone, segment on agenda `h2` pairs (`04 Vraag van …` / `04 Question de …`). For each block:

- **Header** — already parsed; reuse `extract_question_data`.
- **Question body** — paragraphs from first content after h2 until `Antwoord - Réponse:` (or FR-only variant).
- **Answer body** — after delimiter until next agenda h2.

### 3. Staging target (pick one, document in `STAGING.md`)

Options:

- Extend `questions.parquet` with `question_body_nl/fr`, `answer_body_nl/fr` (or JSON `discussion` rebuild).
- New `written_answers.parquet` keyed to `question_id`.
- Emit `utterances` with `item_kind=written_answer` / `speaker_role=minister` — only if graph consumers expect utterance-shaped text.

Prefer matching existing graph intent: Question + Answer nodes, `ASKED` / `ANSWERED` edges.

### 4. Bilingual pairing

407 shows duplicate question rows when NL/FR h2 flush separately (`_5` NL-filled, `_6` FR-filled). Written parser should **merge** NL/FR bodies onto one logical question, not amplify the existing flush bug.

### 5. Tests

Minimum fixtures:

- `56-407.html` — multi-question written section, numbered sub-answers.
- One simpler meeting (e.g. 328 or 352 — 1 `Antwoord - Réponse` hit).
- Regression: agendas 01–03 live turns still produce 12 utterances on 407.

### 6. QA

- Coverage on 407 should rise materially (expect ~0.38 → ~0.85+ if bodies are counted in `saved_words`).
- New check: written-section h2 count vs questions with non-empty answer body.
- Do not regress `speech_char_coverage` on normal debate-only commission meetings.

---

## Key files

| Path | Role |
| --- | --- |
| `cache/sessions/56/meetings/commission/56-407.html` | Primary fixture |
| `scrapers/commission-meetings/src/main.rs` | `extract_questions()`, meeting scrape orchestration |
| `scrapers/crawl/src/meeting_report.rs` | Utterance extraction entry |
| `scrapers/crawl/src/utterance_segment.rs` | Turn-based segmentation (written section bypasses this) |
| `scrapers/crawl/src/speaker_parse.rs` | Turn markers (not applicable to written bodies) |
| `scrapers/crawl/src/question_boundaries.rs` | Question heading classification |
| `scrapers/crawl/src/report_blocks.rs` | Block stream for coverage QA |
| `scrapers/qa/src/speech.rs` | Coverage saved-word accounting |
| `data/sessions/56/commission/questions.parquet` | Current question staging |
| `data/sessions/56/commission/utterances.parquet` | Live oral utterances only |
| `DATA_GRAPH.md` | Question, Answer, `ASKED`, `ANSWERED` target model |
| `STAGING.md` | Staging column contracts |
| `meeting-report-qa-plan.md` | Coverage check + accepted procedural gaps |

---

## Open questions for implementer

1. Is integraal `Antwoord - Réponse:` text always complete, or is QRVA canonical?
2. Should numbered sub-questions (`1)`, `Question 1`) become separate Answer rows or one blob per question?
3. How to handle questions with only FR body in integraal (NL h2 present but body in FR)?
4. Should written answers create `utterances` rows or only populate `questions.discussion` / Answer staging?
5. Plenary integraal: does a written oral section exist there too, or commission-only?

---

## Commands

```bash
# Re-run QA and inspect coverage
just qa

# Inspect 407 staging
uv run --with pyarrow python -c "
import pyarrow.parquet as pq
for t in ['questions','utterances']:
    p=f'data/sessions/56/commission/{t}.parquet'
    d=pq.read_table(p).to_pydict()
    rows=[i for i,m in enumerate(d['meeting_id']) if m=='407']
    print(t, len(rows), 'rows')
"
```
