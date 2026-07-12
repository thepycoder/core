# Proceeding patterns (inventory 2026-07-11)

Scanned `partijgedrag-3` cache: 66 commission + 133 plenary HTML files (session 56).

## Hearings

| Pattern | Where | Count (approx) |
|---------|-------|----------------|
| `NN … Hoorzitting met:` / `Audition de:` | Commission h2 | 2 headings (meeting 56-15) |
| `hoorzitting`/`audition` inside question h2 | Commission | 8 (must stay `question`, not `hearing`) |
| Global `hoorzitting`/`audition` in h2 | Classifier (crawl) | ~53 utterance rows total pipeline |

**Formal hearing regex:** agenda number + title containing `hoorzitting met` or `audition de:` (not question sub-lines).

**Fixtures:** `56-15` (commission, formal hearing), `ic015` speech-only (no question rows).

## Interpellations

| Pattern | Where | Count |
|---------|-------|-------|
| `NN Interpellatie van X aan Y over "…" (56000070I)` | Plenary h2 | ~6 unique (NL+FR pairs) |
| `Motie(s) ingediend tot besluit van de interpellatie` | Plenary h2 | motions — exclude from entity extract |
| Colloquial `interpelle/interpellé` in speech | Commission/plenary | ignore |

**Site-native id suffix:** `I` (e.g. `56000070I`, `56000229I`). Plenary: 368 `I` refs in cache.

**Commission:** no formal interpellation h2 headings; interpellations are plenary proceedings.

**Fixtures:** `56-45`, `56-95`, `56-97` (plenary interpellations).

## Commission votes

| Marker | Commission | Plenary |
|--------|------------|---------|
| `Stemming/vote` | 0/66 | 74/133 |
| `DETAIL VAN DE NAAMSTEMMINGEN` | 0/66 | 75/133 |
| `wordt unaniem aangenomen` | 1/66 | — |

**Conclusion:** commission integraal verslag has no roll-call vote tables; do not port plenary `extract_votes`.

## Test fixtures

- Commission hearing: `56-15`
- Commission questions+speech: `56-1`, `56-17`
- Plenary interpellation: `56-45`, `56-95`, `56-97`
- Plenary motion-from-interpellation (exclude): `56-52`
