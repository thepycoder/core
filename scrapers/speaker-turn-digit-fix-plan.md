# Speaker resolution fix: intervention turn digits leaking into names

## Problem

A large fraction of "unresolved" speakers/questioners in the graph are known
people whose `raw_speaker` string is corrupted by a stray leading digit or
punctuation mark. Example: searching the graph viewer for `Steven Coenegrachts`
returns the resolved Person **plus** a list of "unresolved names" like
`0     Steven Coenegrachts`, `7     Steven Coenegrachts`, ... that all look
correct at a glance.

### Root cause (dominant)

The turn-start regex assumes exactly two digits after the decimal:

```
scrapers/crawl/src/speaker_parse.rs  (turn_start_regex)
(?P<turn>\d{2}\.\d{2})
```

In timed plenary general debates the Chamber appends an **optional single
intervention digit** to the base turn `DD.MM` — no separator. The HTML encodes
this as a third decimal digit on the bold turn-number span, with the speaker
name in a following `span.oraspr`:

| Source marker | Intended meaning | Regex today captures | Leftover in label |
|---|---|---|---|
| `02.15` | main turn 02.15 | `02.15` | — (correct) |
| `02.150` | turn 02.15, intervention 0 | `02.15` | `0` → `0     Steven Coenegrachts` |
| `02.151` | turn 02.15, intervention 1 | `02.15` | `1` → `1     Axel Ronse` |
| `02.110` | turn 02.11, intervention 0 | `02.11` | `0` → `0     Steven Vandeput` |

Confirmed in source HTML (`ip048x.html` / `cache/.../plenary/56-48.html`):

```
...bold'>02.150</span></span><span class=oraspr>...Steven Coenegrachts </span>...(Open Vld): ...
...bold'>02.110</span></span>...<span style='mso-spacerun:yes'> </span>Steven Vandeput </span>...(N-VA): ...
```

For `02.150` the regex captures `turn = 02.15`, and the leftover `0` becomes
the first character of the speaker label. The run of spaces comes from the
`mso-spacerun` span, later collapsed to one. This normalizes to
`0 steven coenegrachts`, which is not in the person index → `not_in_index`.

Almost all affected utterances are `item_kind = general_debate` (~1,930 of
~1,960 digit-prefixed rows). The same turn cluster often contains one clean main
speaker plus interventions 0–9 sharing the truncated `turn_number`.

**Compound effect:** because the digit is now at the start of the string, the
title-strip regex (anchored with `^`) no longer matches, producing entries like
`4  Minister  Jan Jambon` (minister name also fails to resolve).

**Collateral:** `utterance_id` is derived from truncated `turn_number`
(`02.15` for all interventions under that turn). Today **205 `utterance_id`
values are duplicated** (typically 11 rows each = main turn + interventions
0–9). Fixing the regex should change these IDs — that is desirable, not a
regression to avoid.

### Root cause (secondary, minor)

Questioner names carry a leading `-` (e.g. `-Bert Wollants`,
`-Roberto D'Amico`). `split_csv` in `scrapers/normalize/src/common.rs` splits on
comma and trims whitespace but does not strip a leading dash/bullet artifact.

## Scope (measured over `data/normalized/unresolved_persons.parquet`)

Approximate — exact counts depend on cleaning steps; re-run after fix to confirm.

| Bucket | Recoverable / total unresolved occurrences |
|---|---|
| speakers | ~1,850–2,050 / 4185 |
| questioners | 13 / 14 |
| respondents | 0 / 1660 |
| votes | 0 / 4693 |
| authors | 0 / 14 |
| commission_members | 0 / 2 |

Overall ~700+ of ~830 distinct unresolved names (~87%) and ~1,900 occurrences
resolve after stripping leading markers + embedded titles.
`respondents`/`votes`/`authors` show ~0 recovery — those are different problems
(ministers resolve as ExternalPerson, vote-appendix name order, etc.), out of
scope here.

## Approach: defense in depth

Fix the true root cause at parse time, add a one-place defensive cleaning net so
the whole *class* of leading-garbage bugs self-heals, and add a QA guard so
future markup quirks are flagged instead of silently inflating unresolved
counts.

### Step 1 — Fix the turn-number regex (primary, source-level fix)

File: `scrapers/crawl/src/speaker_parse.rs`

- Change `(?P<turn>\d{2}\.\d{2})` to consume the optional intervention digit:

  ```regex
  (?P<turn>\d{2}\.\d{2}\d?)
  ```

  Prefer this over looser patterns like `\d{1,3}\.\d{1,3}` or `\d+\.\d+` — it
  matches the actual Chamber format (`DD.MM` + optional `0`–`9`) without
  widening false positives.

- Verify `detect_turn_start` / `parse_turn_start` capture the **full** turn
  marker (`02.150`, not `02.15`) and that `raw_speaker` no longer includes the
  intervention index.
- Verify `turn_slug` / `utterance_id` **do change** for affected turns and that
  duplicate-ID clusters collapse (205 → 0). Example: `02.150` should produce a
  distinct id suffix from `02.15`.
- Add unit tests:
  - `02.150 Steven Coenegrachts:` → turn `02.150`, speaker `Steven Coenegrachts`
  - `02.15 Stefaan Van Hecke:` → turn `02.15`, speaker `Stefaan Van Hecke`
    (regression: no accidental third-digit consumption)
  - `02.110`-style turn → speaker `Steven Vandeput`, not `0 Steven Vandeput`
  - Fixture: `56-48.html` (contains `02.110`–`02.117` and `02.150` clusters)

Impact: `turn_number`, `utterance_id`, and `turn_slug` change for intervention
rows. IDs are regenerated downstream — re-run all builds (Step 4). Anything that
persisted old utterance IDs outside the pipeline will need updating.

### Step 2 — Defensive leading-marker + title cleaning (universal safety net)

File: `scrapers/identity/src/normalize.rs` (`clean_raw_name`)

- Before title stripping, strip a leading run of **digits and list markers
  only** — digits, bullets (`•`, `·`, `\u2022`), and dashes/en-dashes
  (`-`, `\u2013`) followed by optional space.
- Re-run the existing title-prefix strip afterwards so `4  Minister  Jan Jambon`
  → `Jan Jambon`.
- CAVEAT: do **not** strip all non-letters. Dutch names may legitimately start
  with an apostrophe (`'t ...`); particle/hyphenated names exist. Keep the
  strip narrow (digits + bullets + standalone leading dash).

Because both `speakers` and `questioners` flow through
`resolve_detail` → `clean_raw_name`, this single change also fixes the `-`
questioner cases (Step 1 does not cover those). This is the most "universal"
single edit. Also back-heals already-scraped parquet without a full re-crawl.

- Add unit tests: `0     Steven Vandeput` → resolves; `4  Minister  Jan Jambon`
  → `Jan Jambon`; `-Bert Wollants` → resolves; `'t Hooft`-style input is left
  intact.

### Step 3 — QA regression guard (catch the class, not the instance)

Files: identity/normalize QA outputs (`unresolved_report.md`,
`alias_candidates.parquet`), consistent with `scrapers/meeting-report-qa-plan.md`.

- For every unresolved name, attempt a "cleaned" re-resolution (strip leading
  markers, strip titles, collapse whitespace). If the cleaned form matches a
  known Person, emit it as a **high-confidence regression signal** separate from
  ordinary unresolved entries.
- Add `speaker.utterance_id_duplicates`: flag `utterance_id` values shared by
  multiple distinct `raw_speaker` values (205 clusters today).
- Add `speaker.digit_prefix_resolvable`: `raw_speaker ~ '^\d\s+\S'` and stripped
  name resolves → warning.
- This converts "N unresolved, some are bugs" into "N unresolved that *should*
  have resolved" — a precise alarm that fires immediately if dekamer.be
  introduces a new quirk (4-digit turns, new separators).
- Decision: treat these exact-match, mechanically-explained cases as **hard
  resolutions** via Steps 1–2, and use Step 3 only as the guardrail (not as a
  manual review queue).

### Step 4 — Re-run pipeline and verify

- Run `crawl` → `just normalize-edges` → `just build-graph`.
- Verify unresolved `speakers` occurrences drop from ~4,185 toward the genuine
  remainder (chairs/ministers/externals) and `questioners` from 14 → ~1.
- Verify duplicate `utterance_id` count drops to 0.
- Spot-check in the graph viewer: `Steven Coenegrachts` should return the Person
  with no digit-prefixed "unresolved name" siblings; `Steven Vandeput`,
  `Vincent Van Quickenborne`, `Pierre-Yves Dermagne`, `Benoît Piedboeuf`
  likewise.
- Confirm no drop in total utterance count (only speaker resolution should
  change, not segmentation).

## Files touched

- `scrapers/crawl/src/speaker_parse.rs` — turn regex + tests (Step 1)
- `scrapers/identity/src/normalize.rs` — `clean_raw_name` + tests (Step 2)
- identity/normalize QA output code — regression signals (Step 3)
- (optional) `scrapers/normalize/src/common.rs` — only if leading-dash handling
  is preferred in `split_csv` rather than centralized in `clean_raw_name`

## Risks / notes

- Regenerated `utterance_id`s: expected and desirable (fixes 205 collisions);
  re-run all downstream builds.
- Narrow leading-strip is deliberate to avoid corrupting legitimate names
  (`'t`, particles). Cover with tests.
- Out of scope:
  - `respondents` / `votes` / `authors` unresolved buckets (different causes)
  - Clean minister names (`Jan Jambon`, …) awaiting ExternalPerson wiring
  - Comma-suffix roles (`Bart De Wever , premier ministre`) — separate from
    digit leak; route to ExternalPerson when that layer is built
- Future (optional): parse speaker name from `span.oraspr` only in
  `report_blocks.rs` (`has_oraspr` is already tracked but unused) — more
  structural, immune to turn-format changes, but a larger refactor than the
  regex fix.
