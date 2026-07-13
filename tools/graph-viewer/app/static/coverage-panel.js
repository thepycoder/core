/**
 * Embedded report coverage viewer — opened from graph inspector selections.
 */
const CoveragePanel = (() => {
  let reportData = null;
  let selectedBlockIndex = null;
  let overlayFrame = null;
  let onNavigateNode = null;
  let controlsExpanded = false;

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

  function el(id) {
    return document.getElementById(id);
  }

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

  function selectedValues(id) {
    const select = el(id);
    return select ? [...select.selectedOptions].map((option) => option.value).filter(Boolean) : [];
  }

  function setSelectedValues(id, values) {
    const wanted = new Set(values || []);
    const select = el(id);
    if (!select) return;
    for (const option of select.options) option.selected = wanted.has(option.value);
  }

  function contextFromControls() {
    return {
      sessionId: el("coverage-session")?.value.trim() || "56",
      meetingKind: el("coverage-kind")?.value || "plenary",
      meetingId: el("coverage-meeting")?.value || "",
      blockIndex: selectedBlockIndex,
      entityTypes: selectedValues("coverage-entity-filter"),
      entityIds: (el("coverage-entity-id")?.value || "")
        .split(",")
        .map((value) => value.trim())
        .filter(Boolean),
      coverageKinds: selectedValues("coverage-coverage-filter"),
      spanRoles: (el("coverage-role-filter")?.value || "")
        .split(",")
        .map((value) => value.trim())
        .filter(Boolean),
    };
  }

  function applyContextToControls(context) {
    if (!context) return;
    if (el("coverage-session")) el("coverage-session").value = context.sessionId || "56";
    if (el("coverage-kind")) el("coverage-kind").value = context.meetingKind || "plenary";
    if (el("coverage-meeting") && context.meetingId) el("coverage-meeting").value = context.meetingId;
    setSelectedValues("coverage-entity-filter", context.entityTypes || []);
    if (el("coverage-entity-id")) {
      el("coverage-entity-id").value = (context.entityIds || []).join(", ");
    }
    setSelectedValues("coverage-coverage-filter", context.coverageKinds || []);
    if (el("coverage-role-filter")) {
      el("coverage-role-filter").value = (context.spanRoles || []).join(", ");
    }
  }

  function cleanText(raw) {
    return (raw || "").replace(/\u00ad/g, "").replace(/\u00a0/g, " ").trim();
  }

  function blockText(element, tag) {
    if (tag === "table") {
      return [...element.querySelectorAll("td, th")]
        .map((cell) => cleanText(cell.textContent))
        .filter(Boolean)
        .join(" ");
    }
    return cleanText((element.textContent || "").replace(/\n/g, " "));
  }

  function collectBlockElements(doc) {
    const nodes = [];
    for (const element of doc.querySelectorAll("h1, h2, p, table")) {
      const tag = element.tagName.toLowerCase();
      if (tag === "p" && element.closest("table")) continue;
      if (!blockText(element, tag)) continue;
      nodes.push(element);
    }
    return nodes;
  }

  function clearOverlayClasses(element) {
    element.classList.remove(
      "pg-block",
      "pg-covered",
      "pg-scope",
      "pg-uncovered",
      "pg-invalid",
      "pg-stale",
      "pg-selected"
    );
    element.removeAttribute("data-pg-block");
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
    for (const element of doc.querySelectorAll("[data-pg-block]")) clearOverlayClasses(element);

    elements.forEach((element, index) => {
      const block = blockMap.get(index);
      element.setAttribute("data-pg-block", String(index));
      element.classList.add("pg-block");
      if (block?.has_extraction) element.classList.add("pg-covered");
      else if (block?.has_scope) element.classList.add("pg-scope");
      else element.classList.add("pg-uncovered");
      if (block?.has_invalid) element.classList.add("pg-invalid");
      if (block?.has_stale) element.classList.add("pg-stale");
      if (focusIndex != null && Number(focusIndex) === index) element.classList.add("pg-selected");
      element.onclick = (event) => {
        event.preventDefault();
        selectBlock(index, { scroll: false });
      };
    });

    if (focusIndex != null) {
      elements[Number(focusIndex)]?.scrollIntoView({ block: "center", behavior: "smooth" });
    }
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
    const graph = document.createElement("button");
    graph.type = "button";
    graph.className = "link-action-btn";
    graph.textContent = "Open in inspector";
    graph.addEventListener("click", () => {
      if (onNavigateNode) onNavigateNode(span.entity_type, span.entity_id);
    });
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
    const panel = el("coverage-block-detail");
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

    const frame = el("coverage-frame");
    const doc = frame?.contentDocument;
    if (!doc) return;
    for (const element of doc.querySelectorAll("[data-pg-block]")) {
      element.classList.toggle("pg-selected", element.getAttribute("data-pg-block") === String(index));
    }
    if (scroll) {
      collectBlockElements(doc)[Number(index)]?.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  }

  async function loadReportMeetings() {
    const select = el("coverage-meeting");
    if (!select) return;
    const sessionId = el("coverage-session")?.value.trim() || "56";
    const meetingKind = el("coverage-kind")?.value || "plenary";
    const previous = select.value;
    const data = await api(`/api/reports/meetings?session_id=${sessionId}&meeting_kind=${meetingKind}`);
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
    const panel = el("coverage-provenance");
    if (!panel) return;
    panel.innerHTML = "";
    if (data.source_url) {
      const source = document.createElement("div");
      source.innerHTML = `<a href="${escapeHtml(data.source_url)}" target="_blank" rel="noopener">Open on dekamer.be</a>`;
      panel.appendChild(source);
    }
    if (data.cache_path) {
      const cache = document.createElement("div");
      cache.innerHTML = `<a href="${escapeHtml(cacheUrl(data.cache_path))}" target="_blank" rel="noopener">Open raw cached HTML</a>`;
      panel.appendChild(cache);
    }
    if (data.block_parser_version) {
      const versions = document.createElement("div");
      versions.textContent = `Parser ${data.block_parser_version} · extractor ${data.extractor_version || "—"}`;
      panel.appendChild(versions);
    }
  }

  function showFallbackLinks(context) {
    const stats = el("coverage-stats");
    const diagnostics = el("coverage-diagnostics");
    const title = el("coverage-title");
    if (title) title.textContent = context.label || "Origins";
    if (stats) stats.textContent = "No meeting report is linked to this selection.";
    if (diagnostics) {
      diagnostics.innerHTML = "";
      const note = document.createElement("div");
      note.className = "report-diagnostic missing";
      note.textContent =
        "Pick a meeting below or open a node with report source spans (votes, questions, utterances, …).";
      diagnostics.appendChild(note);
    }
    showBlockDetail(null);
    showViewerStatus("Select a meeting to load cached report HTML.");

    const provenance = el("coverage-provenance");
    if (provenance) {
      provenance.innerHTML = "";
      if (context.sourceUrl) {
        const source = document.createElement("div");
        source.innerHTML = `<a href="${escapeHtml(context.sourceUrl)}" target="_blank" rel="noopener">Open on dekamer.be</a>`;
        provenance.appendChild(source);
      }
      if (context.cachePath) {
        const cache = document.createElement("div");
        cache.innerHTML = `<a href="${escapeHtml(cacheUrl(context.cachePath))}" target="_blank" rel="noopener">Open raw cached HTML</a>`;
        provenance.appendChild(cache);
      }
    }
  }

  async function loadReportHtml(cachePath) {
    const res = await fetch(cacheUrl(cachePath));
    if (!res.ok) throw new Error(`Could not load cached HTML (${res.status})`);
    return res.text();
  }

  function showViewerStatus(message) {
    const status = el("coverage-viewer-status");
    const frame = el("coverage-frame");
    if (status) {
      status.textContent = message;
      status.classList.remove("hidden");
    }
    if (frame) frame.classList.add("hidden");
  }

  function mountReportHtml(html, blocks, focusIndex) {
    const status = el("coverage-viewer-status");
    const frame = el("coverage-frame");
    if (!frame) return Promise.resolve();

    if (overlayFrame) overlayFrame.onload = null;
    overlayFrame = frame;

    return new Promise((resolve, reject) => {
      frame.onload = () => {
        try {
          applyOverlays(frame.contentDocument, blocks, focusIndex);
          status?.classList.add("hidden");
          frame.classList.remove("hidden");
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
    const select = el("coverage-meeting");
    const stats = el("coverage-stats");
    const diagnostics = el("coverage-diagnostics");
    const title = el("coverage-title");
    if (!select || !stats || !diagnostics) return;

    const context = contextFromControls();
    const meetingId = select.value || context.meetingId;
    if (!meetingId) {
      showFallbackLinks(context);
      return;
    }

    const params = new URLSearchParams();
    for (const value of context.entityTypes) params.append("entity_type", value);
    for (const value of context.entityIds) params.append("entity_id", value);
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

  function setControlsExpanded(expanded) {
    controlsExpanded = expanded;
    const section = el("coverage-controls");
    const toggle = el("coverage-controls-toggle");
    if (section) section.classList.toggle("hidden", !expanded);
    if (toggle) {
      toggle.textContent = expanded ? "Hide meeting & filters" : "Meeting & filters";
      toggle.setAttribute("aria-expanded", expanded ? "true" : "false");
    }
  }

  function bindControls() {
    const reload = () => loadReportCoverage().catch((err) => {
      const stats = el("coverage-stats");
      if (stats) stats.textContent = `Error: ${err.message}`;
    });

    el("coverage-meeting")?.addEventListener("change", reload);
    el("coverage-entity-filter")?.addEventListener("change", reload);
    el("coverage-entity-id")?.addEventListener("change", reload);
    el("coverage-coverage-filter")?.addEventListener("change", reload);
    el("coverage-role-filter")?.addEventListener("change", reload);

    const reloadMeetings = async () => {
      selectedBlockIndex = null;
      await loadReportMeetings();
      await loadReportCoverage();
    };
    el("coverage-session")?.addEventListener("change", () => {
      reloadMeetings().catch(console.error);
    });
    el("coverage-kind")?.addEventListener("change", () => {
      reloadMeetings().catch(console.error);
    });

    el("coverage-clear-filters")?.addEventListener("click", () => {
      setSelectedValues("coverage-entity-filter", []);
      setSelectedValues("coverage-coverage-filter", []);
      if (el("coverage-entity-id")) el("coverage-entity-id").value = "";
      if (el("coverage-role-filter")) el("coverage-role-filter").value = "";
      reload();
    });

    el("coverage-controls-toggle")?.addEventListener("click", () => {
      setControlsExpanded(!controlsExpanded);
    });

    el("coverage-close")?.addEventListener("click", () => close());
  }

  function setOpen(open) {
    const drawer = el("coverage-drawer");
    if (!drawer) return;
    drawer.classList.toggle("open", open);
    drawer.setAttribute("aria-hidden", open ? "false" : "true");
    document.body.classList.toggle("coverage-open", open);
  }

  async function open(context = {}) {
    const label = el("coverage-context-label");
    if (label) label.textContent = context.label || "";

    setOpen(true);
    setControlsExpanded(Boolean(context.expandControls));

    selectedBlockIndex = context.blockIndex ?? null;
    applyContextToControls({
      sessionId: context.sessionId || "56",
      meetingKind: context.meetingKind || "plenary",
      meetingId: context.meetingId || "",
      entityTypes: context.entityTypes || [],
      entityIds: context.entityIds || [],
      coverageKinds: context.coverageKinds || [],
      spanRoles: context.spanRoles || [],
    });

    await loadReportMeetings();
    applyContextToControls({
      sessionId: context.sessionId || "56",
      meetingKind: context.meetingKind || "plenary",
      meetingId: context.meetingId || "",
      entityTypes: context.entityTypes || [],
      entityIds: context.entityIds || [],
      coverageKinds: context.coverageKinds || [],
      spanRoles: context.spanRoles || [],
    });

    if (context.meetingId) {
      await loadReportCoverage(context.blockIndex ?? null);
    } else {
      showFallbackLinks(context);
    }
  }

  function close() {
    setOpen(false);
  }

  function isOpen() {
    return el("coverage-drawer")?.classList.contains("open") ?? false;
  }

  function init() {
    bindControls();
    setControlsExpanded(false);
  }

  return { init, open, close, isOpen, setNavigateHandler(handler) { onNavigateNode = handler; } };
})();
