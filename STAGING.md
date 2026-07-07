# Staging contract

Frozen schema for parquet files under `data/` (override with `SCRAPER_DATA_DIR`). All columns are Arrow `Utf8` unless noted nullable.

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

| Column | Notes |
|--------|--------|
| `first_name` | From members parquet |
| `last_name` | From members parquet |
| `year` | Query year |
| `mandate` | Mandate description |
| `institute` | Institute name |
| `remuneration_min` | Parsed min EUR |
| `remuneration_max` | Parsed max EUR |
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

**questions.parquet:** `question_id`, `session_id`, `meeting_id`, `questioners`, `respondents`, `topics_nl`, `topics_fr`, `discussion` (JSON), `internal_ids`, `source_url`, `cache_path`

**votes.parquet:** `vote_id`, `session_id`, `meeting_id`, `date`, `title_nl`, `title_fr`, `yes`, `no`, `abstain`, `members_yes`, `members_no`, `members_abstain`, `dossier_id`, `document_id`, `motion_id`, `source_url`, `cache_path`

**propositions.parquet:** `proposition_id`, `session_id`, `meeting_id`, `title_nl`, `title_fr`, `dossier_id`, `document_id`, `source_url`, `cache_path`

**notices.parquet:** `notice_id`, `session_id`, `meeting_id`, `title_nl`, `title_fr`, `source_url`, `cache_path`

Plenary report URL pattern: `https://www.dekamer.be/doc/PCRI/html/{session}/ip{meeting:03}x.html`

### Commission (`data/sessions/56/commission/`)

**meetings.parquet:** `session_id`, `meeting_id`, `date`, `time_of_day`, `start_time`, `end_time`, `commission`, `chair`, `source_url`, `cache_path`

**questions.parquet:** same columns as plenary questions (`internal_ids`, not `dossier_ids`)

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
| `id` | | Subdocument number |
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

## Identity (`data/identity/`)

**persons.parquet:** `person_id`, `first_name`, `last_name`, `date_of_birth`, `place_of_birth`, `language`, `source_url`, `cache_path` — Chamber MPs only (cvview keys).

**external_persons.parquet:** `external_person_id`, `display_name`, `kind`, `source`, `first_seen_bucket`, `source_url`, `cache_path` — non-MP actors (ministers, experts, Voorzitter, institutional authors).

**external_person_aliases.parquet:** `alias_norm`, `external_person_id`, `source`, `confidence`

**external_person_contexts.parquet:** `context_id`, `external_person_id`, `meeting_id`, `meeting_kind`, `meeting_date`, `question_id`, `question_topics_nl`, `question_topics_fr`, `utterance_excerpt`, `source_url`, `cache_path`, `raw_field` — LLM enrichment input.

**external_person_bios.parquet:** `external_person_id`, `input_hash`, `bio_nl`, `bio_json`, `model`, `search_queries`, `created_at` — LLM output from `enrich-external-persons`.

## Sidecar files (not parquet)

- `data/current_plenary_id.txt` — last known plenary meeting id (discovery only)
- `data/current_commission_id.txt` — last known commission meeting id
- `scrapers/cache/sessions/{session}/dossier_ids.txt` — tab-separated dossier ids discovered from plenary refs
