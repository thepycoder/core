# Scrape all sources, rebuild identity, normalize edges, and write graph Parquet.
update: scrape-sessions scrape-commissions scrape-members scrape-plenary-meetings scrape-commission-meetings scrape-qrva scrape-dossiers scrape-lobby scrape-remunerations build-identity normalize-edges enrich-external-persons build-graph qa

scrape-sessions:
    cargo run --bin sessions

scrape-plenary-meetings:
    cargo run --bin plenary-meetings

scrape-commission-meetings:
    cargo run --bin commission-meetings

scrape-qrva:
    cargo run --bin qrva

scrape-dossiers:
    cargo run --bin dossiers

scrape-members:
    cargo run --bin members

scrape-lobby:
    cargo run --bin lobby

scrape-remunerations:
    cargo run --bin remunerations

scrape-commissions:
    cargo run --bin commissions

build-identity:
    cargo run --bin identity
    cargo run --bin external-identity

normalize-edges:
    cargo run --bin normalize

build-graph:
    cargo run --bin graph

qa:
    cargo run --bin qa

qa-strict:
    cargo run --bin qa -- --strict

qa-update-baseline:
    cargo run --bin qa -- --update-baseline

qa-triage *ARGS:
    cd tools/qa-triage && uv sync && uv run python -m qa_triage {{ARGS}}

enrich-external-persons:
    cargo run --bin external-person-enricher

summarize-text:
    cargo run --bin text-summarizer

summarize-dossiers:
    cargo run --bin dossier-summarizer

generate-dossier-markdown:
    python3 summarizers/dossier-pdf-to-markdown/main.py

summarize-pdf-rust:
    cargo run --bin pdf_extractor
