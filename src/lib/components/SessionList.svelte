<script lang="ts">
  import type { SessionSummary } from "../types";
  import { keyOf, trLabel } from "../filters";
  import {
    durationMs,
    fmtCompact,
    fmtDateTime,
    fmtDuration,
    fmtInt,
    fmtUsd,
    projectName,
    sessionTotal,
    totalTokens,
  } from "../format";

  interface Props {
    sessions: SessionSummary[];
    totalCount: number;
    loading: boolean;
    error: string | null;
    onopen: (s: SessionSummary) => void;
  }
  let { sessions, totalCount, loading, error, onopen }: Props = $props();

  type SortKey =
    | "source"
    | "project"
    | "models"
    | "startedAt"
    | "duration"
    | "input"
    | "output"
    | "cacheCreation"
    | "cacheRead"
    | "total"
    | "cost";

  let sortKey = $state<SortKey>("startedAt");
  let sortDir = $state<"asc" | "desc">("desc");
  let page = $state(1);
  let pageSize = $state(25);

  let expanded = $state<Record<string, boolean>>({});
  const LOG_DELETED_TIP = "Original log file was removed; data kept in local database";

  const columns: { key: SortKey; label: string; num?: boolean }[] = [
    { key: "source", label: "Source" },
    { key: "project", label: "Project" },
    { key: "models", label: "Models" },
    { key: "startedAt", label: "Started" },
    { key: "duration", label: "Duration", num: true },
    { key: "input", label: "Input", num: true },
    { key: "output", label: "Output", num: true },
    { key: "cacheCreation", label: "Cache write", num: true },
    { key: "cacheRead", label: "Cache read", num: true },
    { key: "total", label: "Total tokens", num: true },
    { key: "cost", label: "Cost USD", num: true },
  ];

  function sortValue(s: SessionSummary, k: SortKey): string | number | null {
    switch (k) {
      case "source":
        return s.source;
      case "project":
        return projectName(s.project).toLowerCase();
      case "models":
        return s.models.join(", ").toLowerCase();
      case "startedAt":
        return s.startedAt ? new Date(s.startedAt).getTime() : null;
      case "duration":
        return durationMs(s.startedAt, s.endedAt);
      case "input":
        return s.usage.input;
      case "output":
        return s.usage.output;
      case "cacheCreation":
        return s.usage.cacheCreation;
      case "cacheRead":
        return s.usage.cacheRead;
      case "total":
        return sessionTotal(s);
      case "cost":
        return s.costUsd;
    }
  }

  function setSort(k: SortKey) {
    if (sortKey === k) {
      sortDir = sortDir === "asc" ? "desc" : "asc";
    } else {
      sortKey = k;
      sortDir = k === "source" || k === "project" || k === "models" ? "asc" : "desc";
    }
  }

  const sorted = $derived.by(() => {
    const dir = sortDir === "asc" ? 1 : -1;
    return [...sessions].sort((a, b) => {
      const va = sortValue(a, sortKey);
      const vb = sortValue(b, sortKey);
      if (va == null && vb == null) return 0;
      if (va == null) return 1; // nulls always last
      if (vb == null) return -1;
      if (va < vb) return -dir;
      if (va > vb) return dir;
      return 0;
    });
  });

  const totals = $derived.by(() => {
    const t = { input: 0, output: 0, cacheCreation: 0, cacheRead: 0, cost: 0, unpriced: 0, partial: 0 };
    for (const s of sessions) {
      t.input += s.usage.input;
      t.output += s.usage.output;
      t.cacheCreation += s.usage.cacheCreation;
      t.cacheRead += s.usage.cacheRead;
      t.cost += s.costUsd ?? 0;
      if (s.costUsd == null) t.unpriced++;
      else if (s.unpricedModels.length > 0) t.partial++;
    }
    return t;
  });

  const pageCount = $derived(Math.max(1, Math.ceil(sorted.length / pageSize)));
  $effect(() => {
    if (page > pageCount) page = pageCount;
  });
  const currentPage = $derived(Math.min(page, pageCount));
  const pageSessions = $derived(sorted.slice((currentPage - 1) * pageSize, currentPage * pageSize));
</script>

{#if error}
  <div class="state error">Failed to load sessions: {error}</div>
{:else if loading && totalCount === 0}
  <div class="state">Loading sessions…</div>
{:else if totalCount === 0}
  <div class="state">No sessions found. Add extra directories in Settings if your logs live elsewhere.</div>
{:else}
  <div class="table-wrap">
    <table>
      <thead>
        <tr>
          {#each columns as c (c.key)}
            <th
              class="sortable"
              class:num={c.num}
              aria-sort={sortKey === c.key ? (sortDir === "asc" ? "ascending" : "descending") : "none"}
              onclick={() => setSort(c.key)}
            >
              {c.label}{sortKey === c.key ? (sortDir === "asc" ? " ▲" : " ▼") : ""}
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each pageSessions as s (keyOf(s))}
          {@const open = !!expanded[keyOf(s)]}
          <tr
            class="clickable"
            tabindex="0"
            onclick={() => onopen(s)}
            onkeydown={(e) => e.key === "Enter" && onopen(s)}
          >
            <td class="nowrap">
              {#if s.subagentCount > 0}
                <button
                  class="chevron"
                  aria-label={open ? "Collapse workers" : "Expand workers"}
                  aria-expanded={open}
                  onclick={(e) => {
                    e.stopPropagation();
                    expanded[keyOf(s)] = !open;
                  }}
                  onkeydown={(e) => e.stopPropagation()}>{open ? "▾" : "▸"}</button
                >
              {:else}
                <span class="chevron-gap"></span>
              {/if}
              <span class="badge {s.source}">{s.source}</span>
            </td>
            <td title={s.project ?? ""}>
              {projectName(s.project)}
              {#if s.subagentCount > 0}
                <span class="badge workers" title="{s.subagentCount} subagent transcripts">+{s.subagentCount} workers</span>
              {/if}
              {#if s.logDeleted}
                <span class="badge muted-badge" title={LOG_DELETED_TIP}>log deleted</span>
              {/if}
            </td>
            <td class="ellipsis" title={s.models.join(", ")}>{s.models.join(", ") || "—"}</td>
            <td>{fmtDateTime(s.startedAt)}</td>
            <td class="num">{fmtDuration(durationMs(s.startedAt, s.endedAt))}</td>
            <td class="num" title={fmtInt(s.usage.input)}>{fmtCompact(s.usage.input)}</td>
            <td class="num" title={fmtInt(s.usage.output)}>{fmtCompact(s.usage.output)}</td>
            <td class="num" title={fmtInt(s.usage.cacheCreation)}>{fmtCompact(s.usage.cacheCreation)}</td>
            <td class="num" title={fmtInt(s.usage.cacheRead)}>{fmtCompact(s.usage.cacheRead)}</td>
            <td class="num" title={fmtInt(sessionTotal(s))}>{fmtCompact(sessionTotal(s))}</td>
            <td class="num">
              {#if s.costUsd == null}
                <span title="No price for {s.unpricedModels.join(', ') || s.models.join(', ')}">—</span>
              {:else}
                {fmtUsd(s.costUsd)}
                {#if s.unpricedModels.length > 0}
                  <span class="badge warn" title="No price for {s.unpricedModels.join(', ')}">partial</span>
                {/if}
              {/if}
            </td>
          </tr>
          {#if open}
            {#each s.transcripts as t, ti (ti)}
              <tr class="sub">
                <td colspan="2" class="sub-label">
                  <span class="indent" title={t.filePath}>{trLabel(t)}</span>
                  {#if t.isSubagent && t.agentType}
                    <span class="tag">{t.agentType}</span>
                  {/if}
                  {#if t.logDeleted}
                    <span class="badge muted-badge" title={LOG_DELETED_TIP}>log deleted</span>
                  {/if}
                </td>
                <td class="ellipsis" title={t.models.join(", ")}>{t.models.join(", ") || "—"}</td>
                <td>{fmtDateTime(t.startedAt)}</td>
                <td class="num">{fmtDuration(durationMs(t.startedAt, t.endedAt))}</td>
                <td class="num" title={fmtInt(t.usage.input)}>{fmtCompact(t.usage.input)}</td>
                <td class="num" title={fmtInt(t.usage.output)}>{fmtCompact(t.usage.output)}</td>
                <td class="num" title={fmtInt(t.usage.cacheCreation)}>{fmtCompact(t.usage.cacheCreation)}</td>
                <td class="num" title={fmtInt(t.usage.cacheRead)}>{fmtCompact(t.usage.cacheRead)}</td>
                <td class="num" title={fmtInt(totalTokens(t.usage))}>{fmtCompact(totalTokens(t.usage))}</td>
                <td class="num">
                  {#if t.costUsd == null}
                    <span title="No price for {t.unpricedModels.join(', ') || t.models.join(', ')}">—</span>
                  {:else}
                    {fmtUsd(t.costUsd)}
                    {#if t.unpricedModels.length > 0}
                      <span class="badge warn" title="No price for {t.unpricedModels.join(', ')}">partial</span>
                    {/if}
                  {/if}
                </td>
              </tr>
            {/each}
          {/if}
        {:else}
          <tr><td colspan={columns.length} class="state">No sessions match the filters.</td></tr>
        {/each}
        <tr class="table-spacer" aria-hidden="true"><td colspan={columns.length}></td></tr>
      </tbody>
      <tfoot>
        <tr>
          <td colspan="5">Total ({sessions.length} sessions)</td>
          <td class="num" title={fmtInt(totals.input)}>{fmtCompact(totals.input)}</td>
          <td class="num" title={fmtInt(totals.output)}>{fmtCompact(totals.output)}</td>
          <td class="num" title={fmtInt(totals.cacheCreation)}>{fmtCompact(totals.cacheCreation)}</td>
          <td class="num" title={fmtInt(totals.cacheRead)}>{fmtCompact(totals.cacheRead)}</td>
          <td
            class="num"
            title={fmtInt(totals.input + totals.output + totals.cacheCreation + totals.cacheRead)}
            >{fmtCompact(totals.input + totals.output + totals.cacheCreation + totals.cacheRead)}</td
          >
          <td class="num">
            {fmtUsd(totals.cost)}
            {#if totals.unpriced > 0 || totals.partial > 0}
              <span
                class="badge warn"
                title="Fully unpriced sessions ({totals.unpriced}) are excluded from the total. Partially priced sessions ({totals.partial}) contribute only their priced turns."
                >+{[
                  totals.unpriced > 0 ? `${totals.unpriced} unpriced` : '',
                  totals.partial > 0 ? `${totals.partial} partial` : ''
                ]
                  .filter(Boolean)
                  .join(', ')}</span
              >
            {/if}
          </td>
        </tr>
      </tfoot>
    </table>
  </div>
  <nav class="pagination" aria-label="Session pages">
    <label>Rows per page <select bind:value={pageSize} onchange={() => (page = 1)} aria-label="Rows per page">
      {#each [10, 25, 50, 100] as size}<option value={size}>{size}</option>{/each}
    </select></label>
    <button class="icon" title="Previous page" aria-label="Previous page" disabled={currentPage === 1} onclick={() => page--}>‹</button>
    <span>Page {currentPage} of {pageCount}</span>
    <button class="icon" title="Next page" aria-label="Next page" disabled={currentPage === pageCount} onclick={() => page++}>›</button>
  </nav>
{/if}

<style>
  .table-wrap { flex: 1; min-height: 0; }
  .table-wrap table { height: 100%; }
  .table-spacer { height: 100%; }
  .table-spacer td { border: 0; padding: 0; }
  .pagination {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.75rem;
    padding-top: 0.75rem;
  }
</style>
