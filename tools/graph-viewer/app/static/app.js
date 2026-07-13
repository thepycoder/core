let inspectorNode = null;
const navStack = [];
const linkSearchTimers = new Map();
const PAGE_SIZE = 40;

function navStackSnapshot() {
  return navStack.map((item) => ({ type: item.type, id: item.id, label: item.label }));
}

function viewUrlForItem(item) {
  if (!item) return window.location.pathname;
  if (item.type === "Unresolved") {
    return `?${new URLSearchParams({ unresolved: item.id })}`;
  }
  return `?${new URLSearchParams({ type: item.type, id: item.id })}`;
}

function currentViewUrl() {
  return viewUrlForItem(navStack[navStack.length - 1]);
}

function writeHistory() {
  history.pushState({ navStack: navStackSnapshot() }, "", currentViewUrl());
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
  history.pushState({ navStack: navStackSnapshot() }, "", viewUrlForItem(item));
  openNode(item.type, item.id, { resetNav: false, fromHistory: true });
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
      <dt>Parser version</dt><dd>${escapeHtml(detail.parser_version || "—")}</dd>
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

  if (detail.utterances?.length) {
    el.appendChild(renderUtterances(detail.utterances, detail.utterance_section_title));
  }

  appendLinkSection(el, "Outgoing links", "out", detail.out_edges);
  appendLinkSection(el, "Incoming links", "in", detail.in_edges);
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

function renderUtterances(utterances, sectionTitle) {
  const section = document.createElement("div");
  section.className = "detail-section";
  const title = sectionTitle || "Discussion";
  section.innerHTML = `<h3>${escapeHtml(title)} (${utterances.length} utterances)</h3>`;
  for (const u of utterances) {
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
    section.appendChild(block);
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
    <span class="link-drill-id muted">${escapeHtml(link.neighbor_id)}${link.confidence !== "exact" ? ` · ${escapeHtml(link.confidence)}` : ""}</span>
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
  if (event.state?.navStack?.length) {
    navStack.length = 0;
    navStack.push(...event.state.navStack);
    renderBreadcrumbs();
    const item = navStack[navStack.length - 1];
    if (item.type === "Unresolved") {
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
  if (type && id) {
    seedHomeHistoryEntry();
    await openNode(type, id, { resetNav: true });
    return;
  }
  if (unresolved) {
    seedHomeHistoryEntry();
    await showUnresolved(unresolved);
  }
}

async function init() {
  try {
    await loadHealth();
    await loadIssues();
    await initFromUrl();
  } catch (err) {
    document.getElementById("health-status").textContent = `Error: ${err.message}`;
  }
}

init();
