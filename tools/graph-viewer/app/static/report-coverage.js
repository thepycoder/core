let reportData = null;
let selectedBlockIndex = null;
let overlayFrame = null;

const OVERLAY_CSS = `
[data-pg-block] {
  cursor: pointer;
  transition: background-color 0.12s ease;
}
[data-pg-block].pg-uncovered {
  background-color: rgba(120, 120, 120, 0.1);
}
[data-pg-block].pg-covered {
  background-color: rgba(61, 140, 74, 0.16);
}
[data-pg-block].pg-scope {
  background-color: rgba(100, 149, 237, 0.16);
}
[data-pg-block].pg-invalid {
  background-color: rgba(196, 61, 61, 0.18);
}
[data-pg-block].pg-stale {
  background-color: rgba(214, 138, 31, 0.18);
}
[data-pg-block]:hover:not(.pg-selected) {
  background-color: rgba(88, 166, 255, 0.14);
}
[data-pg-block].pg-selected {
  background-color: rgba(37, 99, 235, 0.2);
  outline: 1px solid rgba(37, 99, 235, 0.5);
  outline-offset: -1px;
}
`;

async function api(path) {
  const res = await fetch(path);
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`${res.status}: ${text}`);
  }
  return res.json();
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text ?? "";
  return div.innerHTML;
}

function cacheUrl(cachePath) {
  if (!cachePath) return null;
  return `/api/cache/${cachePath.split("/").map(encodeURIComponent).join("/")}`;
}

function graphNodeUrl(type, id) {
  const params = new URLSearchParams({ type, id });
  return `/?${params}`;
}

function selectedValues(id) {
  const select = document.getElementById(id);
  return select ? [...select.selectedOptions].map((option) => option.value).filter(Boolean) : [];
}

function setSelectedValues(id, values) {
  const wanted = new Set(values || []);
  const select = document.getElementById(id);
  if (!select) return;
  for (const option of select.options) option.selected = wanted.has(option.value);
}

function contextFromControls() {
  return {
    sessionId: document.getElementById("report-session")?.value.trim() || "56",
    meetingKind: document.getElementById("report-kind")?.value || "plenary",
    meetingId: document.getElementById("report-meeting")?.value || "",
    blockIndex: selectedBlockIndex,
    entityTypes: selectedValues("report-entity-filter"),
    coverageKinds: selectedValues("report-coverage-filter"),
    spanRoles: (document.getElementById("report-role-filter")?.value || "")
      .split(",")
      .map((value) => value.trim())
      .filter(Boolean),
  };
}

function applyContextToControls(context) {
  if (!context) return;
  const session = document.getElementById("report-session");
  const kind = document.getElementById("report-kind");
  const meeting = document.getElementById("report-meeting");
  if (session) session.value = context.sessionId;
  if (kind) kind.value = context.meetingKind;
  if (meeting && context.meetingId) meeting.value = context.meetingId;
  setSelectedValues("report-entity-filter", context.entityTypes);
  setSelectedValues("report-coverage-filter", context.coverageKinds);
  const role = document.getElementById("report-role-filter");
  if (role) role.value = (context.spanRoles || []).join(", ");
}

function updateUrl(context = contextFromControls()) {
  const params = new URLSearchParams();
  params.set("session_id", context.sessionId);
  params.set("meeting_kind", context.meetingKind);
  if (context.meetingId) params.set("meeting_id", context.meetingId);
  if (context.blockIndex != null) params.set("block", String(context.blockIndex));
  for (const value of context.entityTypes || []) params.append("entity_type", value);
  for (const value of context.coverageKinds || []) params.append("coverage_kind", value);
  for (const value of context.spanRoles || []) params.append("span_role", value);
  const next = `${window.location.pathname}?${params}`;
  window.history.replaceState({ context }, "", next);
}

function cleanText(raw) {
  return (raw || "").replace(/\u00ad/g, "").replace(/\u00a0/g, " ").trim();
}

function blockText(el, tag) {
  if (tag === "table") {
    return [...el.querySelectorAll("td, th")]
      .map((cell) => cleanText(cell.textContent))
      .filter(Boolean)
      .join(" ");
  }
  return cleanText((el.textContent || "").replace(/\n/g, " "));
}

function collectBlockElements(doc) {
  const nodes = [];
  for (const el of doc.querySelectorAll("h1, h2, p, table")) {
    const tag = el.tagName.toLowerCase();
    if (tag === "p" && el.closest("table")) continue;
    if (!blockText(el, tag)) continue;
    nodes.push(el);
  }
  return nodes;
}

/**
 * Center a block inside the iframe's own scrolling root.
 * Avoid scrollIntoView: it no-ops while the iframe is display:none, and on
 * long reports "smooth" crawls for seconds while also scrolling the parent page.
 */
function scrollBlockIntoView(element) {
  if (!element) return;
  const doc = element.ownerDocument;
  const root = doc.scrollingElement || doc.documentElement;
  if (!root) return;

  const rect = element.getBoundingClientRect();
  const targetTop =
    root.scrollTop + rect.top - root.clientHeight / 2 + rect.height / 2;
  const maxScroll = Math.max(0, root.scrollHeight - root.clientHeight);
  root.scrollTop = Math.max(0, Math.min(maxScroll, targetTop));
}

function scheduleScrollToBlock(doc, index) {
  if (doc == null || index == null) return;
  const run = () => {
    const elements = collectBlockElements(doc);
    scrollBlockIntoView(elements[Number(index)]);
  };
  requestAnimationFrame(() => {
    requestAnimationFrame(run);
  });
}

function clearOverlayClasses(el) {
  el.classList.remove(
    "pg-block",
    "pg-covered",
    "pg-scope",
    "pg-uncovered",
    "pg-invalid",
    "pg-stale",
    "pg-selected"
  );
  el.removeAttribute("data-pg-block");
}

function applyOverlays(doc, blocks, focusIndex) {
  if (!doc?.body) return;
  if (!doc.getElementById("pg-overlay-styles")) {
    const style = doc.createElement("style");
    style.id = "pg-overlay-styles";
    style.textContent = OVERLAY_CSS;
    (doc.head || doc.documentElement).appendChild(style);
  }

  const blockMap = new Map((blocks || []).map((block) => [block.block_index, block]));
  const elements = collectBlockElements(doc);
  for (const el of doc.querySelectorAll("[data-pg-block]")) clearOverlayClasses(el);

  elements.forEach((el, index) => {
    const block = blockMap.get(index);
    el.setAttribute("data-pg-block", String(index));
    el.classList.add("pg-block");
    if (block?.has_extraction) el.classList.add("pg-covered");
    else if (block?.has_scope) el.classList.add("pg-scope");
    else el.classList.add("pg-uncovered");
    if (block?.has_invalid) el.classList.add("pg-invalid");
    if (block?.has_stale) el.classList.add("pg-stale");
    if (focusIndex != null && Number(focusIndex) === index) el.classList.add("pg-selected");
    el.onclick = (event) => {
      event.preventDefault();
      selectBlock(index, { scroll: false });
    };
  });
}

function appendMetadataRow(container, label, value) {
  if (value == null || value === "") return;
  const row = document.createElement("div");
  const strong = document.createElement("strong");
  strong.textContent = `${label}: `;
  row.append(strong, document.createTextNode(String(value)));
  container.appendChild(row);
}

function renderSpanDetail(span) {
  const card = document.createElement("div");
  card.className = `span-detail ${span.validation_status || "valid"}`;
  appendMetadataRow(card, "Entity", `${span.entity_type} ${span.entity_id}`);
  appendMetadataRow(card, "Role", span.span_role);
  appendMetadataRow(card, "Coverage", span.coverage_kind);
  appendMetadataRow(card, "Fields", span.field_names);
  appendMetadataRow(card, "Blocks", `${span.block_start}–${span.block_end}`);
  appendMetadataRow(card, "Confidence", span.confidence);
  appendMetadataRow(card, "Extractor", span.extractor);
  appendMetadataRow(card, "Status", span.validation_status);
  appendMetadataRow(card, "Reason", span.unresolved_reason);

  const actions = document.createElement("div");
  actions.className = "span-actions";
  const graph = document.createElement("a");
  graph.href = graphNodeUrl(span.entity_type, span.entity_id);
  graph.textContent = "Open in graph explorer";
  actions.appendChild(graph);
  if (span.source_url) {
    const source = document.createElement("a");
    source.href = span.source_url;
    source.target = "_blank";
    source.rel = "noopener";
    source.textContent = "Source";
    actions.appendChild(source);
  }
  card.appendChild(actions);
  return card;
}

function showBlockDetail(block) {
  const panel = document.getElementById("report-block-detail");
  if (!panel) return;
  if (!block) {
    panel.classList.add("hidden");
    panel.innerHTML = "";
    return;
  }
  panel.classList.remove("hidden");
  panel.innerHTML = "";
  const heading = document.createElement("strong");
  heading.textContent = `Block #${block.block_index} (${block.word_count || 0} words)`;
  panel.appendChild(heading);
  appendMetadataRow(panel, "Type", block.block_type);
  appendMetadataRow(panel, "Artifact", block.artifact_id);
  appendMetadataRow(panel, "Parser version", block.block_parser_version);
  if (!block.spans?.length) {
    const empty = document.createElement("p");
    empty.className = "muted";
    empty.textContent = "No spans overlap this block.";
    panel.appendChild(empty);
  }
  for (const span of block.spans || []) panel.appendChild(renderSpanDetail(span));
}

function selectBlock(index, options = {}) {
  const { scroll = true } = options;
  selectedBlockIndex = index;
  const block = reportData?.blocks?.find((entry) => Number(entry.block_index) === Number(index));
  showBlockDetail(block || null);
  updateUrl();

  const frame = document.getElementById("report-frame");
  const doc = frame?.contentDocument;
  if (!doc) return;
  for (const el of doc.querySelectorAll("[data-pg-block]")) {
    el.classList.toggle("pg-selected", el.getAttribute("data-pg-block") === String(index));
  }
  if (scroll) scheduleScrollToBlock(doc, index);
}

async function loadReportMeetings() {
  const select = document.getElementById("report-meeting");
  if (!select) return;
  const sessionId = document.getElementById("report-session")?.value.trim() || "56";
  const meetingKind = document.getElementById("report-kind")?.value || "plenary";
  const query = new URLSearchParams({ session_id: sessionId, meeting_kind: meetingKind });
  const previous = select.value;
  const data = await api(`/api/reports/meetings?${query}`);
  select.innerHTML = "";
  for (const meeting of data.meetings || []) {
    const opt = document.createElement("option");
    opt.value = meeting.meeting_id;
    opt.textContent = `${meeting.meeting_kind} ${meeting.meeting_id}`;
    select.appendChild(opt);
  }
  if (previous && [...select.options].some((option) => option.value === String(previous))) {
    select.value = String(previous);
  }
}

function renderProvenance(data) {
  const el = document.getElementById("report-provenance");
  if (!el) return;
  el.innerHTML = "";
  if (data.source_url) {
    const source = document.createElement("div");
    source.innerHTML = `<a href="${escapeHtml(data.source_url)}" target="_blank" rel="noopener">Open on dekamer.be</a>`;
    el.appendChild(source);
  }
  if (data.cache_path) {
    const cache = document.createElement("div");
    const href = cacheUrl(data.cache_path);
    cache.innerHTML = `<a href="${escapeHtml(href)}" target="_blank" rel="noopener">Open raw cached HTML</a>`;
    el.appendChild(cache);
  }
  if (data.block_parser_version) {
    const versions = document.createElement("div");
    versions.textContent = `Parser ${data.block_parser_version} · extractor ${data.extractor_version || "—"}`;
    el.appendChild(versions);
  }
}

async function loadReportHtml(cachePath) {
  const href = cacheUrl(cachePath);
  const res = await fetch(href);
  if (!res.ok) throw new Error(`Could not load cached HTML (${res.status})`);
  return res.text();
}

function showViewerStatus(message) {
  const status = document.getElementById("viewer-status");
  const frame = document.getElementById("report-frame");
  if (status) {
    status.textContent = message;
    status.classList.remove("hidden");
  }
  if (frame) frame.classList.add("hidden");
}

function mountReportHtml(html, blocks, focusIndex) {
  const status = document.getElementById("viewer-status");
  const frame = document.getElementById("report-frame");
  if (!frame) return;

  if (overlayFrame) {
    overlayFrame.onload = null;
  }
  overlayFrame = frame;

  return new Promise((resolve, reject) => {
    frame.onload = () => {
      try {
        // Unhide before measuring/scrolling — display:none makes scroll a no-op.
        status?.classList.add("hidden");
        frame.classList.remove("hidden");
        const doc = frame.contentDocument;
        applyOverlays(doc, blocks, focusIndex);
        if (focusIndex != null) {
          scheduleScrollToBlock(doc, focusIndex);
          setTimeout(() => scheduleScrollToBlock(doc, focusIndex), 220);
        }
        resolve();
      } catch (err) {
        reject(err);
      }
    };
    frame.onerror = () => reject(new Error("Failed to render report HTML"));
    frame.srcdoc = html;
  });
}

async function loadReportCoverage(focusBlock = selectedBlockIndex) {
  const select = document.getElementById("report-meeting");
  const stats = document.getElementById("report-coverage-stats");
  const diagnostics = document.getElementById("report-diagnostics");
  const title = document.getElementById("report-title");
  if (!select || !stats || !diagnostics) return;

  const context = contextFromControls();
  const meetingId = select.value || context.meetingId;
  if (!meetingId) {
    stats.textContent = "No meeting available for this session and kind.";
    diagnostics.innerHTML = "";
    showBlockDetail(null);
    showViewerStatus("Select a meeting to load the cached report HTML.");
    if (title) title.textContent = "";
    return;
  }

  const params = new URLSearchParams();
  for (const value of context.entityTypes) params.append("entity_type", value);
  for (const value of context.coverageKinds) params.append("coverage_kind", value);
  for (const value of context.spanRoles) params.append("span_role", value);
  const query = params.toString();

  stats.textContent = "Loading…";
  diagnostics.innerHTML = "";
  showBlockDetail(null);
  showViewerStatus("Loading cached report HTML…");

  let data;
  try {
    data = await api(
      `/api/reports/${encodeURIComponent(context.sessionId)}/${encodeURIComponent(context.meetingKind)}/${encodeURIComponent(meetingId)}${query ? `?${query}` : ""}`
    );
  } catch (err) {
    stats.textContent = `Could not load report: ${err.message}`;
    showViewerStatus("Report data could not be loaded.");
    return;
  }

  reportData = data;
  selectedBlockIndex = focusBlock ?? null;
  if (title) {
    title.textContent = `${data.meeting_kind} ${data.meeting_id} · session ${data.session_id}`;
  }

  const ratio = Math.round((data.coverage?.ratio || 0) * 100);
  stats.textContent = `${data.coverage?.covered_words || 0}/${data.coverage?.total_words || 0} words (${ratio}%) · ${data.coverage?.valid_span_count || 0} valid / ${data.coverage?.invalid_span_count || 0} invalid spans`;

  for (const item of data.diagnostics || []) {
    const note = document.createElement("div");
    note.className = `report-diagnostic ${item.state}`;
    note.textContent = `${item.code}: ${item.message}${item.count > 1 ? ` (${item.count})` : ""}`;
    diagnostics.appendChild(note);
  }

  renderProvenance(data);
  updateUrl({ ...context, meetingId, blockIndex: selectedBlockIndex });

  if (!data.cache_path) {
    showViewerStatus(
      data.derived_data_status === "missing"
        ? "Derived report blocks are missing for this report."
        : "No cached HTML path is available for this meeting."
    );
    return;
  }

  try {
    const html = await loadReportHtml(data.cache_path);
    await mountReportHtml(html, data.blocks || [], selectedBlockIndex);
    if (selectedBlockIndex != null) {
      const block = data.blocks?.find(
        (entry) => Number(entry.block_index) === Number(selectedBlockIndex)
      );
      showBlockDetail(block || null);
    }
  } catch (err) {
    showViewerStatus(`Could not render cached HTML: ${err.message}`);
  }
}

function bindControls() {
  const reload = () => loadReportCoverage().catch((err) => {
    document.getElementById("report-coverage-stats").textContent = `Error: ${err.message}`;
  });

  document.getElementById("report-meeting")?.addEventListener("change", reload);
  document.getElementById("report-entity-filter")?.addEventListener("change", reload);
  document.getElementById("report-coverage-filter")?.addEventListener("change", reload);
  document.getElementById("report-role-filter")?.addEventListener("change", reload);

  const reloadMeetings = async () => {
    selectedBlockIndex = null;
    await loadReportMeetings();
    await loadReportCoverage();
  };
  document.getElementById("report-session")?.addEventListener("change", () => {
    reloadMeetings().catch(console.error);
  });
  document.getElementById("report-kind")?.addEventListener("change", () => {
    reloadMeetings().catch(console.error);
  });

  document.getElementById("report-clear-filters")?.addEventListener("click", () => {
    setSelectedValues("report-entity-filter", []);
    setSelectedValues("report-coverage-filter", []);
    document.getElementById("report-role-filter").value = "";
    reload();
  });

  document.getElementById("panel-toggle")?.addEventListener("click", () => {
    const layout = document.getElementById("layout");
    const button = document.getElementById("panel-toggle");
    const hidden = layout?.classList.toggle("panel-hidden");
    if (button) {
      button.textContent = hidden ? "Show panel" : "Hide panel";
      button.setAttribute("aria-expanded", hidden ? "false" : "true");
    }
  });
}

function contextFromUrl() {
  const params = new URLSearchParams(window.location.search);
  const block = params.get("block");
  return {
    sessionId: params.get("session_id") || "56",
    meetingKind: params.get("meeting_kind") || "plenary",
    meetingId: params.get("meeting_id") || "",
    blockIndex: block != null && block !== "" ? Number(block) : null,
    entityTypes: params.getAll("entity_type"),
    coverageKinds: params.getAll("coverage_kind"),
    spanRoles: params.getAll("span_role").length
      ? params.getAll("span_role")
      : (params.get("span_role") || "")
          .split(",")
          .map((value) => value.trim())
          .filter(Boolean),
  };
}

async function init() {
  bindControls();
  const context = contextFromUrl();
  selectedBlockIndex = context.blockIndex;
  applyContextToControls(context);
  await loadReportMeetings();
  applyContextToControls(context);
  await loadReportCoverage(context.blockIndex);
}

init();
