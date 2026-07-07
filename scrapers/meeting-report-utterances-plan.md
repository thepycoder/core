# Meeting Report Utterance Extraction Plan

## Data Snapshot

Data inspected from `/home/victor/Projects/partijgedrag-parent/partijgedrag/core/data`; this worktree still has a broken `web/src/data -> ../../data/data` symlink.

- Raw session data: 133 plenary meetings, 66 commission meetings, 777 plenary question groups, 725 commission question groups, 1,429 plenary votes.
- Existing normalized utterances: 8,042 rows in `normalized/utterances.parquet`.
- Existing utterance schema: `utterance_id`, `question_id`, `session_id`, `meeting_id`, `meeting_kind`, `seq`, `raw_speaker`, `speaker_person_id`, `text`, `source_url`, `cache_path`, `confidence`.
- Coverage today: normalized utterances are Q&A-derived only. Every row has a `question_id`; main-session debates, hearings, propositions, agenda debates, vote explanations, openings, and closings are not represented unless they were captured inside `questions.discussion`.
- Useful gap samples:
  - Plenary `ip019x`: 84 source speaker markers, 0 normalized utterances.
  - Plenary `ip129x`: 95 source speaker markers, 0 normalized utterances.
  - Commission `ic001x`: 51 source speaker markers, 0 normalized utterances.
  - Commission `ic015x`: 16 source speaker markers, 0 normalized utterances.
  - Commission question `56_57_5`: raw row exists but `discussion = []`, so no normalized utterances.
- Speaker resolution today: 6,888 exact utterance speaker matches, 1,154 unresolved; most unresolved are chairs/ministers such as `Voorzitter`, `Jan Jambon`, `Nicole de Moor`, `Georges Gilkinet`.

## Goal

Promote utterances from “question discussion fragments” to a full-session speech layer covering plenary and commission reports end to end, while preserving current Q&A fields for compatibility.

## Target Artifacts

Prefer extending the existing `data/normalized/utterances.parquet` rather than adding separate raw `plenary/utterances.parquet` and `commission/utterances.parquet` files.

Add or derive these fields:

- `agenda_id`: agenda number from headings or turn prefix, e.g. `01`, `18`, `59`.
- `turn_number`: source marker such as `01.04`.
- `item_kind`: `question`, `hearing`, `general_debate`, `proposition`, `notice`, `vote`, `vote_explanation`, `opening`, `closing`, `procedural`, `unknown`.
- `item_id`: raw question/proposition/notice/vote id when known.
- `question_ids`: all internal `Q...C/P` refs attached to grouped questions.
- `dossier_id`, `document_id`, `motion_id`, `vote_id`: optional entity links.
- `speaker_role`: `mp`, `minister`, `chair`, `external`, `unknown`.
- `language`, `block_start`, `block_end`, `source_section`.

Optional but useful: `normalized/report_blocks.parquet` with ordered `h1`, `h2`, `p`, and `table` blocks for debugging and QA.

## Action Plan

1. Build a reusable report block stream.
   - Decode official HTML with tolerant Windows-1252 handling; cached files can contain undefined bytes.
   - Preserve order and metadata for `h1`, `h2`, `p`, and `table`.
   - Keep compact table text, class, `lang`, block index, and original source/cache paths.

2. Build an agenda timeline.
   - Pair bilingual headings by agenda number and order.
   - Plenary has section `h1`s like `Naamstemmingen`; commissions can start directly at `h2`.
   - Store agenda item ranges as `[start_block, end_block)`.
   - Treat heading language attributes as advisory only; existing data already shows they are often wrong.

3. Generalize speaker segmentation.
   - Reuse the current `get_discussion_json` logic, but run it over the full block stream.
   - Start turns on `^\d{2}\.\d{2}\s+Name:` and chair markers.
   - Merge continuation paragraphs until next speaker marker, heading, vote table, or hard section boundary.
   - Preserve source `turn_number`; its `NN` prefix is the best fallback link to agenda item `NN`.

4. Link utterances to entities.
   - Primary: block range containment.
   - Secondary: `turn_number` prefix matching the agenda number.
   - For grouped questions, attach all `Q...C/P` refs and the existing raw `question_id`.
   - For plenary debates, attach dossier/document refs from headings like `(501/1-2)`.
   - For vote explanations, attach the closest vote/agenda item when unambiguous.

5. Normalize speakers and roles.
   - Keep `raw_speaker` permanently.
   - Resolve MPs through `identity/persons.parquet` and aliases.
   - Add explicit role handling for chairs and ministers before treating them as unresolved people.
   - Do not fail extraction on unresolved speakers; emit QA warnings with source context.

6. Preserve Q&A compatibility.
   - Existing raw question IDs differ by layer (`56_2_0` raw commission vs `56_commission_2_0` normalized), so standardize ID mapping before joins.
   - Rebuild `questions.discussion` from utterances as a validation target first, not as an immediate replacement.
   - Keep `questions.discussion` until frontend and summarizers read the normalized layer.

7. Backfill and compare.
   - Start with fixtures: `ip019x`, `ip117x`, `ip129x`, `ic001x`, `ic015x`, `ic017x`, `ic057x`.
   - Compare source speaker-marker counts against new full-session utterance counts.
   - Verify existing Q&A-derived utterance counts are preserved where source coverage was already good.

## Things To Know Upfront

- Existing `normalized/utterances.parquet` proves the graph/identity layer can already consume utterances; extend it rather than inventing a parallel model.
- Source turn numbers reset per agenda item; never treat `seq` as a document-global turn number unless generated by us.
- Vote appendices (`DETAIL VAN DE NAAMSTEMMINGEN`) are evidence for vote casts, not debate utterances.
- Commission meetings 1 and 15 have no question rows but contain real speech content; they are the clearest tests for the new full-session model.
