from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class DetailRow:
    check_id: str
    severity: str
    status: str
    session_id: str
    meeting_kind: str
    meeting_id: str
    entity_type: str
    entity_id: str
    expected: str
    actual: str
    message: str
    source_url: str
    cache_path: str
    source_block: str
    created_at: str

    @classmethod
    def from_mapping(cls, row: dict[str, Any]) -> DetailRow:
        return cls(
            check_id=str(row.get("check_id") or ""),
            severity=str(row.get("severity") or ""),
            status=str(row.get("status") or ""),
            session_id=str(row.get("session_id") or ""),
            meeting_kind=str(row.get("meeting_kind") or ""),
            meeting_id=str(row.get("meeting_id") or ""),
            entity_type=str(row.get("entity_type") or ""),
            entity_id=str(row.get("entity_id") or ""),
            expected=str(row.get("expected") or ""),
            actual=str(row.get("actual") or ""),
            message=str(row.get("message") or ""),
            source_url=str(row.get("source_url") or ""),
            cache_path=str(row.get("cache_path") or ""),
            source_block=str(row.get("source_block") or ""),
            created_at=str(row.get("created_at") or ""),
        )

    def as_dict(self) -> dict[str, str]:
        return {
            "check_id": self.check_id,
            "severity": self.severity,
            "status": self.status,
            "session_id": self.session_id,
            "meeting_kind": self.meeting_kind,
            "meeting_id": self.meeting_id,
            "entity_type": self.entity_type,
            "entity_id": self.entity_id,
            "expected": self.expected,
            "actual": self.actual,
            "message": self.message,
            "source_url": self.source_url,
            "cache_path": self.cache_path,
            "source_block": self.source_block,
        }


@dataclass
class Cluster:
    root_cause_id: str
    title: str
    check_ids: list[str]
    severity: str
    row_count: int
    cluster_key: str
    rows: list[DetailRow] = field(default_factory=list)
    related_cluster_ids: list[str] = field(default_factory=list)

    @property
    def primary_check_id(self) -> str:
        return self.check_ids[0]


@dataclass
class ManifestEntry:
    root_cause_id: str
    title: str
    check_ids: list[str]
    severity: str
    row_count: int
    cluster_key: str
    input_hash: str
    report_path: str
    related_cluster_ids: list[str] = field(default_factory=list)
