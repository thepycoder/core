# Staging contract

Frozen schema for parquet files under `data/` (override with `SCRAPER_DATA_DIR`). Column types are noted per table (`Utf8`, `UINT32`, `BOOLEAN`, `FLOAT64`); optional fields may be nullable.

Cache paths in `cache_path` are relative to `SCRAPER_CACHE_DIR` (default `scrapers/cache/`).

## ID conventions

Meeting-scoped entities use composite ids: `{session_id}_{meeting_id}_{seq}` where `seq` is a 0-based counter within that meeting (votes, propositions, notices). Questions include meeting kind: `{session_id}_{meeting_kind}_{meeting_id}_{seq}` so plenary and commission meeting numbers do not collide.

Site-native refs (e.g. oral question `Q56001442P`) live in `internal_ids` on question rows, not in `question_id`.

## Root tables

### `data/sessions.parquet`

| Column | Notes |
|--------|--------|
| `session_id` | From cvlist54 `legis=` param |
| `start_date` | Session start from list text |
| `end_date` | Session end from list text |
| `source_url` | cvlist54 index URL |
| `cache_path` | e.g. `sessions/index.html` |

### `data/commissions.parquet`

| Column | Notes |
|--------|--------|
| `name` | Commission name (lowercased) |
| `type` | Section heading from LstCom index |
| `chairs` | CSV |
| `subchairs` | CSV |
| `permanent_members` | CSV |
| `replacement_members` | CSV |
| `source_url` | Commission detail page URL |
| `cache_path` | e.g. `commissions/details/....html` |

### `data/lobby.parquet`

| Column | Notes |
|--------|--------|
| `name` | Organisation name |
| `contacts` | Comma-separated |
| `interests` | Declared interests |
| `url` | Org URL from register |
| `source_url` | Empty until live download is wired |
| `cache_path` | e.g. `lobby/lobbyregister.pdf` |

### `data/remunerations.parquet`

Amounts are stored as Utf8 canonical decimal EUR strings (not Arrow DECIMAL/FLOAT64): European source text such as `279 463,46` becomes `279463.46`. The regimand grid emits distinct Begin/Einde period segments for the same mandate; those are retained as separate rows.

| Column | Notes |
|--------|--------|
| `first_name` | From members parquet |
| `last_name` | From members parquet |
| `year` | Query year |
| `mandate` | Mandate description |
| `institute` | Institute name |
| `remuneration_min` | Parsed min EUR as a canonical decimal string (European source amounts such as `279 463,46` become `279463.46`) |
| `remuneration_max` | Parsed max EUR as a canonical decimal string; ranges such as `1,00 - 6 129,00 EUR` normalize each endpoint independently |
| `period_start` | Raw Begin cell (e.g. `Voorafgaand aan 2018`, `03/12/2018`, `Verlengd`) |
| `period_end` | Raw Einde cell (e.g. `03/12/2018`, `Verlengd`) |
| `source_url` | regimand.be search URL |
| `cache_path` | e.g. `remunerations/Last-First-2024.html` |

## Session 56 tables

### `data/sessions/56/members.parquet`

| Column | Nullable | Notes |
|--------|----------|--------|
| `member_id` | | cvview key (`O####`) |
| `session_id` | | |
| `first_name` | | |
| `last_name` | | |
| `date_of_birth` | | `YYYY-MM-DD` |
| `place_of_birth` | | |
| `language` | | ISO-ish code |
| `constituency` | | |
| `fraction` | | Party slug or `independent` |
| `email` | | Decoded from reversed mailto |
| `active` | | `true` / `false` |
| `start` | yes | Mandate start date |
| `source_url` | | cvview detail URL |
| `cache_path` | | e.g. `sessions/56/members/First Last/details.html` |

### Plenary (`data/sessions/56/plenary/`)

**meetings.parquet:** `session_id`, `meeting_id`, `date`, `time_of_day`, `start_time`, `end_time`, `source_url`, `cache_path`

**questions.parquet:** `question_id`, `session_id`, `meeting_id`, `questioners`, `respondents`, `topics_nl`, `topics_fr`, `internal_ids`, `source_url`, `cache_path`

**agenda_items.parquet:** `agenda_item_id` (`{session}_{kind}_{meeting}_agenda_{start_block}`), `session_id`, `meeting_id`, `meeting_kind`, `agenda_id` (printed heading number), `item_kind`, `item_id` (proceeding entity when typed), `title_nl`, `title_fr`, `dossier_id`, `document_id`, `internal_ids`, `start_block`, `end_block`, `title_blocks`, `source_section`, `source_url`, `cache_path` — full meeting timeline.

**utterances.parquet:** `utterance_id`, `session_id`, `meeting_id`, `meeting_kind`, `agenda_id`, `agenda_item_id`, `turn_number`, `seq`, `item_kind`, `item_id`, `question_ids`, `dossier_id`, `document_id`, `motion_id`, `vote_id`, `raw_speaker`, `speaker_role`, `text`, `language`, `block_start`, `block_end`, `source_section`, `source_url`, `cache_path`

**votes.parquet** (decision/matter): `vote_id`, `result_id`, `session_id` (UINT32), `meeting_id` (UINT32), `date`, `seq` (UINT32), `title_nl`, `title_fr`, `method`, `status`, `outcome`, `dossier_id`, `document_id`, `motion_id`, `source_roll_call_number`, `reuses_result` (BOOLEAN), `source_url`, `cache_path`

**vote_results.parquet:** `result_id`, `session_id` (UINT32), `meeting_id` (UINT32), `seq` (UINT32), `method`, `named` (BOOLEAN), `status`, `outcome`, `source_roll_call_number`, `source_url`, `cache_path`

**vote_tallies.parquet:** `result_id`, `tally_kind`, `option_key`, `label_nl`, `label_fr`, `dimension` (`overall`, `nl_group`, `fr_group`), `count` (UINT32), `selected` (BOOLEAN)

**vote_result_members.parquet:** `result_id`, `position` (`yes`/`no`/`abstain`), `seq` (UINT32), `raw_name`

**vote_unresolved_events.parquet:** `session_id` (UINT32), `meeting_id` (UINT32), `event_kind`, `source_roll_call_number`, `block_start`/`block_end` (UINT32, half-open), `reason`, `evidence_text`, `source_url`, `cache_path` — assembly failures (e.g. language-group sum mismatch) retained for QA triage; not promoted to tallies or casts.

**report_blocks.parquet** (derived, rebuildable): `artifact_id`, `source_content_hash`, `block_index` (UINT32), `block_type`, `text`, `structured_json`, `language`, `class_name`, `word_count` (UINT32), `content_hash` (SHA-256 of the canonical structured block), `has_oraspr` (BOOLEAN), `block_parser_version`, `extractor_version`, `source_url`, `cache_path` under `data/derived/sessions/{session}/plenary/`

**source_spans.parquet** (canonical provenance): `span_id`, `artifact_id`, `source_content_hash`, `session_id` (UINT32), `meeting_id` (UINT32), `entity_type`, `entity_id`, `span_role`, `block_start`/`block_end` (UINT32, half-open), `coverage_kind` (`extraction`|`scope`), `field_names`, `confidence` (FLOAT64, 0–1), `extractor`, `block_parser_version`, `extractor_version`, `source_url`, `cache_path`, `validation_status` (`valid`|`unresolved`), `unresolved_reason` (empty for valid spans; otherwise `wrong_artifact`, `stale_source_content`, `stale_block_parser`, `missing_entity_id`, `invalid_half_open_range`, or `out_of_bounds`). Invalid candidates are retained as explicit unresolved rows.

**Plenary only.** Commission integraal verslag HTML does not contain roll-call vote tables (`Stemming`, `DETAIL VAN DE NAAMSTEMMINGEN`, Ja/Nee member lists). Do not expect vote parquet under `data/sessions/{session}/commission/`. Procedural adoption in commission prose (e.g. *wordt unaniem aangenomen*) is not modelled as Vote/CAST unless it appears as a formal sitting/standing outcome in plenary reports.

**propositions.parquet:** `proposition_id`, `session_id`, `meeting_id`, `title_nl`, `title_fr`, `dossier_id`, `document_id`, `source_url`, `cache_path`

**notices.parquet:** `notice_id`, `session_id`, `meeting_id`, `title_nl`, `title_fr`, `source_url`, `cache_path`

**hearings.parquet:** same columns as commission hearings

**interpellations.parquet:** same columns as commission interpellations

Plenary report URL pattern: `https://www.dekamer.be/doc/PCRI/html/{session}/ip{meeting:03}x.html`

**meeting_gaps.parquet:** `session_id`, `meeting_kind` (`plenary`), `meeting_id`, `reason` (`not_found` | `unsupported_format` | `no_result`), `detail`, `source_url`, `cache_path`, `content_hash`, `fetched_at`, `checked_at` — every id in `1..=last_meeting_id` is either a parsed meetings row or exactly one accepted gap. Trailing discovery 404s are not gaps. Parser failures abort and do not publish. Valid zero-row file when the range is complete.

### Commission (`data/sessions/56/commission/`)

**meetings.parquet:** `session_id`, `meeting_id`, `date`, `time_of_day`, `start_time`, `end_time`, `commission`, `chair`, `source_url`, `cache_path`

**questions.parquet:** same columns as plenary questions (`internal_ids`, not `dossier_ids`)

**agenda_items.parquet:** same columns as plenary agenda_items

**utterances.parquet:** same columns as plenary utterances

**hearings.parquet:** `hearing_id`, `session_id`, `meeting_id`, `meeting_kind`, `agenda_id`, `title_nl`, `title_fr`, `witnesses`, `dossier_id`, `internal_ids`, `source_url`, `cache_path`

**interpellations.parquet:** `interpellation_id`, `session_id`, `meeting_id`, `meeting_kind`, `agenda_id`, `interpellators`, `respondents`, `topics_nl`, `topics_fr`, `internal_ids`, `dossier_id`, `source_url`, `cache_path`

**meeting_gaps.parquet:** `session_id`, `meeting_kind` (`commission`), `meeting_id`, `reason` (`not_found` | `unsupported_format` | `no_result`), `detail`, `source_url`, `cache_path`, `content_hash`, `fetched_at`, `checked_at` — ids in `1..=last_meeting_id` with no scraped row; verify against dekamer.be. Trailing discovery 404s are stop signals only. PDF responses are `unsupported_format` (retain cache bytes). `parse_failed` is not a published reason.

Commission report URL pattern: `https://www.dekamer.be/doc/CCRI/html/{session}/ic{meeting:03}x.html`

### `data/sessions/56/dossiers.parquet`

| Column | Nullable | Notes |
|--------|----------|--------|
| `session_id` | | |
| `id` | | Dossier number |
| `last_updated` | | From cache filename date |
| `title` | | |
| `authors` | | CSV |
| `submission_date` | | |
| `end_date` | | |
| `vote_date` | | |
| `document_type` | | Enum debug string |
| `status` | | Enum debug string |
| `latest_adopted_text_url` | yes | PDF URL |
| `latest_report_url` | yes | PDF URL |
| `eurovoc_main_descriptor` | | |
| `eurovoc_descriptors` | | Comma-separated |
| `source_url` | | flwbn.cfm URL |
| `cache_path` | | e.g. `sessions/56/dossiers/56_297_2024-01-15.html` |

### `data/sessions/56/subdocuments.parquet`

| Column | Nullable | Notes |
|--------|----------|--------|
| `dossier_id` | | Parent dossier |
| `id` | | Native FLWB document id (e.g. `56K1243002`) |
| `date` | | |
| `type` | | DocumentType string |
| `authors` | | CSV |
| `file_url` | yes | PDF URL |
| `source_url` | | Parent dossier flwbn URL |
| `cache_path` | | Parent dossier HTML cache path |

## Summaries (`data/summaries/`)

Written by summarizer binaries; schemas unchanged by Stage 0.

**text-summarizer outputs** (plenary/commission question topics & discussions): `input_hash`, `original`, `summary`, `model`, `meeting_id` (nullable), `created_at`

**dossier-summarizer outputs:** `summary_hash`, `summary` or `arguments`, `model`, `dossier_id`, `source`, `created_at`

### `data/sessions/56/written/questions.parquet`

One row per logical QRVA `DOCNAME` (`question_id` = `56_written_{DOCNAME}`).

| Column | Notes |
|--------|--------|
| `question_id` | `56_written_{DOCNAME}` |
| `session_id` | |
| `docname` | Native QRVA document name |
| `kind` | `written` |
| `author_actr_id` | Parsed `(#####)` actor suffix from `AUT` |
| `author_raw` | Full author label |
| `depot_date` | |
| `deadline_date` | |
| `lang` | Original language code |
| `title_nl` / `title_fr` | |
| `text_nl` / `text_fr` | Flattened question body |
| `main_thesa_nl` / `main_thesa_fr` | Thesaurus labels |
| `oral_refs` | CSV of exact oral refs (`Q…C/P`, etc.) |
| `qrva_route_ids` | CSV of route ids |
| `internal_ids` | `qrva:{DOCNAME}` |
| `source_url` / `cache_path` | API provenance |

### `data/sessions/56/written/routes.parquet`

One row per QRVA API route (`ID`); multiple rows may share a `DOCNAME`.

| Column | Notes |
|--------|--------|
| `route_id` | `56_qrva_{API_ID}` |
| `question_id` | Parent written question |
| `qrva_id` | API numeric id |
| `sdocname` | Detail endpoint key |
| `docname` | Shared document name |
| `deptnum` | Department code |
| `dept_title_nl` / `dept_title_fr` | Portfolio titles |
| `questnum` | Departmental question number |
| `statusq` | Route lifecycle status |
| `source_url` / `cache_path` | |

### `data/sessions/56/written/answers.parquet`

One row per populated QRVA answer slot (`NUMA1`–`NUMA4`).

Uses the shared answer schema (see below).

### `data/sessions/{session}/{plenary,commission}/answers.parquet`

Inline *mondelinge vragen schriftelijk behandeld* minister replies from integraal verslag HTML. MP letters live on the linked oral `questions` row (`question_body_*`).

### Shared answer schema (`written/answers.parquet`, `{plenary,commission}/answers.parquet`)

| Column | Notes |
|--------|--------|
| `answer_id` | `56_qrva_{route}_a{slot}` or `{question_id}_a1` for inline |
| `question_id` | Linked question |
| `route_id` | QRVA route id (empty for inline) |
| `session_id` | |
| `meeting_id` / `meeting_kind` | Set for inline answers |
| `agenda_id` | Timeline agenda number when known |
| `answer_slot` | 1–4 for QRVA; 1 for inline |
| `kind` | `written` or `oral_written` |
| `text_nl` / `text_fr` | Minister reply body |
| `status` | Publication / lifecycle status |
| `answer_num` / `publication_ref` / `casa` | QRVA metadata |
| `source_kind` | `qrva` or `integraal` |
| `confidence` | |
| `source_url` / `cache_path` | |

Oral `questions.parquet` (plenary + commission) gains trailing columns:

| Column | Notes |
|--------|--------|
| `question_body_nl` / `question_body_fr` | Canonical MP letter for `treatment_mode=oral_written` (analogous to `written/questions.text_*`) |
| `treatment_mode` | `oral_written` or empty for live debate |

## Sidecar files (not parquet)

- `data/current_plenary_id.txt` — last known plenary meeting id (discovery only)
- `data/current_commission_id.txt` — last known commission meeting id
- `scrapers/cache/sessions/{session}/dossier_ids.txt` — tab-separated dossier ids discovered from plenary refs

## Source manifests (`data/source_manifests/`)

Per-source inventory established by a live scrape (no compatibility shim for pre-manifest caches). Produced by:

- `commission_meetings.parquet` / `plenary_meetings.parquet`
- `sessions.parquet` / `members.parquet` / `commissions.parquet`
- `lobby.parquet` / `remunerations.parquet` / `dossiers.parquet`

**Columns:** `source`, `session_id`, `item_kind`, `native_item_id`, `source_url`, `cache_path`, `status` (`parsed` | `no_result` | `not_found` | `unsupported_format`), `row_count` (UINT32), `content_type`, `content_hash`, `fetched_at`, `checked_at`, `run_mode` (`live` | `cache_only`), `detail`

Cache artifacts may also have a sibling `{file}.meta.json` with `source_url`, `content_type`, `content_hash`, `fetched_at`, `checked_at`. Cache-only mode must not mutate these sidecars.

**Freshness policies** (`scrapers/crawl/src/freshness.rs`): sessions/members/commissions/dossiers ≤ 7 days; lobby/remunerations ≤ 30 days (oldest `checked_at` in the manifest). Terminal dossiers are rechecked every 90 days.

**Dossier cache versions:** `{session}_{id}_{YYYYMMDDTHHMMSSZ}_{hash8}.html` under `cache/sessions/{session}/dossiers/`; old versions retained; newest selected deterministically. Authoritative inventory: `cache/sessions/{session}/dossier_inventory.tsv`.
