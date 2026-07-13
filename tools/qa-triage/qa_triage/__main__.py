from __future__ import annotations

import argparse
import sys

from qa_triage.cluster import cluster_rows
from qa_triage.config import get_settings
from qa_triage.evidence import build_evidence, estimate_tokens
from qa_triage.io import load_warn_fail_details, open_duckdb
from qa_triage.llm import (
    RateLimiter,
    evidence_hash,
    load_manifest,
    save_manifest,
    synthesize_report,
)
from qa_triage.report import fallback_report, write_index, write_report
from qa_triage.models import ManifestEntry


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Cluster QA warn/fail rows and write LLM root-cause reports"
    )
    parser.add_argument(
        "--check",
        help="Only clusters whose primary check_id starts with this prefix",
    )
    parser.add_argument("--cluster-id", help="Only process this root_cause_id")
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Cluster and build evidence only; skip LLM calls",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Ignore manifest cache and re-synthesize reports",
    )
    parser.add_argument(
        "--max-clusters",
        type=int,
        default=0,
        help="Cap number of clusters processed (0 = all)",
    )
    args = parser.parse_args(argv)

    settings = get_settings()
    details = load_warn_fail_details(settings)
    clusters = cluster_rows(details)

    if args.check:
        clusters = [
            c
            for c in clusters
            if any(cid.startswith(args.check) for cid in c.check_ids)
        ]
    if args.cluster_id:
        clusters = [c for c in clusters if c.root_cause_id == args.cluster_id]
    if args.max_clusters > 0:
        clusters = clusters[: args.max_clusters]

    reports_dir = settings.reports_dir
    manifest_path = reports_dir / "manifest.json"
    manifest = load_manifest(manifest_path)
    existing_by_id = {
        e["root_cause_id"]: e for e in manifest.get("entries", [])
    }

    conn = open_duckdb()
    limiter = RateLimiter()
    entries: list[ManifestEntry] = []
    total_tokens = 0

    print(
        f"[qa-triage] {len(details)} warn/fail detail rows -> {len(clusters)} clusters",
        file=sys.stderr,
    )

    for cluster in clusters:
        if not cluster.rows and cluster.related_cluster_ids:
            evidence = {
                "root_cause_id": cluster.root_cause_id,
                "title": cluster.title,
                "check_ids": cluster.check_ids,
                "severity": cluster.severity,
                "row_count": cluster.row_count,
                "cluster_key": cluster.cluster_key,
                "related_cluster_ids": cluster.related_cluster_ids,
                "note": "Parent cluster aggregating child reports",
            }
        else:
            evidence = build_evidence(cluster, settings, conn)
        input_hash = evidence_hash(evidence)
        tokens = estimate_tokens(evidence)
        total_tokens += tokens

        report_name = f"{cluster.root_cause_id}.md"
        report_path = reports_dir / report_name

        cached = existing_by_id.get(cluster.root_cause_id)
        if (
            not args.force
            and cached
            and cached.get("input_hash") == input_hash
            and report_path.exists()
            and not args.dry_run
        ):
            print(
                f"[qa-triage] cache hit {cluster.root_cause_id} ({cluster.row_count} rows)",
                file=sys.stderr,
            )
            entries.append(ManifestEntry(**cached))
            continue

        print(
            f"[qa-triage] {cluster.root_cause_id}: {cluster.row_count} rows, "
            f"~{tokens} tokens, checks={cluster.check_ids}",
            file=sys.stderr,
        )

        if args.dry_run:
            markdown = fallback_report(
                cluster,
                evidence_note=f"Evidence hash: `{input_hash}` (~{tokens} tokens).",
            )
        else:
            api_key = settings.mistral_api_token
            if not api_key:
                print(
                    "[qa-triage] MISTRAL_API_TOKEN not set; use --dry-run or set token in .env",
                    file=sys.stderr,
                )
                return 2
            limiter.wait()
            markdown = synthesize_report(evidence, api_key)

        write_report(report_path, markdown)
        entry = ManifestEntry(
            root_cause_id=cluster.root_cause_id,
            title=cluster.title,
            check_ids=cluster.check_ids,
            severity=cluster.severity,
            row_count=cluster.row_count,
            cluster_key=cluster.cluster_key,
            input_hash=input_hash,
            report_path=str(report_path.relative_to(settings.qa_dir)),
            related_cluster_ids=cluster.related_cluster_ids,
        )
        entries.append(entry)

    conn.close()
    write_index(reports_dir / "index.md", entries)
    save_manifest(
        manifest_path,
        {
            "entries": [entry.__dict__ for entry in entries],
            "detail_rows": len(details),
            "cluster_count": len(clusters),
            "estimated_tokens": total_tokens,
            "dry_run": args.dry_run,
        },
    )

    _print_cluster_table(clusters)
    print(
        f"[qa-triage] wrote {len(entries)} reports to {reports_dir} "
        f"(~{total_tokens} evidence tokens)",
        file=sys.stderr,
    )
    return 0


def _print_cluster_table(clusters) -> None:
    print(f"\n{'id':<45} {'sev':<6} {'rows':>6}  checks")
    print("-" * 80)
    for c in clusters:
        checks = ",".join(c.check_ids)
        print(f"{c.root_cause_id:<45} {c.severity:<6} {c.row_count:>6}  {checks}")


if __name__ == "__main__":
    raise SystemExit(main())
