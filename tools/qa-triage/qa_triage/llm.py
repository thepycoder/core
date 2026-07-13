from __future__ import annotations

import hashlib
import json
import time
from pathlib import Path
from typing import Any

import httpx

MISTRAL_URL = "https://api.mistral.ai/v1/chat/completions"
MODEL = "mistral-large-latest"
MIN_INTERVAL_S = 2.0

SYSTEM_PROMPT = """You are a parliamentary data pipeline triage analyst for the Belgian Chamber (dekamer.be) scraper stack.

Your job is to IDENTIFY root causes only — never propose code patches, diffs, or implementation.

Given a JSON evidence bundle from automated QA clustering, write a markdown report for a downstream fix agent.

Rules:
- Diagnose the dominant root cause; note secondary factors briefly if relevant.
- Classify category as one of: parser, id_convention, identity, graph_wiring, false_positive, missing_feature, qa_check_itself.
- Set confidence: high, medium, or low.
- Cite evidence using entity_id, parquet field values, and HTML excerpt lines.
- Name pipeline stage: scrape, identity, normalize, graph, or qa_check_itself.
- List files to inspect (from code_pointers and your analysis) with one-line rationale each.
- Suggest fix direction and tests to add — but NO code.
- If evidence is insufficient, say so and list what a human should inspect.

Output ONLY markdown matching this structure (fill every section):

# {title}

- **Root cause id:** `{root_cause_id}`
- **Severity:** fail | warn
- **Affected checks:** `check_id` (N rows)
- **Confidence:** high | medium | low
- **Category:** parser | id_convention | identity | graph_wiring | false_positive | missing_feature | qa_check_itself

## Symptom

## Root cause

## Evidence

## Pipeline stage

## Files to inspect

## Suggested fix direction

## Tests to add

## Related clusters

## Out of scope
"""


def evidence_hash(evidence: dict[str, Any]) -> str:
    payload = json.dumps(evidence, sort_keys=True, ensure_ascii=False)
    return hashlib.sha256(payload.encode()).hexdigest()[:16]


def synthesize_report(
    evidence: dict[str, Any],
    api_key: str,
    *,
    client: httpx.Client | None = None,
) -> str:
    user_content = (
        "Identify the root cause(s) for this QA cluster. "
        "If multiple, pick the dominant one and note secondary factors.\n\n"
        f"```json\n{json.dumps(evidence, indent=2, ensure_ascii=False)[:48000]}\n```"
    )

    owns_client = client is None
    if client is None:
        client = httpx.Client(timeout=120.0)

    try:
        response = client.post(
            MISTRAL_URL,
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
                "Accept": "application/json",
            },
            json={
                "model": MODEL,
                "messages": [
                    {"role": "system", "content": SYSTEM_PROMPT},
                    {"role": "user", "content": user_content},
                ],
            },
        )
        response.raise_for_status()
        data = response.json()
        content = data["choices"][0]["message"]["content"].strip()
        return content
    finally:
        if owns_client:
            client.close()


class RateLimiter:
    def __init__(self, interval_s: float = MIN_INTERVAL_S) -> None:
        self.interval_s = interval_s
        self._last_call = 0.0

    def wait(self) -> None:
        now = time.monotonic()
        elapsed = now - self._last_call
        if elapsed < self.interval_s:
            time.sleep(self.interval_s - elapsed)
        self._last_call = time.monotonic()


def load_manifest(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {"entries": []}
    return json.loads(path.read_text(encoding="utf-8"))


def save_manifest(path: Path, manifest: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
