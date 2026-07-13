# QA triage

Clusters warn/fail rows from `just qa` and writes LLM root-cause reports for fix agents.

```bash
# From core repo root (after `just qa`):
just qa-triage

# Clustering + evidence only (no API calls):
just qa-triage --dry-run

# Single check prefix or cluster:
uv run qa-triage --check graph
uv run qa-triage --cluster-id question_id_mismatch_utterance_part_of
```

Requires `MISTRAL_API_TOKEN` in `.env` unless `--dry-run`.

Outputs: `data/qa/reports/index.md`, `{root_cause_id}.md`, `manifest.json`.
