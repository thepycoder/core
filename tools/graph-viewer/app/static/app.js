let inspectorNode = null;
const navStack = [];
const linkSearchTimers = new Map();
const PAGE_SIZE = 40;

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
  const item = navStack[index];
  openNode(item.type, item.id, false);
}

function renderBreadcrumbs() {
  const bar = document.getElementById("breadcrumb-bar");
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

function seedFromSample(issueId, sample) {
  const d = sample.data || sample;
  switch (issueId) {
    case "orphan_to":
      return { type: d.to_type, id: d.to_id };
    case "orphan_from":
      return { type: d.from_type, id: d.from_id };
    case "vote_reconciliation":
      return { type: "Vote", id: d.vote_id };
    case "duplicate_utterance_ids":
    case "utterances_without_spoke":
      return { type: "Utterance", id: d.utterance_id };
    default:
      return null;
  }
}

async function loadHealth() {
  const health = await api("/api/health");
  const missing = health.files.filter((f) => !f.exists).length;
  const el = document.getElementById("health-status");
  el.textContent = `${health.data_dir} · ${missing ? `${missing} missing files` : "data ok"}`;
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
      s.className = "sample";
      s.textContent = JSON.stringify(sample.data);
      s.addEventListener("click", () => {
        const seed = seedFromSample(issue.id, sample);
        if (seed) openNode(seed.type, seed.id, true);
      });
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
  navStack.length = 0;
  if (row.source === "node" || row.source === "content") {
    await openNode(row.type, row.id, true);
    return;
  }
  await showUnresolved(row.label);
}

async function openNode(type, id, resetNav = true) {
  if (resetNav) {
    navStack.length = 0;
  }
  const detail = await api(`/api/node/${encodeURIComponent(type)}/${id}`);
  pushNav(type, id, detail.label);
  inspectorNode = { type, id };
  renderNodeDetail(detail);
}

async function drillTo(neighborType, neighborId) {
  await openNode(neighborType, neighborId, false);
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
    el.appendChild(renderUtterances(detail.utterances));
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

function renderUtterances(utterances) {
  const section = document.createElement("div");
  section.className = "detail-section";
  section.innerHTML = `<h3>Discussion (${utterances.length} utterances)</h3>`;
  for (const u of utterances) {
    const block = document.createElement("div");
    block.className = "utterance-block";
    const speaker = u.speaker_person_id
      ? `${u.raw_speaker} → Person ${u.speaker_person_id}`
      : u.speaker_entity_id
        ? `${u.raw_speaker} → ${u.speaker_entity_type} ${u.speaker_entity_id}`
        : u.raw_speaker;
    block.innerHTML = `
      <div class="speaker">${escapeHtml(speaker)}</div>
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
    if (!u.speaker_person_id && !u.speaker_entity_id && u.raw_speaker) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "link-action-btn";
      btn.textContent = "Trace speaker";
      btn.style.marginTop = "0.35rem";
      btn.addEventListener("click", () => showUnresolved(u.raw_speaker));
      block.appendChild(btn);
    } else if (u.speaker_entity_id && u.speaker_entity_type) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "link-action-btn";
      btn.textContent = `Open ${u.speaker_entity_type}`;
      btn.style.marginTop = "0.35rem";
      btn.addEventListener("click", () => drillTo(u.speaker_entity_type, u.speaker_entity_id));
      block.appendChild(btn);
    }
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
  search.placeholder = `Filter by title, id, edge type…`;
  search.addEventListener("input", () => {
    clearTimeout(linkSearchTimers.get(listId));
    linkSearchTimers.set(
      listId,
      setTimeout(() => loadLinkList(listId, direction, group.edge_type, search.value, 0), 250)
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

  loadLinkList(listId, direction, group.edge_type, "", 0);
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
    <span class="link-drill-type">${escapeHtml(link.neighbor_type)} · ${escapeHtml(link.edge_type)}</span>
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

async function loadLinkList(listId, direction, edgeType, q = "", offset = 0) {
  if (!inspectorNode) return;
  const listEl = document.getElementById(listId);
  const pagerEl = document.getElementById(`${listId}-pager`);
  if (!listEl) return;

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
        loadLinkList(listId, direction, edgeType, search?.value || "", Math.max(0, offset - PAGE_SIZE));
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
        loadLinkList(listId, direction, edgeType, search?.value || "", offset + PAGE_SIZE);
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
  const detail = await api(`/api/edge?${params}`);

  const el = document.getElementById("inspector-content");
  const panel = document.createElement("div");
  panel.className = "inspector-note";
  panel.innerHTML = `
    <strong>Edge: ${escapeHtml(detail.edge_type)}</strong><br>
    ${escapeHtml(detail.from_type)}:${escapeHtml(detail.from_id)} →
    ${escapeHtml(detail.to_type)}:${escapeHtml(detail.to_id)}<br>
    confidence: ${escapeHtml(detail.confidence)} · artifact: ${escapeHtml(detail.source_artifact_id || "—")}
  `;
  if (detail.source_url) {
    panel.innerHTML += `<br><a href="${detail.source_url}" target="_blank" rel="noopener">Edge source URL</a>`;
  }
  if (detail.cache_path) {
    panel.innerHTML += ` · <a href="${cacheUrl(detail.cache_path)}" target="_blank" rel="noopener">Edge cache</a>`;
  }
  el.prepend(panel);
  panel.scrollIntoView({ behavior: "smooth", block: "nearest" });
}

async function showUnresolved(rawName) {
  navStack.length = 0;
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
            true
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
}

document.getElementById("search-btn").addEventListener("click", runSearch);
document.getElementById("search-input").addEventListener("keydown", (e) => {
  if (e.key === "Enter") runSearch();
});

async function init() {
  try {
    await loadHealth();
    await loadIssues();
  } catch (err) {
    document.getElementById("health-status").textContent = `Error: ${err.message}`;
  }
}

init();
