let inspectorNode = null;
const navStack = [];
let reportContext = null;
const linkSearchTimers = new Map();
const PAGE_SIZE = 40;

function navStackSnapshot() {
  return navStack.map((item) => ({ type: item.type, id: item.id, label: item.label }));
}

function reportContextSnapshot() {
  if (!reportContext) return null;
  return {
    ...reportContext,
    entityTypes: [...(reportContext.entityTypes || [])],
    coverageKinds: [...(reportContext.coverageKinds || [])],
    spanRoles: [...(reportContext.spanRoles || [])],
  };
}

function addReportParams(params, context = reportContext) {
  if (!context) return params;
  params.set("report", context.meetingId);
  params.set("session_id", context.sessionId);
  params.set("meeting_kind", context.meetingKind);
  if (context.blockIndex != null) params.set("block", String(context.blockIndex));
  for (const value of context.entityTypes || []) params.append("entity_type", value);
  for (const value of context.coverageKinds || []) params.append("coverage_kind", value);
  for (const value of context.spanRoles || []) params.append("span_role", value);
  return params;
}

function viewUrlForItem(item) {
  if (!item) return window.location.pathname;
  if (item.type === "Report") {
    return `?${addReportParams(new URLSearchParams()).toString()}`;
  }
  if (item.type === "Unresolved") {
    const params = new URLSearchParams({ unresolved: item.id });
    addReportParams(params);
    return `?${params}`;
  }
  const params = new URLSearchParams({ type: item.type, id: item.id });
  addReportParams(params);
  return `?${params}`;
}

function currentViewUrl() {
  return viewUrlForItem(navStack[navStack.length - 1]);
}

function writeHistory() {
  history.pushState(
    { navStack: navStackSnapshot(), reportContext: reportContextSnapshot() },
    "",
    currentViewUrl()
  );
}

function seedHomeHistoryEntry() {
  history.replaceState({ navStack: [] }, "", window.location.pathname);
}

function resetInspector() {
  inspectorNode = null;
  renderProvenanceBar("", "");
  const el = document.getElementById("inspector-content");
  el.innerHTML =
    "Search for an entity or click an issue sample to inspect links and open source documents.";
  el.classList.add("muted");
}

async function api(path, options = {}) {
  const res = await fetch(path, options);
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`${res.status}: ${text}`);
  }
  return res.json();
}

function truncate(text, max) {
  if (!text) return "";
  return text.length > max ? `${text.slice(0, max)}…` : text;
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text ?? "";
  return div.innerHTML;
}

const EDGE_ROLE_LABELS = {
  chair: "Chair",
  subchair: "Subchair",
  permanent: "Permanent member",
  replacement: "Replacement member",
  member: "Member",
};

function formatEdgeRole(role) {
  if (!role) return "";
  return EDGE_ROLE_LABELS[role] || role.replace(/_/g, " ");
}

function displayEdgeRole(role) {
  const label = formatEdgeRole(role);
  if (!label || label === "Member") return "";
  return label;
}

function edgeRoleMarkup(role) {
  const label = displayEdgeRole(role);
  if (!label) return "";
  return ` · <span class="link-drill-role">${escapeHtml(label)}</span>`;
}

function cacheUrl(cachePath) {
  if (!cachePath) return null;
  return `/api/cache/${cachePath.split("/").map(encodeURIComponent).join("/")}`;
}

function pushNav(type, id, label) {
  const top = navStack[navStack.length - 1];
  if (top && top.type === type && top.id === id) return;
  navStack.push({ type, id, label: label || id });
  renderBreadcrumbs();
}

function navigateToIndex(index) {
  if (index < 0 || index >= navStack.length) return;
  navStack.length = index + 1;
  renderBreadcrumbs();
  const item = navStack[index];
  history.pushState(
    { navStack: navStackSnapshot(), reportContext: reportContextSnapshot() },
    "",
    viewUrlForItem(item)
  );
  if (item.type === "Report") {
    openReportContext(reportContext, { fromHistory: true });
  } else {
    openNode(item.type, item.id, { resetNav: false, fromHistory: true });
  }
}

function renderBreadcrumbs() {
  const bar = document.getElementById("breadcrumb-bar");
  if (!bar) return;
  bar.innerHTML = "";
  if (!navStack.length) {
    bar.classList.add("hidden");
    return;
  }
  bar.classList.remove("hidden");

  navStack.forEach((item, i) => {
    if (i > 0) {
      const sep = document.createElement("span");
      sep.className = "crumb-sep";
      sep.textContent = "›";
      bar.appendChild(sep);
    }
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = i === navStack.length - 1 ? "crumb crumb-current" : "crumb";
    btn.textContent = `${item.type}: ${truncate(item.label, 40)}`;
    if (i < navStack.length - 1) {
      btn.addEventListener("click", () => navigateToIndex(i));
    }
    bar.appendChild(btn);
  });
}

function renderProvenanceBar(sourceUrl, cachePath) {
  const bar = document.getElementById("provenance-bar");
  bar.innerHTML = "";
  if (!sourceUrl && !cachePath) {
    bar.classList.add("hidden");
    return;
  }
  bar.classList.remove("hidden");

  if (sourceUrl) {
    const a = document.createElement("a");
    a.href = sourceUrl;
    a.target = "_blank";
    a.rel = "noopener";
    a.className = "provenance-btn";
    a.textContent = "Open on dekamer.be";
    bar.appendChild(a);
  }
  if (cachePath) {
    const a = document.createElement("a");
    a.href = cacheUrl(cachePath);
    a.target = "_blank";
    a.rel = "noopener";
    a.className = "provenance-btn";
    a.textContent = "Open cached HTML";
    bar.appendChild(a);
  }
}

function seedFromSample(_issueId, sample) {
  if (sample.node_type && sample.node_id) {
    return { type: sample.node_type, id: sample.node_id };
  }
  return null;
}

function formatIssueSampleHint(sample) {
  if (sample.action === "node") return "Open in inspector";
  if (sample.action === "unresolved_bucket") return "Browse unresolved names";
  if (sample.action === "artifact") return "View artifact";
  return "View issue details";
}

async function openIssueSample(issue, sample) {
  if (sample.action === "node" && sample.node_type && sample.node_id) {
    try {
      await openNode(sample.node_type, sample.node_id, { resetNav: true });
      return;
    } catch (err) {
      showIssueContext(issue, sample, err.message);
      return;
    }
  }
  if (sample.action === "unresolved_bucket") {
    await showUnresolvedBucket(sample.unresolved_bucket, sample.unresolved_reason, sample.label);
    return;
  }
  if (sample.action === "artifact" && sample.artifact_id) {
    await showArtifactIssue(sample);
    return;
  }
  if (sample.action === "report" && sample.meeting_id && sample.source_block) {
    await openReportAtBlock(
      sample.meeting_id,
      sample.source_block,
      sample.session_id || "56",
      sample.meeting_kind || "plenary"
    );
    return;
  }
  showIssueContext(issue, sample);
}

function showIssueContext(issue, sample, errorMessage = "") {
  if (!sample.label && !sample.data) return;
  navStack.length = 0;
  pushNav("QA issue", issue.id, issue.id);
  renderBreadcrumbs();
  renderProvenanceBar(sample.source_url || sample.data?.source_url || "", sample.cache_path || sample.data?.cache_path || "");

  const el = document.getElementById("inspector-content");
  el.innerHTML = "";
  el.classList.remove("muted");
  inspectorNode = null;

  const header = document.createElement("div");
  header.className = "entity-header";
  header.innerHTML = `
    <span class="type-badge">QA issue</span>
    <h2>${escapeHtml(issue.id)}</h2>
    <div class="entity-meta"><span>${issue.count} occurrences</span></div>
  `;
  el.appendChild(header);

  const summary = document.createElement("p");
  summary.className = "inspector-note";
  summary.textContent = issue.summary;
  el.appendChild(summary);

  if (errorMessage) {
    const err = document.createElement("p");
    err.className = "inspector-note";
    err.textContent = `Could not open linked node: ${errorMessage}`;
    el.appendChild(err);
  }

  const section = document.createElement("div");
  section.className = "detail-section";
  section.innerHTML = `<h3>Sample detail</h3>`;

  const dl = document.createElement("dl");
  dl.className = "preview-fields";
  const fields = [
    ["Message", sample.label || sample.data?.message],
    ["Entity", sample.data?.entity_type && sample.data?.entity_id ? `${sample.data.entity_type} ${sample.data.entity_id}` : ""],
    ["Expected", sample.data?.expected],
    ["Actual", sample.data?.actual],
    ["Meeting", sample.data?.meeting_kind && sample.data?.meeting_id ? `${sample.data.meeting_kind} ${sample.data.meeting_id}` : ""],
  ];
  for (const [label, value] of fields) {
    if (!value) continue;
    const dt = document.createElement("dt");
    dt.textContent = label;
    const dd = document.createElement("dd");
    dd.textContent = value;
    dl.appendChild(dt);
    dl.appendChild(dd);
  }
  section.appendChild(dl);
  el.appendChild(section);
  writeHistory();
}

async function showUnresolvedBucket(bucket, reason, title) {
  navStack.length = 0;
  pushNav("Unresolved bucket", `${bucket}:${reason}`, title || `${bucket} · ${reason}`);
  renderBreadcrumbs();
  renderProvenanceBar("", "");

  const params = new URLSearchParams({ limit: "100", offset: "0" });
  if (bucket) params.set("bucket", bucket);
  if (reason) params.set("reason", reason);
  const data = await api(`/api/unresolved?${params}`);

  const el = document.getElementById("inspector-content");
  el.innerHTML = "";
  el.classList.remove("muted");
  inspectorNode = null;

  const header = document.createElement("div");
  header.className = "entity-header";
  header.innerHTML = `
    <span class="type-badge">Unresolved bucket</span>
    <h2>${escapeHtml(title || `${bucket} · ${reason}`)}</h2>
    <div class="entity-meta"><span>${data.total} unresolved names</span></div>
  `;
  el.appendChild(header);

  const section = document.createElement("div");
  section.className = "detail-section";
  section.innerHTML = `<h3>Names</h3>`;
  const list = document.createElement("div");
  list.className = "link-list";

  if (!data.rows.length) {
    list.innerHTML = `<div class="muted">No rows for this bucket filter.</div>`;
  } else {
    for (const row of data.rows) {
      const item = document.createElement("button");
      item.type = "button";
      item.className = "link-drill";
      item.innerHTML = `
        <span class="link-drill-type">${escapeHtml(row.source_bucket)} · ${escapeHtml(row.role || "—")}</span>
        <span class="link-drill-label">${escapeHtml(row.raw_name)}</span>
        <span class="link-drill-id muted">${escapeHtml(row.context_label || row.context_id || "")}</span>
      `;
      item.addEventListener("click", () => showUnresolved(row.raw_name));
      list.appendChild(item);
    }
  }

  section.appendChild(list);
  el.appendChild(section);
  writeHistory();
}

async function showArtifactIssue(sample) {
  const detail = await api(`/api/artifact/${encodeURIComponent(sample.artifact_id)}`);
  navStack.length = 0;
  pushNav("Artifact", sample.artifact_id, sample.artifact_id);
  renderBreadcrumbs();
  renderProvenanceBar(detail.source_url, detail.cache_path);

  const el = document.getElementById("inspector-content");
  el.innerHTML = "";
  el.classList.remove("muted");
  inspectorNode = null;

  const header = document.createElement("div");
  header.className = "entity-header";
  header.innerHTML = `
    <span class="type-badge">Artifact</span>
    <h2>${escapeHtml(sample.artifact_id)}</h2>
    <div class="entity-meta"><span>${escapeHtml(sample.label || "Missing scraped_at")}</span></div>
  `;
  el.appendChild(header);

  const section = document.createElement("div");
  section.className = "detail-section preview-section";
  section.innerHTML = `
    <dl class="preview-fields">
      <dt>Block parser</dt><dd>${escapeHtml(detail.block_parser_version || "—")}</dd>
      <dt>Extractor</dt><dd>${escapeHtml(detail.extractor_version || "—")}</dd>
      <dt>Scraped at</dt><dd>${escapeHtml(detail.scraped_at || "—")}</dd>
    </dl>
  `;
  el.appendChild(section);
  writeHistory();
}

async function loadHealth() {
  const health = await api("/api/health");
  const missing = health.files.filter((f) => !f.exists).length;
  const el = document.getElementById("health-status");
  const version = health.viewer_version ? ` · viewer ${health.viewer_version}` : "";
  el.textContent = `${health.data_dir} · ${missing ? `${missing} missing files` : "data ok"}${version}`;
  if (health.warnings.length) el.title = health.warnings.join("\n");
}

async function loadIssues() {
  const { issues } = await api("/api/issues");
  const list = document.getElementById("issues-list");
  list.innerHTML = "";

  for (const issue of issues) {
    const li = document.createElement("li");
    li.className = "issue-item";
    li.innerHTML = `
      <div class="issue-count severity-${issue.severity}">${issue.count} · ${issue.id}</div>
      <div class="muted">${issue.summary}</div>
    `;
    for (const sample of issue.samples.slice(0, 3)) {
      const s = document.createElement("div");
      s.className = `sample${sample.action === "node" ? " sample-actionable" : ""}`;
      s.title = formatIssueSampleHint(sample);
      s.textContent = sample.label || JSON.stringify(sample.data);
      s.addEventListener("click", () => openIssueSample(issue, sample));
      li.appendChild(s);
    }
    list.appendChild(li);
  }
}

async function runSearch() {
  const q = document.getElementById("search-input").value.trim();
  const type = document.getElementById("search-type").value;
  const params = new URLSearchParams({ q, limit: "25" });
  if (type) params.set("type", type);

  const list = document.getElementById("search-results");
  list.innerHTML = "";
  if (!q) return;

  try {
    const { results } = await api(`/api/search?${params}`);
    if (!results.length) {
      list.innerHTML = `<li class="muted">No matches for "${escapeHtml(q)}"</li>`;
      return;
    }
    for (const row of results) {
      const li = document.createElement("li");
      li.className = row.source === "node" ? "search-hit" : "search-hit search-hit-unresolved";
      const badge =
        row.source === "node"
          ? row.type
          : row.source === "content"
            ? `${row.type} · text`
            : "Unresolved name";
      li.innerHTML = `<strong>${badge}</strong> ${escapeHtml(truncate(row.label, 50))}<br>
        <span class="muted">${escapeHtml(row.subtitle || row.id)}</span>`;
      li.addEventListener("click", () => openSearchResult(row));
      list.appendChild(li);
    }
  } catch (err) {
    list.innerHTML = `<li class="muted">Search error: ${escapeHtml(err.message)}</li>`;
  }
}

async function openSearchResult(row) {
  if (row.source === "node" || row.source === "content") {
    await openNode(row.type, row.id, { resetNav: true });
    return;
  }
  await showUnresolved(row.label);
}

async function openNode(type, id, options = {}) {
  const { resetNav = true, fromHistory = false } = options;
  if (type === "Unresolved") {
    await showUnresolved(id, { fromHistory });
    return;
  }
  if (resetNav) {
    navStack.length = 0;
  }
  document.getElementById("report-section")?.classList.remove("fullscreen");
  document.getElementById("report-close")?.classList.add("hidden");
  const detail = await api(`/api/node/${encodeURIComponent(type)}/${id}`);
  pushNav(type, id, detail.label);
  inspectorNode = { type, id };
  renderNodeDetail(detail);
  if (!fromHistory) {
    writeHistory();
  }
}

async function drillTo(neighborType, neighborId) {
  await openNode(neighborType, neighborId, { resetNav: false });
}

function renderNodeDetail(detail) {
  renderProvenanceBar(detail.source_url, detail.cache_path);

  const el = document.getElementById("inspector-content");
  el.innerHTML = "";
  el.classList.remove("muted");

  const header = document.createElement("div");
  header.className = "entity-header";
  header.innerHTML = `
    <span class="type-badge">${escapeHtml(detail.type)}</span>
    <h2>${escapeHtml(detail.label)}</h2>
    <div class="entity-meta">
      <span>id: ${escapeHtml(detail.id)}</span>
      <span>${detail.out_edges.reduce((s, g) => s + g.count, 0)} outgoing</span>
      <span>${detail.in_edges.reduce((s, g) => s + g.count, 0)} incoming</span>
    </div>
  `;
  el.appendChild(header);

  if (detail.preview) {
    el.appendChild(renderEntityPreview(detail.preview));
  }

  if (detail.vote_breakdown) {
    el.appendChild(renderVoteBreakdown(detail.vote_breakdown));
  }

  if (detail.vote_reconciliation) {
    el.appendChild(renderVoteReconciliation(detail.vote_reconciliation));
  }

  if (detail.utterance_groups?.length) {
    el.appendChild(renderUtteranceGroups(detail.utterance_groups, detail.utterance_section_title));
  } else if (detail.utterances?.length) {
    el.appendChild(renderUtterances(detail.utterances, detail.utterance_section_title));
  }

  if (detail.source_evidence?.length) {
    el.appendChild(renderSourceEvidence(detail.source_evidence));
  }

  appendLinkSection(el, "Outgoing links", "out", detail.out_edges);
  appendLinkSection(el, "Incoming links", "in", detail.in_edges);
}

function renderSourceEvidence(rows) {
  const section = document.createElement("div");
  section.className = "detail-section source-evidence";
  const heading = document.createElement("h3");
  heading.textContent = "Report source evidence";
  section.appendChild(heading);
  for (const span of rows) {
    const card = renderSpanDetail(span);
    const open = document.createElement("button");
    open.type = "button";
    open.className = "link-action-btn";
    open.textContent = `Open report at block ${span.block_start}`;
    open.addEventListener("click", () =>
      openReportAtBlock(
        span.meeting_id,
        span.block_start,
        span.session_id,
        span.meeting_kind
      )
    );
    card.appendChild(open);
    section.appendChild(card);
  }
  return section;
}

function renderEntityPreview(preview) {
  const section = document.createElement("div");
  section.className = "detail-section preview-section";

  if (preview.title) {
    const title = document.createElement("div");
    title.className = "preview-title";
    title.textContent = preview.title;
    section.appendChild(title);
  }

  if (preview.fields?.length) {
    const dl = document.createElement("dl");
    dl.className = "preview-fields";
    for (const field of preview.fields) {
      const dt = document.createElement("dt");
      dt.textContent = field.label;
      const dd = document.createElement("dd");
      if (field.link) {
        const a = document.createElement("a");
        a.href = field.link;
        a.target = "_blank";
        a.rel = "noopener";
        a.textContent = field.value;
        dd.appendChild(a);
      } else {
        dd.textContent = field.value;
      }
      dl.append(dt, dd);
    }
    section.appendChild(dl);
  }

  if (preview.content) {
    const block = document.createElement("div");
    block.className = "content-block";
    if (preview.content_label) {
      const label = document.createElement("div");
      label.className = "content-label";
      label.textContent = preview.content_label;
      block.appendChild(label);
    }
    const text = document.createElement("div");
    text.className = "content-text";
    const full = preview.content;
    const limit = 1200;
    if (full.length > limit) {
      text.textContent = full.slice(0, limit) + "…";
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "link-action-btn show-more-btn";
      btn.textContent = "Show full text";
      btn.addEventListener("click", () => {
        text.textContent = full;
        btn.remove();
      });
      block.append(text, btn);
    } else {
      text.textContent = full;
      block.appendChild(text);
    }
    section.appendChild(block);
  }

  if (preview.related?.length) {
    const related = document.createElement("div");
    related.className = "preview-related";
    related.innerHTML = `<div class="content-label">Related</div>`;
    for (const item of preview.related) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "link-action-btn";
      btn.textContent = `${item.type}: ${truncate(item.label, 60)}`;
      btn.addEventListener("click", () => drillTo(item.type, item.id));
      related.appendChild(btn);
    }
    section.appendChild(related);
  }

  return section;
}

function renderVoteBreakdown(breakdown) {
  const section = document.createElement("div");
  section.className = "detail-section vote-breakdown-section";
  section.innerHTML = `<h3>Vote breakdown</h3>`;

  const positionLabels = { yes: "Yes", no: "No", abstain: "Abstain" };

  for (const group of breakdown.groups || []) {
    const details = document.createElement("details");
    details.className = "vote-position-group";
    if (group.members?.length) {
      details.open = true;
    }
    const count = group.members?.length ?? 0;
    const summary = document.createElement("summary");
    summary.textContent = `${positionLabels[group.position] || group.position} (${count || group.headline_count || 0})`;
    details.appendChild(summary);

    const list = document.createElement("div");
    list.className = "vote-cast-list";
    for (const member of group.members || []) {
      if (member.person_id && !member.unresolved) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "link-action-btn vote-cast-member";
        btn.textContent = member.label;
        btn.title = member.raw_name && member.raw_name !== member.label ? member.raw_name : "";
        btn.addEventListener("click", () => drillTo("Person", member.person_id));
        list.appendChild(btn);
      } else {
        const span = document.createElement("span");
        span.className = "vote-cast-member unresolved";
        span.textContent = member.label;
        span.title = "Unresolved person";
        list.appendChild(span);
      }
    }
    if (!group.members?.length) {
      const empty = document.createElement("div");
      empty.className = "muted vote-cast-empty";
      empty.textContent = "No member names recorded";
      list.appendChild(empty);
    }
    details.appendChild(list);
    section.appendChild(details);
  }

  return section;
}

function renderVoteReconciliation(v) {
  const section = document.createElement("div");
  section.className = "detail-section";
  const ok = v.reconciled === "true";
  section.innerHTML = `
    <h3>Vote totals ${ok ? "" : "⚠ mismatch"}</h3>
    <div class="stats-grid">
      <div><span>Yes (headline)</span>${escapeHtml(v.yes)} / parsed ${escapeHtml(v.members_yes_count)}</div>
      <div><span>No</span>${escapeHtml(v.no)} / ${escapeHtml(v.members_no_count)}</div>
      <div><span>Abstain</span>${escapeHtml(v.abstain)} / ${escapeHtml(v.members_abstain_count)}</div>
    </div>
  `;
  if (v.source_url || v.cache_path) {
    const links = document.createElement("div");
    links.style.marginTop = "0.5rem";
    if (v.source_url) {
      links.innerHTML += `<a href="${v.source_url}" target="_blank" rel="noopener">Vote source page</a> · `;
    }
    if (v.cache_path) {
      links.innerHTML += `<a href="${cacheUrl(v.cache_path)}" target="_blank" rel="noopener">Vote cache HTML</a>`;
    }
    section.appendChild(links);
  }
  return section;
}

function proceedingNodeType(itemKind) {
  const map = {
    question: "Question",
    hearing: "Hearing",
    interpellation: "Interpellation",
  };
  return map[itemKind] || null;
}

function appendUtteranceActionButton(actions, label, onClick) {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "link-action-btn";
  btn.textContent = label;
  btn.addEventListener("click", onClick);
  actions.appendChild(btn);
}

function appendUtteranceActions(block, u) {
  const actions = document.createElement("div");
  actions.className = "utterance-actions";

  if (u.utterance_id) {
    appendUtteranceActionButton(actions, "Open utterance", () =>
      drillTo("Utterance", u.utterance_id)
    );
  }

  const proceedingType = proceedingNodeType(u.item_kind);
  if (
    proceedingType &&
    u.item_id &&
    !(inspectorNode?.type === proceedingType && inspectorNode?.id === u.item_id)
  ) {
    appendUtteranceActionButton(actions, `Open ${proceedingType}`, () =>
      drillTo(proceedingType, u.item_id)
    );
  } else if (
    u.meeting_node_id &&
    !(inspectorNode?.type === "Meeting" && inspectorNode?.id === u.meeting_node_id)
  ) {
    appendUtteranceActionButton(actions, "Open meeting", () =>
      drillTo("Meeting", u.meeting_node_id)
    );
  }

  if (u.speaker_person_id) {
    appendUtteranceActionButton(actions, "Open Person", () =>
      drillTo("Person", u.speaker_person_id)
    );
  } else if (u.speaker_entity_id && u.speaker_entity_type) {
    appendUtteranceActionButton(actions, `Open ${u.speaker_entity_type}`, () =>
      drillTo(u.speaker_entity_type, u.speaker_entity_id)
    );
  } else if (u.raw_speaker) {
    appendUtteranceActionButton(actions, "Trace speaker", () => showUnresolved(u.raw_speaker));
  }

  if (actions.childElementCount) {
    block.appendChild(actions);
  }
}

function renderUtteranceBlock(u) {
  const block = document.createElement("div");
  block.className = "utterance-block";
  const turnPrefix = u.turn_number ? `${u.turn_number} · ` : "";
  const roleSuffix = u.speaker_role ? ` (${u.speaker_role})` : "";
  const speaker = `${u.raw_speaker || "?"}${roleSuffix}`;
  block.innerHTML = `
    <div class="speaker">${escapeHtml(turnPrefix + speaker)}</div>
    <div class="text">${escapeHtml(truncate(u.text, 1500))}</div>
  `;
  if (u.text && u.text.length > 1500) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "link-action-btn show-more-btn";
    btn.textContent = "Show full utterance";
    const textEl = block.querySelector(".text");
    btn.addEventListener("click", () => {
      textEl.textContent = u.text;
      btn.remove();
    });
    block.appendChild(btn);
  }
  appendUtteranceActions(block, u);
  return block;
}

function renderUtterances(utterances, sectionTitle) {
  const section = document.createElement("div");
  section.className = "detail-section";
  const title = sectionTitle || "Discussion";
  section.innerHTML = `<h3>${escapeHtml(title)} (${utterances.length} utterances)</h3>`;
  for (const u of utterances) {
    section.appendChild(renderUtteranceBlock(u));
  }
  return section;
}

function renderUtteranceGroups(groups, sectionTitle) {
  const section = document.createElement("div");
  section.className = "detail-section";
  const total = groups.reduce((sum, group) => sum + (group.utterances?.length || 0), 0);
  const title = sectionTitle || "Discussion";
  section.innerHTML = `<h3>${escapeHtml(title)} (${total} utterances, ${groups.length} agenda items)</h3>`;

  for (const group of groups) {
    const utterances = group.utterances || [];
    if (!utterances.length) continue;

    const agendaSection = document.createElement("details");
    agendaSection.className = "agenda-group";
    agendaSection.open = groups.length <= 6;

    const agendaLabel = group.agenda_id
      ? `${group.agenda_id} · ${group.title}`
      : group.title;
    const kindSuffix = group.item_kind ? ` · ${group.item_kind.replaceAll("_", " ")}` : "";
    const summary = document.createElement("summary");
    summary.className = "agenda-group-header";
    summary.innerHTML = `
      <span class="agenda-group-title">${escapeHtml(agendaLabel)}</span>
      <span class="agenda-group-meta">${escapeHtml(`${utterances.length} utterances${kindSuffix}`)}</span>
    `;
    agendaSection.appendChild(summary);

    const body = document.createElement("div");
    body.className = "agenda-group-body";
    for (const u of utterances) {
      body.appendChild(renderUtteranceBlock(u));
    }
    agendaSection.appendChild(body);
    section.appendChild(agendaSection);
  }

  return section;
}

function appendLinkSection(container, title, direction, groups) {
  const section = document.createElement("div");
  section.className = "detail-section";
  const h3 = document.createElement("h3");
  h3.textContent = title;
  section.appendChild(h3);

  if (!groups.length) {
    section.innerHTML += `<div class="muted">None</div>`;
    container.appendChild(section);
    return;
  }

  for (const group of groups) {
    section.appendChild(createLinkGroup(direction, group));
  }
  container.appendChild(section);
}

function createLinkGroup(direction, group) {
  const block = document.createElement("div");
  block.className = "link-group";

  const listId = `links-${direction}-${group.edge_type}`.replace(/[^a-zA-Z0-9_-]/g, "_");

  block.innerHTML = `
    <div class="link-group-header">
      <strong>${escapeHtml(group.edge_type)}</strong>
      <span class="muted">(${group.count})</span>
    </div>
  `;

  const search = document.createElement("input");
  search.type = "search";
  search.className = "link-search";
  const isSpokenFilter =
    inspectorNode &&
    (inspectorNode.type === "Person" || inspectorNode.type === "ExternalPerson") &&
    group.edge_type === "SPOKE";
  search.placeholder = isSpokenFilter
    ? "Filter by id, speaker, or utterance text…"
    : "Filter by title, id, edge type…";
  search.addEventListener("input", () => {
    clearTimeout(linkSearchTimers.get(listId));
    linkSearchTimers.set(
      listId,
      setTimeout(() => loadLinkList(list, pager, direction, group.edge_type, search.value, 0), 250)
    );
  });
  block.appendChild(search);

  const list = document.createElement("div");
  list.className = "link-list";
  list.id = listId;
  block.appendChild(list);

  const pager = document.createElement("div");
  pager.className = "pagination";
  pager.id = `${listId}-pager`;
  block.appendChild(pager);

  loadLinkList(list, pager, direction, group.edge_type, "", 0);
  return block;
}

function createLinkRow(link) {
  const row = document.createElement("div");
  row.className = "link-row";

  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "link-drill";
  const label = link.neighbor_label || link.neighbor_id;
  btn.innerHTML = `
    <span class="link-drill-type">${escapeHtml(link.neighbor_type)} · ${escapeHtml(link.edge_type)}${edgeRoleMarkup(link.role)}</span>
    <span class="link-drill-label">${escapeHtml(label)}</span>
    <span class="link-drill-id muted">${escapeHtml(link.neighbor_id)}${link.confidence < 1 ? ` · ${escapeHtml(link.confidence)}` : ""}</span>
  `;
  btn.addEventListener("click", () => drillTo(link.neighbor_type, link.neighbor_id));
  row.appendChild(btn);

  const actions = document.createElement("div");
  actions.className = "link-actions";

  if (link.source_url) {
    const a = document.createElement("a");
    a.href = link.source_url;
    a.target = "_blank";
    a.rel = "noopener";
    a.className = "link-action-btn";
    a.textContent = "Source";
    actions.appendChild(a);
  }
  if (link.cache_path) {
    const a = document.createElement("a");
    a.href = cacheUrl(link.cache_path);
    a.target = "_blank";
    a.rel = "noopener";
    a.className = "link-action-btn";
    a.textContent = "Cache";
    actions.appendChild(a);
  }

  const edgeBtn = document.createElement("button");
  edgeBtn.type = "button";
  edgeBtn.className = "link-action-btn";
  edgeBtn.textContent = "Edge";
  edgeBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    showEdgeDetail(link);
  });
  actions.appendChild(edgeBtn);

  row.appendChild(actions);
  return row;
}

async function loadLinkList(listEl, pagerEl, direction, edgeType, q = "", offset = 0) {
  if (!inspectorNode || !listEl) return;

  listEl.innerHTML = `<div class="muted">Loading…</div>`;
  if (pagerEl) pagerEl.innerHTML = "";

  const params = new URLSearchParams({
    direction,
    edge_type: edgeType,
    limit: String(PAGE_SIZE),
    offset: String(offset),
  });
  if (q.trim()) params.set("q", q.trim());

  try {
    const data = await api(
      `/api/node/${encodeURIComponent(inspectorNode.type)}/${inspectorNode.id}/links?${params}`
    );
    listEl.innerHTML = "";
    if (!data.links.length) {
      listEl.innerHTML = `<div class="muted">No matches</div>`;
      return;
    }
    for (const link of data.links) {
      listEl.appendChild(createLinkRow(link));
    }

    if (pagerEl && data.total > PAGE_SIZE) {
      const prev = document.createElement("button");
      prev.type = "button";
      prev.textContent = "← Prev";
      prev.disabled = offset === 0;
      prev.addEventListener("click", () => {
        const search = listEl.parentElement.querySelector(".link-search");
        loadLinkList(
          listEl,
          pagerEl,
          direction,
          edgeType,
          search?.value || "",
          Math.max(0, offset - PAGE_SIZE)
        );
      });

      const info = document.createElement("span");
      info.className = "muted";
      info.textContent = `${offset + 1}–${offset + data.links.length} of ${data.total}`;

      const next = document.createElement("button");
      next.type = "button";
      next.textContent = "Next →";
      next.disabled = offset + PAGE_SIZE >= data.total;
      next.addEventListener("click", () => {
        const search = listEl.parentElement.querySelector(".link-search");
        loadLinkList(listEl, pagerEl, direction, edgeType, search?.value || "", offset + PAGE_SIZE);
      });

      pagerEl.append(prev, info, next);
    } else if (pagerEl) {
      pagerEl.innerHTML = `<span class="muted">${data.total} total</span>`;
    }
  } catch (err) {
    listEl.innerHTML = `<div class="muted">Error: ${escapeHtml(err.message)}</div>`;
  }
}

async function showEdgeDetail(link) {
  const params = new URLSearchParams({
    edge_type: link.edge_type,
    from_type: link.from_type,
    from_id: link.from_id,
    to_type: link.to_type,
    to_id: link.to_id,
  });
  if (link.role) params.set("role", link.role);
  const detail = await api(`/api/edge?${params}`);

  const el = document.getElementById("inspector-content");
  const panel = document.createElement("div");
  panel.className = "inspector-note";
  const roleLine = displayEdgeRole(detail.role)
    ? `<br>role: ${escapeHtml(displayEdgeRole(detail.role))}`
    : "";
  panel.innerHTML = `
    <strong>Edge: ${escapeHtml(detail.edge_type)}</strong><br>
    ${escapeHtml(detail.from_type)}:${escapeHtml(detail.from_id)} →
    ${escapeHtml(detail.to_type)}:${escapeHtml(detail.to_id)}${roleLine}<br>
    confidence: ${escapeHtml(detail.confidence)} · artifact: ${escapeHtml(detail.source_artifact_id || "—")}
  `;
  if (detail.properties_json) {
    try {
      const props = JSON.parse(detail.properties_json);
      const propLines = Object.entries(props)
        .map(([key, value]) => `${key}: ${value}`)
        .join(" · ");
      if (propLines) {
        panel.innerHTML += `<br>properties: ${escapeHtml(propLines)}`;
      }
    } catch (_err) {
      panel.innerHTML += `<br>properties: ${escapeHtml(detail.properties_json)}`;
    }
  }
  if (detail.source_url) {
    panel.innerHTML += `<br><a href="${detail.source_url}" target="_blank" rel="noopener">Edge source URL</a>`;
  }
  if (detail.cache_path) {
    panel.innerHTML += ` · <a href="${cacheUrl(detail.cache_path)}" target="_blank" rel="noopener">Edge cache</a>`;
  }
  el.prepend(panel);
  panel.scrollIntoView({ behavior: "smooth", block: "nearest" });
}

async function showUnresolved(rawName, options = {}) {
  const { fromHistory = false } = options;
  if (!fromHistory) {
    navStack.length = 0;
    pushNav("Unresolved", rawName, rawName);
  }
  renderBreadcrumbs();
  renderProvenanceBar("", "");

  const data = await api(`/api/unresolved/${encodeURIComponent(rawName)}/context`);
  const el = document.getElementById("inspector-content");
  el.innerHTML = "";
  el.classList.remove("muted");

  const header = document.createElement("div");
  header.className = "entity-header";
  header.innerHTML = `
    <span class="type-badge">Unresolved</span>
    <h2>${escapeHtml(rawName)}</h2>
    <div class="entity-meta"><span>${data.total} occurrences · not in identity index</span></div>
  `;
  el.appendChild(header);

  const note = document.createElement("p");
  note.className = "inspector-note";
  note.textContent =
    "This name appears in source data but was not linked to a Person node — often a minister, spelling variant, or missing MP.";
  el.appendChild(note);

  const section = document.createElement("div");
  section.className = "detail-section";
  section.innerHTML = `<h3>Occurrences</h3>`;

  const search = document.createElement("input");
  search.type = "search";
  search.className = "link-search";
  search.placeholder = "Filter occurrences…";
  section.appendChild(search);

  const list = document.createElement("div");
  list.className = "link-list";
  section.appendChild(list);

  const renderRows = (filter) => {
    list.innerHTML = "";
    const f = filter.toLowerCase();
    const rows = data.rows.filter(
      (r) =>
        !f ||
        r.context_label?.toLowerCase().includes(f) ||
        r.context_id?.toLowerCase().includes(f) ||
        r.source_bucket?.toLowerCase().includes(f)
    );
    for (const row of rows.slice(0, 100)) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "link-drill";
      btn.innerHTML = `
        <span class="link-drill-type">${escapeHtml(row.source_bucket)} · ${escapeHtml(row.role)}</span>
        <span class="link-drill-label">${escapeHtml(row.context_label || row.context_id)}</span>
      `;
      const actions = document.createElement("div");
      actions.className = "link-actions";
      if (row.source_url) {
        const a = document.createElement("a");
        a.href = row.source_url;
        a.target = "_blank";
        a.className = "link-action-btn";
        a.textContent = "Source";
        actions.appendChild(a);
      }
      if (row.cache_path) {
        const a = document.createElement("a");
        a.href = cacheUrl(row.cache_path);
        a.target = "_blank";
        a.className = "link-action-btn";
        a.textContent = "Cache";
        actions.appendChild(a);
      }
      const wrap = document.createElement("div");
      wrap.className = "link-row";
      btn.addEventListener("click", () => {
        if (row.context_id?.includes("_")) {
          openNode(
            row.source_bucket === "votes" ? "Vote" : "Question",
            row.context_id,
            { resetNav: false }
          );
        }
      });
      wrap.append(btn, actions);
      list.appendChild(wrap);
    }
    if (rows.length > 100) {
      list.innerHTML += `<div class="muted">Showing first 100 of ${rows.length} matches</div>`;
    }
  };

  search.addEventListener("input", () => renderRows(search.value));
  renderRows("");
  el.appendChild(section);
  if (!fromHistory) {
    writeHistory();
  }
}

document.getElementById("search-btn").addEventListener("click", runSearch);
document.getElementById("search-input").addEventListener("keydown", (e) => {
  if (e.key === "Enter") runSearch();
});

window.addEventListener("popstate", (event) => {
  reportContext = event.state?.reportContext || null;
  if (event.state?.navStack?.length) {
    navStack.length = 0;
    navStack.push(...event.state.navStack);
    renderBreadcrumbs();
    const item = navStack[navStack.length - 1];
    if (item.type === "Report") {
      openReportContext(reportContext, { fromHistory: true });
    } else if (item.type === "Unresolved") {
      showUnresolved(item.id, { fromHistory: true });
    } else {
      openNode(item.type, item.id, { resetNav: false, fromHistory: true });
    }
    return;
  }
  navStack.length = 0;
  renderBreadcrumbs();
  resetInspector();
});

async function initFromUrl() {
  const params = new URLSearchParams(window.location.search);
  const type = params.get("type");
  const id = params.get("id");
  const unresolved = params.get("unresolved");
  const report = params.get("report");
  const block = params.get("block");
  if (report) {
    reportContext = {
      sessionId: params.get("session_id") || "56",
      meetingKind: params.get("meeting_kind") || "plenary",
      meetingId: report,
      blockIndex: block == null ? null : Number(block),
      entityTypes: params.getAll("entity_type"),
      coverageKinds: params.getAll("coverage_kind"),
      spanRoles: params.getAll("span_role"),
    };
    applyReportContextToControls(reportContext);
  }
  if (type && id) {
    seedHomeHistoryEntry();
    if (reportContext) {
      pushNav(
        "Report",
        `${reportContext.sessionId}/${reportContext.meetingKind}/${reportContext.meetingId}`,
        `${reportContext.meetingKind} ${reportContext.meetingId}`
      );
    }
    await openNode(type, id, { resetNav: !reportContext });
    return;
  }
  if (report) {
    navStack.length = 0;
    pushNav(
      "Report",
      `${reportContext.sessionId}/${reportContext.meetingKind}/${report}`,
      `${reportContext.meetingKind} ${report}`
    );
    history.replaceState(
      { navStack: navStackSnapshot(), reportContext: reportContextSnapshot() },
      "",
      currentViewUrl()
    );
    await openReportContext(reportContext, { fromHistory: true });
    return;
  }
  if (unresolved) {
    seedHomeHistoryEntry();
    await showUnresolved(unresolved);
  }
}

async function loadReportMeetings() {
  const select = document.getElementById("report-meeting");
  if (!select) return;
  const sessionId = document.getElementById("report-session")?.value.trim() || "56";
  const meetingKind = document.getElementById("report-kind")?.value || "plenary";
  const query = new URLSearchParams({
    session_id: sessionId,
    meeting_kind: meetingKind,
  });
  const previous = reportContext?.meetingId || select.value;
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

function renderStructuredTable(structured) {
  const rows = structured?.table_rows;
  if (!Array.isArray(rows) || !rows.length) return "";
  const body = rows
    .map((row) => {
      const cells = (row.cells || [])
        .map((cell) => {
          const colspan = Math.max(1, Math.min(20, Number(cell.colspan) || 1));
          const rowspan = Math.max(1, Math.min(100, Number(cell.rowspan) || 1));
          const tag = cell.is_header || row.is_header ? "th" : "td";
          return `<${tag} colspan="${colspan}" rowspan="${rowspan}">${escapeHtml(cell.text || "")}</${tag}>`;
        })
        .join("");
      return `<tr>${cells}</tr>`;
    })
    .join("");
  return `<table class="report-table">${body}</table>`;
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

function reportContextFromControls(blockIndex = null) {
  return {
    sessionId: document.getElementById("report-session")?.value.trim() || "56",
    meetingKind: document.getElementById("report-kind")?.value || "plenary",
    meetingId: document.getElementById("report-meeting")?.value || "",
    blockIndex,
    entityTypes: selectedValues("report-entity-filter"),
    coverageKinds: selectedValues("report-coverage-filter"),
    spanRoles: (document.getElementById("report-role-filter")?.value || "")
      .split(",")
      .map((value) => value.trim())
      .filter(Boolean),
  };
}

function applyReportContextToControls(context) {
  if (!context) return;
  const session = document.getElementById("report-session");
  const kind = document.getElementById("report-kind");
  const meeting = document.getElementById("report-meeting");
  if (session) session.value = context.sessionId;
  if (kind) kind.value = context.meetingKind;
  if (meeting) meeting.value = context.meetingId;
  setSelectedValues("report-entity-filter", context.entityTypes);
  setSelectedValues("report-coverage-filter", context.coverageKinds);
  const role = document.getElementById("report-role-filter");
  if (role) role.value = (context.spanRoles || []).join(", ");
}

function updateReportUrl(meetingId, blockIndex) {
  reportContext = reportContextFromControls(blockIndex);
  reportContext.meetingId = meetingId || reportContext.meetingId;
  const top = navStack[navStack.length - 1];
  if (!top || top.type !== "Report") {
    navStack.length = 0;
    pushNav(
      "Report",
      `${reportContext.sessionId}/${reportContext.meetingKind}/${reportContext.meetingId}`,
      `${reportContext.meetingKind} ${reportContext.meetingId}`
    );
  }
  const next = `${window.location.pathname}?${addReportParams(new URLSearchParams()).toString()}`;
  window.history.replaceState(
    { navStack: navStackSnapshot(), reportContext: reportContextSnapshot() },
    "",
    next
  );
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
  appendMetadataRow(card, "Extractor version", span.extractor_version);
  appendMetadataRow(card, "Parser version", span.block_parser_version);
  appendMetadataRow(card, "Status", span.validation_status);
  appendMetadataRow(card, "Reason", span.unresolved_reason);

  const actions = document.createElement("div");
  actions.className = "span-actions";
  const graph = document.createElement("button");
  graph.type = "button";
  graph.className = "link-action-btn";
  graph.textContent = "Open graph node";
  graph.addEventListener("click", () =>
    openNode(span.entity_type, span.entity_id, { resetNav: false })
  );
  actions.appendChild(graph);
  if (span.source_url) {
    const source = document.createElement("a");
    source.href = span.source_url;
    source.target = "_blank";
    source.rel = "noopener";
    source.textContent = "Source";
    actions.appendChild(source);
  }
  if (span.cache_path) {
    const cache = document.createElement("a");
    cache.href = cacheUrl(span.cache_path);
    cache.target = "_blank";
    cache.rel = "noopener";
    cache.textContent = "Cached report";
    actions.appendChild(cache);
  }
  card.appendChild(actions);
  return card;
}

function showReportBlockDetail(block) {
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
  appendMetadataRow(panel, "Artifact", block.artifact_id);
  appendMetadataRow(panel, "Content hash", block.content_hash);
  appendMetadataRow(panel, "Source hash", block.source_content_hash);
  appendMetadataRow(panel, "Parser version", block.block_parser_version);
  appendMetadataRow(panel, "Block extractor", block.extractor_version);
  if (!block.spans?.length) {
    const empty = document.createElement("p");
    empty.className = "muted";
    empty.textContent = "No spans overlap this block.";
    panel.appendChild(empty);
  }
  for (const span of block.spans || []) panel.appendChild(renderSpanDetail(span));
}

async function loadReportCoverage(focusBlock) {
  const select = document.getElementById("report-meeting");
  const container = document.getElementById("report-blocks");
  const stats = document.getElementById("report-coverage-stats");
  const diagnostics = document.getElementById("report-diagnostics");
  if (!select || !container || !stats || !diagnostics) return;
  const meetingId = select.value;
  if (!meetingId) {
    stats.textContent = "No meeting is available for this session and kind.";
    diagnostics.innerHTML = "";
    showReportBlockDetail(null);
    container.innerHTML = "";
    return;
  }
  const context = reportContextFromControls(focusBlock ?? reportContext?.blockIndex ?? null);
  const params = new URLSearchParams();
  for (const value of context.entityTypes) params.append("entity_type", value);
  for (const value of context.coverageKinds) params.append("coverage_kind", value);
  for (const value of context.spanRoles) params.append("span_role", value);
  const query = params.toString();
  stats.textContent = "Loading report coverage…";
  diagnostics.innerHTML = "";
  container.innerHTML = "";
  showReportBlockDetail(null);
  let data;
  try {
    data = await api(
      `/api/reports/${encodeURIComponent(context.sessionId)}/${encodeURIComponent(context.meetingKind)}/${encodeURIComponent(meetingId)}${query ? `?${query}` : ""}`
    );
  } catch (err) {
    stats.textContent = `Could not load report: ${err.message}`;
    diagnostics.innerHTML = "";
    container.innerHTML = "<p class='report-state error'>Report data could not be loaded.</p>";
    showReportBlockDetail(null);
    return;
  }
  const ratio = Math.round((data.coverage?.ratio || 0) * 100);
  stats.textContent = `Extraction coverage: ${data.coverage?.covered_words || 0}/${data.coverage?.total_words || 0} words (${ratio}%) · ${data.coverage?.valid_span_count || 0} valid / ${data.coverage?.invalid_span_count || 0} invalid spans`;
  for (const item of data.diagnostics || []) {
    const note = document.createElement("div");
    note.className = `report-diagnostic ${item.state}`;
    note.textContent = `${item.code}: ${item.message}${item.count > 1 ? ` (${item.count})` : ""}`;
    diagnostics.appendChild(note);
  }
  if (!data.blocks?.length) {
    const missing = document.createElement("p");
    missing.className = `report-state ${data.derived_data_status === "missing" ? "missing" : "empty"}`;
    missing.textContent =
      data.derived_data_status === "missing"
        ? "Derived report blocks are missing for this report."
        : "This report contains no blocks.";
    container.appendChild(missing);
    showReportBlockDetail(null);
    updateReportUrl(meetingId, null);
    return;
  }
  const selectedBlock = focusBlock ?? context.blockIndex;
  updateReportUrl(meetingId, selectedBlock);
  container.innerHTML = "";
  for (const block of data.blocks) {
    const article = document.createElement("article");
    const hasExtraction = block.has_extraction;
    article.className = "report-block";
    if (hasExtraction) article.classList.add("covered");
    else if (block.has_scope) article.classList.add("scope-only");
    else article.classList.add("uncovered");
    if (block.has_invalid) article.classList.add("invalid");
    if (block.has_stale) article.classList.add("stale");
    if (selectedBlock != null && Number(block.block_index) === Number(selectedBlock)) {
      article.classList.add("selected");
      showReportBlockDetail(block);
    }
    const header = document.createElement("header");
    const label = document.createElement("strong");
    label.textContent = `${block.block_type} #${block.block_index}`;
    header.appendChild(label);
    for (const span of block.spans || []) {
      const badge = document.createElement("span");
      badge.className = "span-badge";
      badge.classList.add(span.coverage_kind === "scope" ? "scope" : "extraction");
      if (span.validation_status !== "valid") {
        badge.classList.add(span.unresolved_reason.startsWith("stale_") ? "stale" : "invalid");
      }
      badge.textContent = `${span.entity_type}:${span.span_role}`;
      badge.title = `${span.coverage_kind} · ${span.field_names || "no fields"} · ${span.extractor || "unknown extractor"} ${span.extractor_version || ""}`;
      badge.addEventListener("click", (e) => {
        e.stopPropagation();
        openNode(span.entity_type, span.entity_id, { resetNav: false });
      });
      header.appendChild(badge);
    }
    article.appendChild(header);
    if (block.block_type === "table" && block.structured) {
      article.insertAdjacentHTML("beforeend", renderStructuredTable(block.structured));
    } else {
      const body = document.createElement(
        block.block_type === "h1" ? "h3" : block.block_type === "h2" ? "h4" : "p"
      );
      body.textContent = block.text || "";
      article.appendChild(body);
    }
    article.addEventListener("click", () => {
      container.querySelectorAll(".report-block.selected").forEach((el) => el.classList.remove("selected"));
      article.classList.add("selected");
      showReportBlockDetail(block);
      updateReportUrl(meetingId, block.block_index);
    });
    container.appendChild(article);
  }
  if (selectedBlock != null) {
    const selected = container.querySelector(".report-block.selected");
    selected?.scrollIntoView({ block: "center" });
  }
}

async function openReportContext(context, options = {}) {
  if (!context) return;
  const { fromHistory = false } = options;
  reportContext = { ...context };
  applyReportContextToControls(reportContext);
  await loadReportMeetings();
  applyReportContextToControls(reportContext);
  document.getElementById("report-section")?.classList.add("fullscreen");
  document.getElementById("report-close")?.classList.remove("hidden");
  if (!fromHistory) writeHistory();
  await loadReportCoverage(reportContext.blockIndex);
}

async function openReportAtBlock(
  meetingId,
  blockIndex,
  sessionId = "56",
  meetingKind = "plenary"
) {
  const current = reportContextFromControls(Number(blockIndex));
  current.sessionId = String(sessionId);
  current.meetingKind = meetingKind;
  current.meetingId = String(meetingId);
  current.blockIndex = Number(blockIndex);
  reportContext = current;
  pushNav(
    "Report",
    `${current.sessionId}/${current.meetingKind}/${current.meetingId}`,
    `${current.meetingKind} ${current.meetingId}`
  );
  await openReportContext(current);
}

function bindReportControls() {
  const reload = () => loadReportCoverage().catch((err) => {
    document.getElementById("report-coverage-stats").textContent = `Error: ${err.message}`;
  });
  document.getElementById("report-meeting")?.addEventListener("change", reload);
  document.getElementById("report-entity-filter")?.addEventListener("change", reload);
  document.getElementById("report-coverage-filter")?.addEventListener("change", reload);
  document.getElementById("report-role-filter")?.addEventListener("change", reload);
  const reloadMeetings = async () => {
    reportContext = null;
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
  document.getElementById("report-close")?.addEventListener("click", () => {
    if (navStack.length > 1) navigateToIndex(navStack.length - 2);
    else {
      document.getElementById("report-section")?.classList.remove("fullscreen");
      document.getElementById("report-close")?.classList.add("hidden");
    }
  });
}

async function init() {
  try {
    await loadHealth();
    await loadIssues();
    bindReportControls();
    await loadReportMeetings();
    const hasInitialView = new URLSearchParams(window.location.search).toString() !== "";
    await initFromUrl();
    const select = document.getElementById("report-meeting");
    if (!hasInitialView && select?.value) await loadReportCoverage();
  } catch (err) {
    document.getElementById("health-status").textContent = `Error: ${err.message}`;
  }
}

init();
