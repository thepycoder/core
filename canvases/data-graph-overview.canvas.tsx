import {
  Button,
  Card,
  CardBody,
  CardHeader,
  CollapsibleSection,
  computeDAGLayout,
  Grid,
  H1,
  H2,
  H3,
  Pill,
  Row,
  Select,
  Spacer,
  Stack,
  Stat,
  Swatch,
  Table,
  Text,
  UsageBar,
  useCanvasAction,
  useCanvasState,
  useHostTheme,
  type Color,
} from "cursor/canvas";

// ── Node catalog (from DATA_GRAPH.md) ────────────────────────────────────────

type Domain = "foundation" | "identity" | "proceedings" | "legislative" | "enrichment";
type Status = "working" | "scraped" | "partial" | "planned";

type NodeDef = {
  id: string;
  label: string;
  domain: Domain;
  status: Status;
  idKey: string;
  note: string;
};

const NODES: NodeDef[] = [
  { id: "Session", label: "Session", domain: "foundation", status: "scraped", idKey: "session_id", note: "Legislative term anchor" },
  { id: "Person", label: "Person", domain: "identity", status: "working", idKey: "person_id", note: "Chamber MPs (cvview-backed)" },
  { id: "ExternalPerson", label: "ExternalPerson", domain: "identity", status: "working", idKey: "external_person_id", note: "57 entities; ministers, experts, roles" },
  { id: "Party", label: "Party", domain: "identity", status: "working", idKey: "slug / name", note: "Fraction; time-bounded membership" },
  { id: "Commission", label: "Commission", domain: "identity", status: "working", idKey: "name / enum", note: "Committee; links to meetings & dossiers" },
  { id: "Meeting", label: "Meeting", domain: "proceedings", status: "scraped", idKey: "{session, kind, meeting_id}", note: "Plenary + commission integraal" },
  { id: "AgendaItem", label: "AgendaItem", domain: "proceedings", status: "partial", idKey: "{meeting_id, seq}", note: "Report headings; heuristic boundaries" },
  { id: "Utterance", label: "Utterance", domain: "proceedings", status: "working", idKey: "{meeting}_{agenda}_{turn}", note: "43,431 full-session rows; PART_OF Question/Hearing/Interpellation" },
  { id: "Question", label: "Question", domain: "proceedings", status: "working", idKey: "{session}_{kind}_{meeting}_{seq} | 56_written_{DOCNAME}", note: "Oral + QRVA written; oral-written inline bodies" },
  { id: "Answer", label: "Answer", domain: "proceedings", status: "working", idKey: "56_qrva_{route}_a{slot} | {question_id}_a1", note: "QRVA slots + integraal oral-written blocks" },
  { id: "Vote", label: "Vote", domain: "proceedings", status: "working", idKey: "{meeting_id, vote_id}", note: "Plenary integraal + appendix" },
  { id: "VoteCast", label: "VoteCast", domain: "proceedings", status: "partial", idKey: "{vote_id, person_id, position}", note: "Graph uses CAST Person→Vote (no node yet)" },
  { id: "Motion", label: "Motion", domain: "proceedings", status: "planned", idKey: "motion id + context", note: "Referenced in vote parsing, not modelled" },
  { id: "Hearing", label: "Hearing", domain: "proceedings", status: "working", idKey: "{session}_{kind}_{meeting}_{seq}", note: "Commission hoorzitting/audition; hearings.parquet" },
  { id: "Interpellation", label: "Interpellation", domain: "proceedings", status: "working", idKey: "{session}_{kind}_{meeting}_{seq}", note: "Plenary Interpellatie van; internal_ids …I" },
  { id: "Dossier", label: "Dossier", domain: "legislative", status: "working", idKey: "{session_id}/{number}", note: "1,640 nodes; FLWB browse + plenary refs" },
  { id: "Document", label: "Document", domain: "legislative", status: "scraped", idKey: "FLWB doc id", note: "Metadata scraped; body via PDF pipeline" },
  { id: "Amendment", label: "Amendment", domain: "legislative", status: "scraped", idKey: "doc id + dossier", note: "Subdocument typed AMENDEMENT" },
  { id: "Report", label: "Report", domain: "legislative", status: "scraped", idKey: "doc id + dossier", note: "VERSLAG subdocuments; PDF-heavy" },
  { id: "Topic", label: "Topic", domain: "legislative", status: "partial", idKey: "Eurovoc id + label", note: "On dossiers; utterance tagging future" },
  { id: "LobbyOrg", label: "LobbyOrg", domain: "enrichment", status: "scraped", idKey: "name", note: "301 orgs from lobbyregister.pdf" },
  { id: "Remuneration", label: "Remuneration", domain: "enrichment", status: "scraped", idKey: "{person, year, mandate}", note: "regimand.be; name match only" },
  { id: "MediaRecording", label: "MediaRecording", domain: "enrichment", status: "planned", idKey: "media id", note: "media.dekamer.be; fuzzy date match" },
  { id: "InterventionAnalysis", label: "InterventionAnalysis", domain: "enrichment", status: "planned", idKey: "dossier / meeting ref", note: "Structured speaker/topic data" },
];

// ── Edge catalog ─────────────────────────────────────────────────────────────

type EdgeDef = {
  type: string;
  from: string;
  to: string;
  status: Status;
  note: string;
};

const EDGES: EdgeDef[] = [
  { type: "MEMBER_OF", from: "Person", to: "Party", status: "working", note: "Per-session; time range on CV" },
  { type: "MEMBER_OF", from: "Person", to: "Commission", status: "working", note: "Permanent vs replacement" },
  { type: "HOLDS_ROLE", from: "Person", to: "Meeting", status: "partial", note: "Chair regex for commission" },
  { type: "HOLDS_ROLE", from: "Person", to: "Dossier", status: "partial", note: "Rapporteur on dossier fiche" },
  { type: "ATTENDED", from: "Person", to: "Meeting", status: "planned", note: "Opening/closing attendance lists" },
  { type: "SPOKE", from: "Person", to: "Utterance", status: "working", note: "40,311 edges; chairs/unresolved skipped" },
  { type: "SPOKE", from: "ExternalPerson", to: "Utterance", status: "working", note: "Ministers, experts, roles via ActorResolver" },
  { type: "PART_OF", from: "Utterance", to: "Meeting", status: "working", note: "61,057 edges across all item kinds" },
  { type: "PART_OF", from: "Utterance", to: "Question", status: "working", note: "When item_kind = question" },
  { type: "PART_OF", from: "Utterance", to: "Hearing", status: "working", note: "When item_kind = hearing" },
  { type: "PART_OF", from: "Utterance", to: "Interpellation", status: "working", note: "When item_kind = interpellation" },
  { type: "PART_OF", from: "Hearing", to: "Meeting", status: "working", note: "Proceeding entity under meeting" },
  { type: "PART_OF", from: "Interpellation", to: "Meeting", status: "working", note: "Proceeding entity under meeting" },
  { type: "INTERPELLED", from: "Person", to: "Interpellation", status: "working", note: "From interpellators field" },
  { type: "RESPONDED", from: "Person", to: "Interpellation", status: "working", note: "Named respondents" },
  { type: "RESPONDED", from: "ExternalPerson", to: "Interpellation", status: "working", note: "Portfolio-title respondents" },
  { type: "INVITED", from: "ExternalPerson", to: "Hearing", status: "partial", note: "Witnesses when parseable" },
  { type: "ASKED", from: "Person", to: "Question", status: "working", note: "Oral + written_asked (actr id first)" },
  { type: "ADDRESSED_TO", from: "Question", to: "ExternalPerson", status: "working", note: "QRVA dept routes; metadata in properties_json" },
  { type: "HAS_ANSWER", from: "Question", to: "Answer", status: "working", note: "One edge per answer slot / inline block" },
  { type: "ANSWERED_BY", from: "Answer", to: "ExternalPerson", status: "working", note: "Dept role + named minister when resolvable" },
  { type: "ANSWERED_BY", from: "Answer", to: "Person", status: "working", note: "Inline oral-written respondents" },
  { type: "REFERENCES", from: "Question", to: "Question", status: "working", note: "Exact oral ref merges written QRVA → oral Question" },
  { type: "ANSWERED", from: "ExternalPerson", to: "Question", status: "working", note: "Legacy header respondents (oral)" },
  { type: "ANSWERED", from: "Person", to: "Question", status: "working", note: "Legacy header respondents (oral)" },
  { type: "ABOUT", from: "Question", to: "Topic", status: "planned", note: "Free text; summarizer exists" },
  { type: "LINKED_TO", from: "Question", to: "Dossier", status: "partial", note: "Commission questions carry dossier ids" },
  { type: "AUTHORED", from: "Person", to: "Document", status: "working", note: "6,798 via ActorResolver" },
  { type: "REFERENCES", from: "Meeting", to: "Dossier", status: "partial", note: "Regex from proposition/vote titles" },
  { type: "DISCUSSED_IN", from: "Dossier", to: "Meeting", status: "planned", note: "Dossier fiche calendar not ingested" },
  { type: "VOTED_ON", from: "Vote", to: "Dossier", status: "working", note: "145 orphan refs to partial ids" },
  { type: "VOTED_ON", from: "Vote", to: "Document", status: "working", note: "From vote title parsing" },
  { type: "VOTED_ON", from: "Vote", to: "Motion", status: "partial", note: "motion_id partially parsed" },
  { type: "CAST", from: "Person", to: "Vote", status: "working", note: "189,496 casts; 6 vote mismatches" },
  { type: "TAGGED_WITH", from: "Dossier", to: "Topic", status: "working", note: "Eurovoc on dossier fiche" },
  { type: "TAGGED_WITH", from: "Utterance", to: "Topic", status: "planned", note: "NLP / intervention analysis" },
  { type: "SUBMITTED", from: "Document", to: "Dossier", status: "working", note: "FLWB hierarchy" },
  { type: "DECLARES_INTEREST", from: "Person", to: "LobbyOrg", status: "planned", note: "Lobby register not linked" },
  { type: "EARNED", from: "Person", to: "Remuneration", status: "scraped", note: "Name match only" },
  { type: "RECORDED_IN", from: "Meeting", to: "MediaRecording", status: "planned", note: "Not scraped" },
];

// Layout edges: structural spine for DAG positioning (may include cycles → back-edges)
const LAYOUT_EDGES = [
  { from: "Session", to: "Meeting" },
  { from: "Session", to: "Person" },
  { from: "Session", to: "ExternalPerson" },
  { from: "Session", to: "Commission" },
  { from: "Session", to: "Party" },
  { from: "Session", to: "Dossier" },
  { from: "Person", to: "Party" },
  { from: "Person", to: "Commission" },
  { from: "Person", to: "Meeting" },
  { from: "Meeting", to: "AgendaItem" },
  { from: "Meeting", to: "Vote" },
  { from: "Meeting", to: "MediaRecording" },
  { from: "AgendaItem", to: "Utterance" },
  { from: "AgendaItem", to: "Question" },
  { from: "AgendaItem", to: "Interpellation" },
  { from: "AgendaItem", to: "Hearing" },
  { from: "Person", to: "Utterance" },
  { from: "ExternalPerson", to: "Utterance" },
  { from: "Person", to: "Question" },
  { from: "ExternalPerson", to: "Question" },
  { from: "Question", to: "Answer" },
  { from: "Question", to: "Topic" },
  { from: "Question", to: "Dossier" },
  { from: "Utterance", to: "Topic" },
  { from: "Dossier", to: "Document" },
  { from: "Dossier", to: "Topic" },
  { from: "Document", to: "Amendment" },
  { from: "Document", to: "Report" },
  { from: "Vote", to: "Dossier" },
  { from: "Vote", to: "Document" },
  { from: "Vote", to: "Motion" },
  { from: "Person", to: "VoteCast" },
  { from: "VoteCast", to: "Vote" },
  { from: "Person", to: "Document" },
  { from: "Person", to: "LobbyOrg" },
  { from: "Person", to: "Remuneration" },
  { from: "Meeting", to: "InterventionAnalysis" },
  { from: "Meeting", to: "Dossier" },
  { from: "Dossier", to: "Meeting" },
];

const DOMAIN_LABELS: Record<Domain, string> = {
  foundation: "Foundation",
  identity: "Identity",
  proceedings: "Proceedings",
  legislative: "Legislative",
  enrichment: "Enrichment",
};

const DOMAIN_COLORS: Record<Domain, Color> = {
  foundation: "purple",
  identity: "blue",
  proceedings: "cyan",
  legislative: "green",
  enrichment: "orange",
};

const STATUS_LABELS: Record<Status, string> = {
  working: "In graph",
  scraped: "Scraped flat",
  partial: "Partial",
  planned: "Planned",
};

const STATUS_TONE: Record<Status, "success" | "info" | "warning" | "neutral"> = {
  working: "success",
  scraped: "info",
  partial: "warning",
  planned: "neutral",
};

const STATUS_STAT_TONE: Record<Status, "success" | "info" | "warning"> = {
  working: "success",
  scraped: "info",
  partial: "warning",
  planned: "info",
};

const STATUS_USAGE_COLOR: Record<Status, Color> = {
  working: "green",
  scraped: "blue",
  partial: "yellow",
  planned: "gray",
};

const DOMAIN_FILTER_OPTIONS = [
  { value: "all", label: "All domains" },
  { value: "foundation", label: "Foundation" },
  { value: "identity", label: "Identity" },
  { value: "proceedings", label: "Proceedings" },
  { value: "legislative", label: "Legislative" },
  { value: "enrichment", label: "Enrichment" },
];

const NODE_W = 128;
const NODE_H = 30;

function statusCounts(items: { status: Status }[]) {
  const c: Record<Status, number> = { working: 0, scraped: 0, partial: 0, planned: 0 };
  for (const item of items) c[item.status]++;
  return c;
}

function GraphDiagram({
  selectedId,
  onSelect,
  domainFilter,
}: {
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  domainFilter: string;
}) {
  const theme = useHostTheme();

  const visibleNodes =
    domainFilter === "all" ? NODES : NODES.filter((n) => n.domain === domainFilter);

  const visibleIds = new Set(visibleNodes.map((n) => n.id));

  const layout = computeDAGLayout({
    nodes: visibleNodes.map((n) => ({ id: n.id })),
    edges: LAYOUT_EDGES.filter((e) => visibleIds.has(e.from) && visibleIds.has(e.to)),
    direction: "horizontal",
    nodeWidth: NODE_W,
    nodeHeight: NODE_H,
    rankGap: 72,
    nodeGap: 20,
    padding: 32,
  });

  const nodeById = new Map(NODES.map((n) => [n.id, n]));
  const posById = new Map(layout.nodes.map((n) => [n.id, n]));

  const connected = new Set<string>();
  if (selectedId) {
    connected.add(selectedId);
    for (const e of EDGES) {
      if (e.from === selectedId) connected.add(e.to);
      if (e.to === selectedId) connected.add(e.from);
    }
  }

  const edgeHighlighted = (from: string, to: string) =>
    selectedId != null && (from === selectedId || to === selectedId);

  return (
    <div style={{ overflowX: "auto", overflowY: "hidden" }}>
      <svg
        width={layout.width}
        height={layout.height}
        style={{ display: "block", minWidth: layout.width }}
      >
        {/* Rank bands */}
        {layout.ranks.map((rank) => (
          <rect
            key={rank.rank}
            x={rank.x - 8}
            y={rank.y - 8}
            width={rank.width + 16}
            height={rank.height + 16}
            fill={theme.fill.quaternary}
            stroke={theme.stroke.tertiary}
            strokeWidth={1}
            rx={6}
          />
        ))}

        {/* Edges */}
        {layout.edges.map((edge, i) => {
          const hl = edgeHighlighted(edge.from, edge.to);
          const stroke = edge.isBackEdge
            ? theme.text.tertiary
            : hl
              ? theme.accent.primary
              : theme.stroke.secondary;
          const dash = edge.isBackEdge ? "5 4" : undefined;
          const opacity = selectedId && !hl ? 0.25 : edge.isBackEdge ? 0.5 : 0.85;
          const midX = (edge.sourceX + edge.targetX) / 2;
          const midY = (edge.sourceY + edge.targetY) / 2;
          return (
            <g key={`${edge.from}-${edge.to}-${i}`} opacity={opacity}>
              <line
                x1={edge.sourceX}
                y1={edge.sourceY}
                x2={midX}
                y2={edge.sourceY}
                stroke={stroke}
                strokeWidth={hl ? 2 : 1}
                strokeDasharray={dash}
              />
              <line
                x1={midX}
                y1={edge.sourceY}
                x2={midX}
                y2={edge.targetY}
                stroke={stroke}
                strokeWidth={hl ? 2 : 1}
                strokeDasharray={dash}
              />
              <line
                x1={midX}
                y1={edge.targetY}
                x2={edge.targetX}
                y2={edge.targetY}
                stroke={stroke}
                strokeWidth={hl ? 2 : 1}
                strokeDasharray={dash}
              />
              <polygon
                points={`${edge.targetX},${edge.targetY} ${edge.targetX - 5},${edge.targetY - 3} ${edge.targetX - 5},${edge.targetY + 3}`}
                fill={stroke}
              />
            </g>
          );
        })}

        {/* Nodes */}
        {layout.nodes.map((pos) => {
          const def = nodeById.get(pos.id);
          if (!def) return null;
          const isSelected = selectedId === pos.id;
          const isConnected = connected.has(pos.id);
          const dimmed = selectedId != null && !isSelected && !isConnected;
          const domainColor = theme.category[DOMAIN_COLORS[def.domain]];

          return (
            <g
              key={pos.id}
              opacity={dimmed ? 0.35 : 1}
              style={{ cursor: "pointer" }}
              onClick={() => onSelect(isSelected ? null : pos.id)}
            >
              <rect
                x={pos.x}
                y={pos.y}
                width={NODE_W}
                height={NODE_H}
                rx={4}
                fill={isSelected ? theme.fill.secondary : theme.bg.elevated}
                stroke={isSelected ? theme.accent.primary : theme.stroke.primary}
                strokeWidth={isSelected ? 2 : 1}
              />
              <rect
                x={pos.x}
                y={pos.y}
                width={4}
                height={NODE_H}
                rx={2}
                fill={domainColor}
              />
              <text
                x={pos.x + NODE_W / 2 + 2}
                y={pos.y + NODE_H / 2 + 4}
                textAnchor="middle"
                fontSize={11}
                fill={theme.text.primary}
                fontFamily="system-ui, sans-serif"
              >
                {def.label}
              </text>
              <circle
                cx={pos.x + NODE_W - 8}
                cy={pos.y + 8}
                r={3}
                fill={theme.category[STATUS_USAGE_COLOR[def.status]]}
              />
            </g>
          );
        })}
      </svg>
    </div>
  );
}

function NodeDetail({ nodeId }: { nodeId: string }) {
  const node = NODES.find((n) => n.id === nodeId);
  if (!node) return null;

  const inbound = EDGES.filter((e) => e.to === nodeId);
  const outbound = EDGES.filter((e) => e.from === nodeId);

  return (
    <Stack gap={8}>
      <Row gap={8} align="center">
        <Swatch color={DOMAIN_COLORS[node.domain]} />
        <Text weight="semibold">{node.label}</Text>
        <Pill tone={STATUS_TONE[node.status]} size="sm">
          {STATUS_LABELS[node.status]}
        </Pill>
      </Row>
      <Text tone="secondary" size="small">
        ID key: <Text as="span">{node.idKey}</Text>
      </Text>
      <Text tone="secondary" size="small">
        {node.note}
      </Text>
      {outbound.length > 0 && (
        <Stack gap={4}>
          <Text size="small" weight="semibold">
            Outgoing ({outbound.length})
          </Text>
          {outbound.map((e) => (
            <div key={`${e.type}-${e.to}`}>
              <Row gap={6} align="center">
                <Pill tone="neutral" size="sm">
                  {e.type}
                </Pill>
                <Text size="small" tone="secondary">
                  → {e.to}
                </Text>
              </Row>
            </div>
          ))}
        </Stack>
      )}
      {inbound.length > 0 && (
        <Stack gap={4}>
          <Text size="small" weight="semibold">
            Incoming ({inbound.length})
          </Text>
          {inbound.map((e) => (
            <div key={`${e.type}-${e.from}`}>
              <Row gap={6} align="center">
                <Text size="small" tone="secondary">
                  {e.from}
                </Text>
                <Pill tone="neutral" size="sm">
                  {e.type}
                </Pill>
              </Row>
            </div>
          ))}
        </Stack>
      )}
    </Stack>
  );
}

export default function DataGraphOverview() {
  const theme = useHostTheme();
  const dispatch = useCanvasAction();
  const [selectedNode, setSelectedNode] = useCanvasState<string | null>("selectedNode", null);
  const [domainFilter, setDomainFilter] = useCanvasState("domainFilter", "all");

  const nodeCounts = statusCounts(NODES);
  const edgeCounts = statusCounts(EDGES);

  const usageSegments = (["working", "scraped", "partial", "planned"] as Status[]).map((s) => ({
    id: s,
    value: nodeCounts[s],
    color: STATUS_USAGE_COLOR[s],
  }));
  const totalNodes = NODES.length;

  return (
    <Stack gap={20} style={{ padding: "4px 2px 24px" }}>
      <Stack gap={6}>
        <H1>Belgian Chamber Data Graph</H1>
        <Row gap={6} align="center" wrap>
          <Text tone="secondary">
            Canonical graph model for dekamer.be — nodes, edges, and pipeline coverage from
          </Text>
          <Button
            variant="ghost"
            onClick={() => dispatch({ type: "openFile", path: "DATA_GRAPH.md" })}
          >
            DATA_GRAPH.md
          </Button>
        </Row>
      </Stack>

      <Grid columns={4} gap={12}>
        <Stat label="Node types" value={String(NODES.length)} tone="info" />
        <Stat label="Edge types" value={String(EDGES.length)} tone="info" />
        <Stat label="Graph nodes (built)" value="55,282" tone="success" />
        <Stat label="Graph edges (built)" value="332,982" tone="success" />
      </Grid>

      <Card>
        <CardHeader trailing="23 node types">
          Implementation coverage
        </CardHeader>
        <CardBody>
          <Stack gap={10}>
            <UsageBar
              segments={usageSegments}
              total={totalNodes}
              topLeftLabel="Node implementation status"
              topRightLabel={`${totalNodes} node types`}
            />
            <Row gap={16} wrap>
              {(["working", "scraped", "partial", "planned"] as Status[]).map((s) => (
                <div key={s}>
                  <Row gap={6} align="center">
                    <Swatch color={STATUS_USAGE_COLOR[s]} />
                    <Text size="small" tone="secondary">
                      {STATUS_LABELS[s]}: {nodeCounts[s]}
                    </Text>
                  </Row>
                </div>
              ))}
            </Row>
            <Text size="small" tone="tertiary">
              Source: DATA_GRAPH.md coverage table · branch stage-viz · last build 2026-07-07
            </Text>
          </Stack>
        </CardBody>
      </Card>

      <H2>Graph structure</H2>
      <Text tone="secondary" size="small">
        Click a node to inspect its edges. Dashed lines are cycle back-edges. Colored left bar =
        domain; dot = implementation status.
      </Text>

      <Card>
        <CardHeader
          trailing={
            <Select
              value={domainFilter}
              onChange={setDomainFilter}
              options={DOMAIN_FILTER_OPTIONS}
            />
          }
        >
          Entity relationship diagram
        </CardHeader>
        <CardBody style={{ padding: 0 }}>
          <GraphDiagram
            selectedId={selectedNode}
            onSelect={setSelectedNode}
            domainFilter={domainFilter}
          />
        </CardBody>
      </Card>

      <Grid columns={2} gap={12}>
        <Card>
          <CardHeader>Domain legend</CardHeader>
          <CardBody>
            <Stack gap={8}>
              {(Object.keys(DOMAIN_LABELS) as Domain[]).map((d) => (
                <div key={d}>
                  <Row gap={8} align="center">
                    <Swatch color={DOMAIN_COLORS[d]} />
                    <Text size="small">{DOMAIN_LABELS[d]}</Text>
                    <Spacer />
                    <Text size="small" tone="tertiary">
                      {NODES.filter((n) => n.domain === d).length} nodes
                    </Text>
                  </Row>
                </div>
              ))}
            </Stack>
          </CardBody>
        </Card>

        <Card>
          <CardHeader>
            {selectedNode ? `Selected: ${selectedNode}` : "Select a node"}
          </CardHeader>
          <CardBody>
            {selectedNode ? (
              <NodeDetail nodeId={selectedNode} />
            ) : (
              <Text tone="secondary" size="small">
                Click any node in the diagram above to see its ID key, notes, and
                incoming/outgoing edge types.
              </Text>
            )}
          </CardBody>
        </Card>
      </Grid>

      <H2>Core spine</H2>
      <Text tone="secondary" size="small">
        The parliamentary data flows through this primary path — everything else hangs off
        identity, legislative files, or enrichment.
      </Text>
      <Card variant="borderless">
        <CardBody>
          <Row gap={6} align="center" wrap style={{ fontSize: 13, color: theme.text.secondary }}>
            <Pill tone="info">Session</Pill>
            <Text tone="tertiary">→</Text>
            <Pill tone="info">Person</Pill>
            <Text tone="tertiary">↔</Text>
            <Pill tone="info">Meeting</Pill>
            <Text tone="tertiary">→</Text>
            <Pill tone="info">Utterance</Pill>
            <Text tone="tertiary">↔</Text>
            <Pill tone="info">Question</Pill>
            <Text tone="tertiary">/</Text>
            <Pill tone="info">Vote</Pill>
            <Text tone="tertiary">↔</Text>
            <Pill tone="info">Dossier</Pill>
            <Text tone="tertiary">→</Text>
            <Pill tone="info">Document</Pill>
          </Row>
        </CardBody>
      </Card>

      <H2>Reference tables</H2>

      <CollapsibleSection title="All nodes" count={NODES.length} defaultOpen={false}>
        <Table
          headers={["Node", "Domain", "Status", "ID key", "Notes"]}
          rows={NODES.map((n) => [
            <Row gap={6} align="center">
              <Swatch color={DOMAIN_COLORS[n.domain]} />
              <Text size="small">{n.label}</Text>
            </Row>,
            DOMAIN_LABELS[n.domain],
            <Pill tone={STATUS_TONE[n.status]} size="sm">
              {STATUS_LABELS[n.status]}
            </Pill>,
            <Text size="small" tone="secondary">
              {n.idKey}
            </Text>,
            <Text size="small" tone="secondary">
              {n.note}
            </Text>,
          ])}
        />
      </CollapsibleSection>

      <CollapsibleSection title="All edges" count={EDGES.length} defaultOpen={false}>
        <Table
          headers={["Type", "From", "To", "Status", "Notes"]}
          rows={EDGES.map((e) => [
            <Pill tone="neutral" size="sm">
              {e.type}
            </Pill>,
            e.from,
            e.to,
            <Pill tone={STATUS_TONE[e.status]} size="sm">
              {STATUS_LABELS[e.status]}
            </Pill>,
            <Text size="small" tone="secondary">
              {e.note}
            </Text>,
          ])}
        />
      </CollapsibleSection>

      <CollapsibleSection
        title="Edge status breakdown"
        count={EDGES.length}
        defaultOpen={false}
      >
        <Grid columns={4} gap={12}>
          {(["working", "scraped", "partial", "planned"] as Status[]).map((s) => (
            <div key={s}>
              <Stat
                label={STATUS_LABELS[s]}
                value={String(edgeCounts[s])}
                tone={STATUS_STAT_TONE[s]}
              />
            </div>
          ))}
        </Grid>
      </CollapsibleSection>

      <H3>Pipeline</H3>
      <Row gap={8} wrap>
        <Pill tone="success">build-identity</Pill>
        <Text tone="tertiary">+</Text>
        <Pill tone="success">external-identity</Pill>
        <Text tone="tertiary">→</Text>
        <Pill tone="success">normalize-edges</Pill>
        <Text tone="tertiary">→</Text>
        <Pill tone="info">enrich-external-persons</Pill>
        <Text tone="tertiary">→</Text>
        <Pill tone="success">build-graph</Pill>
        <Text tone="tertiary">→</Text>
        <Pill tone="success">qa</Pill>
        <Text tone="tertiary">→</Text>
        <Pill tone="info">graph-viewer :8765</Pill>
      </Row>
      <Row gap={8}>
        <Button
          variant="secondary"
          onClick={() => dispatch({ type: "openFile", path: "DATA_GRAPH.md" })}
        >
          Open DATA_GRAPH.md
        </Button>
        <Button
          variant="ghost"
          onClick={() => setSelectedNode(null)}
        >
          Clear selection
        </Button>
      </Row>
    </Stack>
  );
}
