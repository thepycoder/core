# Scrape all sources, rebuild identity, normalize edges, and write graph Parquet.
update: scrape-sessions scrape-commissions scrape-members scrape-plenary-meetings scrape-commission-meetings scrape-qrva scrape-dossiers scrape-lobby scrape-remunerations build-identity normalize-edges enrich-external-persons build-graph qa

# Rebuild staging + graph from existing scraper cache only (no network fetches).
reparse: reparse-scrapers build-identity normalize-edges build-graph qa

reparse-scrapers:
    #!/usr/bin/env bash
    set -euo pipefail
    export SCRAPER_CACHE_ONLY=1
    cargo run --release --bin sessions
    cargo run --release --bin commissions
    cargo run --release --bin members
    cargo run --release --bin plenary-meetings
    cargo run --release --bin commission-meetings
    cargo run --release --bin qrva
    cargo run --release --bin dossiers
    cargo run --release --bin lobby
    cargo run --release --bin remunerations

scrape-sessions:
    cargo run --release --bin sessions

scrape-plenary-meetings:
    cargo run --release --bin plenary-meetings

scrape-commission-meetings:
    cargo run --release --bin commission-meetings

scrape-qrva:
    cargo run --release --bin qrva

scrape-dossiers:
    cargo run --release --bin dossiers

scrape-members:
    cargo run --release --bin members

scrape-lobby:
    cargo run --release --bin lobby

scrape-remunerations:
    cargo run --release --bin remunerations

scrape-commissions:
    cargo run --release --bin commissions

build-identity:
    cargo run --release --bin identity
    cargo run --release --bin external-identity

normalize-edges:
    cargo run --release --bin normalize

build-graph:
    cargo run --release --bin graph

qa:
    cargo run --release --bin qa

qa-strict:
    cargo run --release --bin qa -- --strict

qa-update-baseline:
    cargo run --release --bin qa -- --update-baseline

qa-triage *ARGS:
    cd tools/qa-triage && uv sync && uv run python -m qa_triage {{ARGS}}

enrich-external-persons:
    cargo run --release --bin external-person-enricher

summarize-text:
    cargo run --release --bin text-summarizer

summarize-dossiers:
    cargo run --release --bin dossier-summarizer

generate-dossier-markdown:
    python3 summarizers/dossier-pdf-to-markdown/main.py

summarize-pdf-rust:
    cargo run --release --bin pdf_extractor
