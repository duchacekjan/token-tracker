<script lang="ts">
  import "./app.css";
  import { getDbWarning, listSessions } from "./lib/api";
  import type { SessionFilter, SessionSummary } from "./lib/types";
  import { applyFilter, defaultFilter } from "./lib/filters";
  import FilterBar from "./lib/components/FilterBar.svelte";
  import ExportModal from "./lib/components/ExportModal.svelte";
  import SessionList from "./lib/components/SessionList.svelte";
  import SessionDetail from "./lib/components/SessionDetail.svelte";
  import SettingsPanel from "./lib/components/SettingsPanel.svelte";

  let sessions = $state<SessionSummary[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let view = $state<"list" | "detail" | "settings">("list");
  let selected = $state<SessionSummary | null>(null);
  let filter = $state<SessionFilter>(defaultFilter());
  let tab = $state<"table" | "charts">("table");
  let exportOpen = $state(false);
  const filtered = $derived(applyFilter(sessions, filter));

  async function load() {
    loading = true;
    error = null;
    try {
      sessions = await listSessions();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }
  load();

  let dbWarning = $state<string | null>(null);
  getDbWarning()
    .then((w) => (dbWarning = w))
    .catch(() => {});

  function open(s: SessionSummary) {
    selected = s;
    view = "detail";
  }
</script>

<div class="app">
  <header class="topbar">
    <h1>Token Tracker</h1>
    <button
      class="icon"
      title="Settings"
      aria-label="Settings"
      onclick={() => (view = view === "settings" ? "list" : "settings")}>⚙</button
    >
  </header>
  {#if dbWarning}
    <div class="db-warning" role="alert">
      <span>{dbWarning}</span>
      <button class="icon" title="Dismiss" aria-label="Dismiss" onclick={() => (dbWarning = null)}
        >✕</button
      >
    </div>
  {/if}
  <main class="content">
    {#if view === "settings"}
      <SettingsPanel onback={() => (view = "list")} onpricing={load} />
    {:else if view === "detail" && selected}
      <SessionDetail session={selected} onback={() => (view = "list")} />
    {:else}
      <FilterBar
        bind:filter
        matched={filtered.length}
        total={sessions.length}
        {loading}
        onreload={load}
        onexport={() => (exportOpen = true)}
      />
      <div class="tabs" role="tablist" aria-label="View">
        <button
          role="tab"
          id="tab-table"
          aria-selected={tab === "table"}
          aria-controls="panel-main"
          class:active={tab === "table"}
          onclick={() => (tab = "table")}>Table</button
        >
        <button
          role="tab"
          id="tab-charts"
          aria-selected={tab === "charts"}
          aria-controls="panel-main"
          class:active={tab === "charts"}
          onclick={() => (tab = "charts")}>Charts</button
        >
      </div>
      <div role="tabpanel" id="panel-main" aria-labelledby={tab === "table" ? "tab-table" : "tab-charts"}>
        {#if tab === "table"}
          <SessionList
            sessions={filtered}
            totalCount={sessions.length}
            {loading}
            {error}
            onopen={open}
          />
        {:else}
          {#await import("./lib/components/ChartsView.svelte") then m}
            <m.default sessions={filtered} />
          {:catch e}
            <div class="state error">Failed to load charts: {String(e)}</div>
          {/await}
        {/if}
      </div>
    {/if}
  </main>
  {#if exportOpen}
    <ExportModal {sessions} initial={filter} onclose={() => (exportOpen = false)} />
  {/if}
</div>

<style>
  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 10px;
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    background: transparent;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    color: var(--muted);
    padding: 6px 12px;
  }
  .tabs button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .db-warning {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    padding: 0.5rem 1rem;
    background: var(--warn-bg);
    color: var(--warn);
    border-bottom: 1px solid var(--border);
    font-size: 0.9em;
  }
  .db-warning span {
    flex: 1;
    overflow-wrap: anywhere;
  }
</style>
