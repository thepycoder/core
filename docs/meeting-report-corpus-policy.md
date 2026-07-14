# Meeting report corpus policy

## Purpose

Integraal verslag (plenary and commission meeting reports) is the primary source for parliamentary speech, questions, votes, and procedural context. Not every word in a cached report is promoted to an `Utterance` row. This document records which report shapes are **deliberately under-extracted** and how QA should treat them.

Raw HTML and `report_blocks.parquet` remain canonical evidence regardless of utterance coverage.

## What stays out of utterance extraction

Do **not** expand utterance extraction solely to capture:

- constitutive formalities and opening procedure
- oath wording and credentials verification reports
- ceremonial tributes and eulogies
- institutional appointments (Bureau, commissions, delegations)
- administrative communications

That content is available in cache and report blocks. It is low value for utterance-based analysis (`SPOKE`, stance, Q&A linkage) and the wrong abstraction as normalized speech rows. Structured scrapers (`MEMBER_OF`, notices, votes) cover the institutional facts better.

This is **corpus policy**, not a parser failure.

## Whole-report vs isolated procedural items

| Concept | Meaning | QA speech-coverage treatment |
|---|---|---|
| **Whole-report class** | The sitting is primarily constitutive or administrative | Expected low coverage → `info` with policy reference |
| **Mixed report** | Contains one or more oath/credentials agenda items **and** substantive political content | Normal speech-coverage evaluation (`warn` when below p5) |
| **Session opening** | Ordinary session start with bureau/commission appointments | Annotated; coverage still evaluated unless reclassified |
| **Vote-dominated** | Confidence/motion votes dominate; few speeches | Annotated; vote QA is the primary completeness metric |

**Critical rule:** a single procedural heading must **not** exempt an entire mixed report. Meeting 24 (successor oaths plus government declaration and confidence motion) must remain visible as a low-coverage mixed report.

**Keyword trap:** ordinary political use of “oath” / “serment” / “eed” in debate (e.g. plenary 22, question about President Trump’s oath) is **not** credentials verification. Classification uses procedural **agenda headings** (`h1`/`h2`), not naive full-text keyword search.

## Session 56 plenary catalog

Implementation: `scrapers/crawl/src/corpus_policy.rs` (`classify_meeting`).

### Fully or primarily constitutive

| Meeting | Date | Source | Contents | Class |
|---:|---|---|---|---|
| 1 | 2024-07-04 | `cache/sessions/56/meetings/plenary/56-1.html` | Opening extraordinary session; credentials; agenda adoption | `constitutive` |
| 2 | 2024-07-10 | `cache/sessions/56/meetings/plenary/56-2.html` | Credentials reports, admission votes, constitutional oaths | `constitutive` |
| 3 | 2024-07-16 | `cache/sessions/56/meetings/plenary/56-3.html` | Admission, credentials verification, oath report | `constitutive` |
| 4 | 2024-07-18 | `cache/sessions/56/meetings/plenary/56-4.html` | Oaths, funeral tributes, appointments, admin comms | `constitutive_administrative` |

Meeting 2 does not use normal `h2` structure for its main agenda. Report blocks contain `Onderzoek van de geloofsbrieven en eedafleggingen` / `Vérification des pouvoirs et prestations de serment`; classification must not rely only on heading tags.

### Mixed reports (not exempt)

These contain a formal oath/credentials item but must **not** be exempted as whole documents:

`7`, `15`, `24`, `35`, `63`, `69`, `93`, `119`

Meeting 24 is the clearest counterexample: successor oaths plus government declaration and confidence motion.

### Other low speech-coverage shapes

| Meeting | Shape | Class |
|---:|---|---|
| 67 | Opening ordinary session, Bureau/commission appointments, chair address | `session_opening` |
| 79 | Vote-dominated sitting with confidence and motion votes | `vote_dominated` |

## QA behavior

Check: `utterance.speech_char_coverage`

- Meetings classified `constitutive` or `constitutive_administrative` emit **`info`** (not `warn`) when below the kind p5 threshold, with a reference to this document.
- Those meetings are excluded from the p5 percentile pool so they do not skew regression baselines.
- Mixed, session-opening, and vote-dominated meetings keep normal coverage evaluation.
- Vote, agenda, source-span, and cache checks remain active for every classified report.

## Extending the catalog

Add new entries to `corpus_policy.rs` with a source reference in the `note` field. Prefer explicit meeting IDs over range rules (never `meeting_id <= N`). Add unit tests when introducing edge cases.
