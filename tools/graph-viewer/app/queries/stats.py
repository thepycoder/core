from app.models import StatsResponse, TypeCount


def fetch_stats(conn) -> StatsResponse:
    node_count = conn.execute("SELECT count(*) FROM nodes").fetchone()[0]
    edge_count = conn.execute("SELECT count(*) FROM edges").fetchone()[0]
    artifact_count = conn.execute("SELECT count(*) FROM artifacts").fetchone()[0]

    nodes_by_type = [
        TypeCount(type=row[0], count=row[1])
        for row in conn.execute(
            """
            SELECT node_type, count(*) AS n
            FROM nodes
            GROUP BY 1
            ORDER BY n DESC, node_type
            """
        ).fetchall()
    ]

    edges_by_type = [
        TypeCount(type=row[0], count=row[1])
        for row in conn.execute(
            """
            SELECT edge_type, count(*) AS n
            FROM edges
            GROUP BY 1
            ORDER BY n DESC, edge_type
            """
        ).fetchall()
    ]

    return StatsResponse(
        node_count=node_count,
        edge_count=edge_count,
        artifact_count=artifact_count,
        nodes_by_type=nodes_by_type,
        edges_by_type=edges_by_type,
    )
