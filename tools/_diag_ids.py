#!/usr/bin/env python3
import duckdb

c = duckdb.connect()
print("=== commission 105 question nodes ===")
print(c.execute("""
SELECT node_id FROM read_parquet('data/graph/nodes.parquet')
WHERE node_type='Question' AND node_id LIKE '56_commission_105_%'
ORDER BY node_id
""").fetchall())
print("=== commission 105 utterance item_ids ===")
print(c.execute("""
SELECT DISTINCT item_id FROM read_parquet('data/normalized/utterances.parquet')
WHERE meeting_id='105' AND meeting_kind='commission' AND item_kind='question'
ORDER BY item_id
""").fetchall())
print("=== staging questions 105 ===")
print(c.execute("""
SELECT question_id FROM read_parquet('data/sessions/56/commission/questions.parquet')
WHERE meeting_id=105 ORDER BY question_id
""").fetchall())
print("=== session utterances 105 sample ===")
print(c.execute("""
SELECT DISTINCT item_id FROM read_parquet('data/sessions/56/commission/utterances.parquet')
WHERE meeting_id=105 AND item_kind='question' ORDER BY item_id LIMIT 5
""").fetchall())
print("=== off-by-one pattern count ===")
print(c.execute("""
WITH orphans AS (
  SELECT regexp_extract(entity_id, '->Question:(.+)$', 1) AS target_id
  FROM read_parquet('data/qa/meeting_report_check_details.parquet')
  WHERE check_id='graph.edge_endpoints_exist' AND entity_id LIKE '%->Question:%'
),
nodes AS (
  SELECT node_id FROM read_parquet('data/graph/nodes.parquet') WHERE node_type='Question'
)
SELECT count(*) FROM orphans o
WHERE NOT EXISTS (SELECT 1 FROM nodes n WHERE n.node_id = o.target_id)
  AND EXISTS (
    SELECT 1 FROM nodes n WHERE n.node_id = regexp_replace(o.target_id, '_(\\d+)$', '_' || (CAST(regexp_extract(o.target_id, '_(\\d+)$', 1) AS INT)-1)::VARCHAR)
  )
""").fetchall())
