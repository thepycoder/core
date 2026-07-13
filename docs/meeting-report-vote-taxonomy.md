# Meeting report vote taxonomy

Formal vote shapes extracted from session 56 plenary cache. Each parser edge case has a committed fixture under `scrapers/crawl/tests/fixtures/votes/` with the originating meeting noted in comments.

| Shape | Meetings | Fixture |
|-------|----------|---------|
| Standard roll call + appendix | 60, 129 | `roll_call_compact.html` |
| Compact roll call, no appendix | 81 | `meeting_81_compact_no_appendix.html` |
| Result reuse | 102, 127 | `result_reuse.html` |
| Language-group roll call | 133 | `language_group_roll_call.html` |
| Language-group invalid (sum mismatch) | — | `language_group_invalid.html` |
| Secret ballot aggregate | 14, 16 | `secret_ballot_aggregate.html` |
| Secret ballot + candidates | 14, 16 | `secret_ballot_candidates.html` |
| Multiple secret ballots in one meeting | — | `multiple_secret_ballots.html` |
| Secret naturalization (meeting 135) | 135 | `meeting_135_secret_naturalization.html` |
| Quorum failure (heading) | — | `quorum_failure.html` |
| Quorum failure (formal sequence) | 110 | `quorum_failure_formal_sequence.html` |
| Sitting/standing adopted | 15, 131, 133 | `sitting_standing.html` |
| Sitting/standing rejected | — | `sitting_standing_rejected.html` |
| Reverse appendix marker | 15 | `appendix_reverse_order.html` |
| Repeated source occurrence (appendix keyed by occurrence) | — | `repeated_source_occurrence.html` |
| Negative: debate quoted table | — | `debate_quoted_table.html` |
| Negative: post-zone quoted table | — | `post_zone_quoted_table.html` |
| Negative: unanimity / no objection exchange | — | `unanimity_no_objection_negative.html` |

## ID conventions

- **Vote (decision):** `{session_id}-{meeting_id}-v{seq}` — one row per formal decision/matter in `votes.parquet`.
- **VoteResult (evidence):** `{session_id}-{meeting_id}-r{seq}` — roll-call table, secret ballot, sitting/standing, or quorum block; reusable when `votes.reuses_result` is true.
- **Source roll-call number:** site-native appendix marker (e.g. `1`, `2`); paired with `(source_roll_call_number, occurrence)` when the same marker appears more than once in one report.
- **Spans:** `span_id` is stable per `(artifact_id, entity_type, entity_id, span_role, block_start, block_end)`; `artifact_id` is SHA-256 of `source_url` + `cache_path`.

## Formal zone rules

Votes are emitted only inside:

- `Naamstemmingen` / roll-call section headings, or
- A local formal sequence (`Begin van de stemming` → vote marker → outcome/quorum), or
- `Geheime stemming` secret-ballot sections, or
- Paired sitting/standing proposal + outcome paragraphs.

Zone boundaries close at the next section heading (`h1`/`h2`).

## Deliberate exclusions

- Debate prose mentioning vote counts
- Quoted roll-call tables outside formal zones
- Generic unanimity / no-objection exchanges
- Commission-result summaries in speeches

Unresolved assembly events (e.g. language-group tally mismatch) are written to `vote_unresolved_events.parquet` with `block_start`/`block_end` evidence — not silently zero-filled.

## Manual browser checklist (graph-viewer)

After `just scrape-plenary-meetings` and starting graph-viewer:

1. Open `?report=56&kind=plenary&meeting=60` — roll-call tables render as structured rows; extraction badges on vote blocks
2. Open `?report=56&kind=plenary&meeting=110` — quorum failures with participation tallies, no yes/no member bars
3. Open a Vote node — breakdown uses `result_id` joins (no schema error)
4. Click a QA issue with `source_block` — report opens at block index with history preserved
5. Filter report overlays (`entity_type`, `coverage_kind`, `span_role`) — invalid/stale spans show diagnostics panel
