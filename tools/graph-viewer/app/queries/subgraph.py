from __future__ import annotations

from app.models import (
    ExpandRequest,
    GraphEdge,
    GraphElements,
    GraphNode,
    SubgraphMeta,
    SubgraphResponse,
)

MAX_NODES = 500
MAX_EDGES = 2000


def compound_id(node_type: str, node_id: str) -> str:
    return f"{node_type}:{node_id}"


def split_compound_id(value: str) -> tuple[str, str]:
    node_type, _, node_id = value.partition(":")
    if not node_id:
        raise ValueError(f"Invalid compound node id: {value}")
    return node_type, node_id


def _edge_filter_sql(edge_types: list[str] | None) -> tuple[str, list]:
    if not edge_types:
        return "", []
    placeholders = ", ".join("?" for _ in edge_types)
    return f" AND e.edge_type IN ({placeholders})", list(edge_types)


def _fetch_node(conn, node_type: str, node_id: str) -> GraphNode | None:
    row = conn.execute(
        """
        SELECT node_type, node_id, label, source_url, cache_path
        FROM nodes
        WHERE node_type = ? AND node_id = ?
        LIMIT 1
        """,
        [node_type, node_id],
    ).fetchone()
    if not row:
        return GraphNode(
            id=compound_id(node_type, node_id),
            label=node_id,
            type=node_type,
            orphan=True,
        )
    return GraphNode(
        id=compound_id(row[0], row[1]),
        label=row[2] or row[1],
        type=row[0],
        orphan=False,
        source_url=row[3] or "",
        cache_path=row[4] or "",
    )


def _expand_frontier(
    conn,
    frontier: set[tuple[str, str]],
    edge_types: list[str] | None,
    collected_edges: dict[str, GraphEdge],
    node_keys: set[tuple[str, str]],
) -> set[tuple[str, str]]:
    if not frontier:
        return set()

    next_frontier: set[tuple[str, str]] = set()
    filter_sql, filter_params = _edge_filter_sql(edge_types)

    for node_type, node_id in frontier:
        rows = conn.execute(
            f"""
            SELECT edge_type, from_type, from_id, to_type, to_id,
                   source_artifact_id, source_url, cache_path, confidence
            FROM edges e
            WHERE (
                (e.from_type = ? AND e.from_id = ?)
                OR (e.to_type = ? AND e.to_id = ?)
            )
            {filter_sql}
            """,
            [node_type, node_id, node_type, node_id, *filter_params],
        ).fetchall()

        for row in rows:
            if len(collected_edges) >= MAX_EDGES:
                return next_frontier

            edge_key = "|".join(row[:5])
            if edge_key in collected_edges:
                continue

            from_key = (row[1], row[2])
            to_key = (row[3], row[4])
            collected_edges[edge_key] = GraphEdge(
                id=edge_key,
                source=compound_id(row[1], row[2]),
                target=compound_id(row[3], row[4]),
                type=row[0],
                confidence=row[8] if row[8] is not None else 1.0,
                source_url=row[6] or "",
                cache_path=row[7] or "",
                source_artifact_id=row[5] or "",
            )
            node_keys.add(from_key)
            node_keys.add(to_key)

            if from_key != (node_type, node_id):
                next_frontier.add(from_key)
            if to_key != (node_type, node_id):
                next_frontier.add(to_key)

    return next_frontier


def fetch_subgraph(
    conn,
    seed_type: str,
    seed_id: str,
    hops: int = 1,
    edge_types: list[str] | None = None,
) -> SubgraphResponse:
    hops = max(1, min(hops, 2))
    seed_key = (seed_type, seed_id)
    node_keys: set[tuple[str, str]] = {seed_key}
    collected_edges: dict[str, GraphEdge] = {}
    frontier = {seed_key}
    truncated = False

    for _ in range(hops):
        if len(node_keys) >= MAX_NODES:
            truncated = True
            break
        next_frontier = _expand_frontier(
            conn, frontier, edge_types, collected_edges, node_keys
        )
        frontier = {key for key in next_frontier if key not in node_keys}
        node_keys.update(frontier)
        if len(node_keys) >= MAX_NODES:
            truncated = True
            break

    nodes: list[GraphNode] = []
    for node_type, node_id in sorted(node_keys):
        if len(nodes) >= MAX_NODES:
            truncated = True
            break
        row = conn.execute(
            """
            SELECT label, source_url, cache_path
            FROM nodes
            WHERE node_type = ? AND node_id = ?
            """,
            [node_type, node_id],
        ).fetchone()
        if row:
            nodes.append(
                GraphNode(
                    id=compound_id(node_type, node_id),
                    label=row[0] or node_id,
                    type=node_type,
                    orphan=False,
                    source_url=row[1] or "",
                    cache_path=row[2] or "",
                )
            )
        else:
            nodes.append(
                GraphNode(
                    id=compound_id(node_type, node_id),
                    label=node_id,
                    type=node_type,
                    orphan=True,
                )
            )

    edges = list(collected_edges.values())
    if len(edges) > MAX_EDGES:
        edges = edges[:MAX_EDGES]
        truncated = True

    return SubgraphResponse(
        elements=GraphElements(nodes=nodes, edges=edges),
        meta=SubgraphMeta(
            truncated=truncated,
            node_count=len(nodes),
            edge_count=len(edges),
        ),
    )


def fetch_expand(conn, request: ExpandRequest) -> SubgraphResponse:
    node_keys: set[tuple[str, str]] = set()
    for compound in request.node_ids:
        node_keys.add(split_compound_id(compound))

    collected_edges: dict[str, GraphEdge] = {}
    truncated = False
    frontier = set(node_keys)
    _expand_frontier(
        conn,
        frontier,
        request.edge_types,
        collected_edges,
        node_keys,
    )

    nodes: list[GraphNode] = []
    for node_type, node_id in sorted(node_keys):
        if len(nodes) >= MAX_NODES:
            truncated = True
            break
        nodes.append(_fetch_node(conn, node_type, node_id))

    edges = list(collected_edges.values())[:MAX_EDGES]
    if len(collected_edges) > MAX_EDGES:
        truncated = True

    return SubgraphResponse(
        elements=GraphElements(nodes=nodes, edges=edges),
        meta=SubgraphMeta(
            truncated=truncated,
            node_count=len(nodes),
            edge_count=len(edges),
        ),
    )
