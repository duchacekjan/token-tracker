<script lang="ts">
  import type { SessionSummary } from "../types";
  import {
    durationMs,
    fmtCompact,
    fmtDateTime,
    fmtDuration,
    fmtInt,
    fmtUsd,
    projectName,
    sessionTotal,
  } from "../format";

  interface Props {
    sessions: SessionSummary[];
    loading: boolean;
    error: string | null;
    onopen: (s: SessionSummary) => void;
    onreload: () => void;
  }
  let { sessions, loading, error, onopen, onreload }: Props = $props();

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

  let sourceFilter = $state<"all" | "claude" | "codex">("all");
  let search = $state("");
  let sortKey = $state<SortKey>("startedAt");
  let sortDir = $state<"asc" | "desc">("desc");

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

  const filtered = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return sessions.filter((s) => {
      if (sourceFilter !== "all" && s.source !== sourceFilter) return false;
      if (!q) return true;
      return (
        (s.project ?? "").toLowerCase().includes(q) ||
        s.id.toLowerCase().includes(q) ||
        s.models.some((m) => m.toLowerCase().includes(q))
      );
    });
  });

  const sorted = $derived.by(() => {
    const dir = sortDir === "asc" ? 1 : -1;
    return [...filtered].sort((a, b) => {
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
    for (const s of filtered) {
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
</script>

<div class="toolbar">
  <select bind:value={sourceFilter} aria-label="Source filter">
    <option value="all">All sources</option>
    <option value="claude">Claude</option>
    <option value="codex">Codex</option>
  </select>
  <input
    class="grow"
    type="search"
    placeholder="Search project, session id, model…"
    bind:value={search}
  />
  <span class="count">{filtered.length} of {sessions.length} sessions</span>
  <button onclick={onreload} disabled={loading}>{loading ? "Loading…" : "Reload"}</button>
</div>

{#if error}
  <div class="state error">Failed to load sessions: {error}</div>
{:else if loading && sessions.length === 0}
  <div class="state">Loading sessions…</div>
{:else if sessions.length === 0}
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
        {#each sorted as s (`${s.source}:${s.id}`)}
          <tr
            class="clickable"
            tabindex="0"
            onclick={() => onopen(s)}
            onkeydown={(e) => e.key === "Enter" && onopen(s)}
          >
            <td><span class="badge {s.source}">{s.source}</span></td>
            <td title={s.project ?? ""}>{projectName(s.project)}</td>
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
        {:else}
          <tr><td colspan={columns.length} class="state">No sessions match the filters.</td></tr>
        {/each}
      </tbody>
      <tfoot>
        <tr>
          <td colspan="5">Total ({filtered.length} sessions)</td>
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
{/if}
