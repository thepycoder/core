from __future__ import annotations

from typing import Any

from pydantic import BaseModel, Field


class HealthFileStatus(BaseModel):
    path: str
    exists: bool


class HealthResponse(BaseModel):
    data_dir: str
    cache_dir: str
    duckdb_ok: bool
    viewer_version: str = ""
    files: list[HealthFileStatus]
    warnings: list[str]


class TypeCount(BaseModel):
    type: str
    count: int


class StatsResponse(BaseModel):
    node_count: int
    edge_count: int
    artifact_count: int
    nodes_by_type: list[TypeCount]
    edges_by_type: list[TypeCount]


class IssueSample(BaseModel):
    data: dict[str, Any] = Field(default_factory=dict)
    label: str = ""
    action: str = "context"
    node_type: str = ""
    node_id: str = ""
    unresolved_bucket: str = ""
    unresolved_reason: str = ""
    artifact_id: str = ""
    source_url: str = ""
    cache_path: str = ""
    session_id: str = ""
    meeting_kind: str = ""
    meeting_id: str = ""
    source_block: str = ""


class Issue(BaseModel):
    id: str
    severity: str
    count: int
    summary: str
    samples: list[IssueSample] = Field(default_factory=list)


class IssuesResponse(BaseModel):
    issues: list[Issue]


class SearchResult(BaseModel):
    id: str
    type: str
    label: str
    degree_in: int
    degree_out: int
    source: str = "node"
    score: int = 0
    subtitle: str = ""
    context_type: str | None = None
    context_id: str | None = None
    context_label: str | None = None


class SearchResponse(BaseModel):
    results: list[SearchResult]


class GraphNode(BaseModel):
    id: str
    label: str
    type: str
    orphan: bool = False
    source_url: str = ""
    cache_path: str = ""


class GraphEdge(BaseModel):
    id: str
    source: str
    target: str
    type: str
    confidence: float = 1.0
    source_url: str = ""
    cache_path: str = ""
    source_artifact_id: str = ""


class GraphElements(BaseModel):
    nodes: list[GraphNode]
    edges: list[GraphEdge]


class SubgraphMeta(BaseModel):
    truncated: bool
    node_count: int
    edge_count: int


class SubgraphResponse(BaseModel):
    elements: GraphElements
    meta: SubgraphMeta


class ExpandRequest(BaseModel):
    node_ids: list[str]
    edge_types: list[str] | None = None


class EdgeGroup(BaseModel):
    edge_type: str
    count: int
    samples: list[dict[str, Any]] = Field(default_factory=list)


class PreviewField(BaseModel):
    label: str
    value: str
    link: str | None = None


class PreviewRelated(BaseModel):
    type: str
    id: str
    label: str


class EntityPreview(BaseModel):
    title: str | None = None
    fields: list[PreviewField] = Field(default_factory=list)
    content: str | None = None
    content_label: str | None = None
    related: list[PreviewRelated] = Field(default_factory=list)


class VoteCastMember(BaseModel):
    person_id: str | None = None
    label: str
    raw_name: str = ""
    confidence: float = 1.0
    unresolved: bool = False


class VotePositionGroup(BaseModel):
    position: str
    headline_count: int = 0
    members: list[VoteCastMember] = Field(default_factory=list)


class VoteBreakdown(BaseModel):
    groups: list[VotePositionGroup] = Field(default_factory=list)


class UtteranceGroup(BaseModel):
    agenda_id: str = ""
    title: str
    item_kind: str = ""
    utterances: list[dict[str, Any]] = Field(default_factory=list)


class NodeDetailResponse(BaseModel):
    id: str
    type: str
    label: str
    source_url: str
    cache_path: str
    in_edges: list[EdgeGroup]
    out_edges: list[EdgeGroup]
    preview: EntityPreview | None = None
    utterances: list[dict[str, Any]] = Field(default_factory=list)
    utterance_groups: list[UtteranceGroup] = Field(default_factory=list)
    utterance_section_title: str | None = None
    vote_reconciliation: dict[str, Any] | None = None
    vote_breakdown: VoteBreakdown | None = None
    source_evidence: list[SourceEvidence] = Field(default_factory=list)


class NodeLink(BaseModel):
    edge_type: str
    from_type: str
    from_id: str
    to_type: str
    to_id: str
    role: str = ""
    confidence: float
    neighbor_label: str
    neighbor_type: str
    neighbor_id: str
    source_url: str = ""
    cache_path: str = ""


class NodeLinksResponse(BaseModel):
    direction: str
    edge_type: str | None
    total: int
    limit: int
    offset: int
    links: list[NodeLink]


class EdgeDetailResponse(BaseModel):
    edge_type: str
    from_type: str
    from_id: str
    to_type: str
    to_id: str
    role: str = ""
    source_artifact_id: str
    source_url: str
    cache_path: str
    confidence: float
    properties_json: str = ""
    artifact: dict[str, Any] | None = None


class UnresolvedRow(BaseModel):
    raw_name: str
    reason: str
    source_bucket: str
    role: str
    context_id: str
    context_label: str
    source_url: str
    cache_path: str


class UnresolvedResponse(BaseModel):
    rows: list[UnresolvedRow]
    total: int


class ArtifactResponse(BaseModel):
    source_artifact_id: str
    source_url: str
    cache_path: str
    source_content_hash: str
    block_parser_version: str
    extractor_version: str
    scraped_at: str


class SourceEvidence(BaseModel):
    span_id: str
    session_id: str
    meeting_kind: str
    meeting_id: str
    entity_type: str
    entity_id: str
    span_role: str
    block_start: int
    block_end: int
    coverage_kind: str
    field_names: str = ""
    confidence: float = 0.0
    extractor: str = ""
    block_parser_version: str = ""
    extractor_version: str = ""
    source_url: str = ""
    cache_path: str = ""
    validation_status: str = ""
    unresolved_reason: str = ""


class ReportSpan(BaseModel):
    span_id: str
    artifact_id: str = ""
    source_content_hash: str = ""
    entity_type: str
    entity_id: str
    span_role: str
    block_start: int
    block_end: int
    coverage_kind: str
    field_names: str = ""
    confidence: float = 0.0
    extractor: str = ""
    block_parser_version: str = ""
    extractor_version: str = ""
    source_url: str = ""
    cache_path: str = ""
    validation_status: str = "valid"
    unresolved_reason: str = ""


class ReportBlock(BaseModel):
    block_index: int
    block_type: str
    text: str
    structured: dict[str, Any] = Field(default_factory=dict)
    word_count: int
    has_oraspr: bool = False
    artifact_id: str = ""
    source_content_hash: str = ""
    content_hash: str = ""
    block_parser_version: str = ""
    extractor_version: str = ""
    spans: list[ReportSpan] = Field(default_factory=list)
    has_extraction: bool = False
    has_scope: bool = False
    has_invalid: bool = False
    has_stale: bool = False


class ReportCoverageStats(BaseModel):
    total_words: int
    covered_words: int
    ratio: float
    valid_span_count: int = 0
    invalid_span_count: int = 0


class ReportDiagnostic(BaseModel):
    code: str
    state: str
    message: str
    count: int = 1
    span_ids: list[str] = Field(default_factory=list)


class ReportCoverageResponse(BaseModel):
    session_id: str
    meeting_kind: str
    meeting_id: str
    blocks: list[ReportBlock]
    coverage: ReportCoverageStats
    cache_path: str = ""
    source_url: str = ""
    block_parser_version: str = ""
    extractor_version: str = ""
    derived_data_status: str = "available"
    diagnostics: list[ReportDiagnostic] = Field(default_factory=list)


class ReportMeeting(BaseModel):
    session_id: str
    meeting_kind: str
    meeting_id: str
    cache_path: str = ""
    source_url: str = ""


class ReportMeetingsResponse(BaseModel):
    meetings: list[ReportMeeting]


class BrowseCategory(BaseModel):
    id: str
    label: str
    description: str
    node_type: str
    count: int


class BrowseCategoriesResponse(BaseModel):
    categories: list[BrowseCategory]


class BrowseItem(BaseModel):
    id: str
    type: str
    label: str
    subtitle: str = ""
    sort_key: str = ""


class BrowseResponse(BaseModel):
    category: str
    total: int
    limit: int
    offset: int
    items: list[BrowseItem]
