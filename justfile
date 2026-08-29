# Scrape all sources into staging Parquet.
update: scrape-sessions scrape-commissions scrape-members scrape-plenary-meetings scrape-commission-meetings scrape-qrva scrape-dossiers scrape-lobby scrape-remunerations

# Rebuild staging Parquet from cached scraper data.
reparse: reparse-scrapers

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

summarize-text:
    cargo run --release --bin text-summarizer

summarize-dossiers:
    cargo run --release --bin dossier-summarizer

generate-dossier-markdown:
    python3 summarizers/dossier-pdf-to-markdown/main.py

summarize-pdf-rust:
    cargo run --release --bin pdf_extractor
