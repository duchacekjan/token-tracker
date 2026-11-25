<script lang="ts">
  import "./app.css";
  import { getDbWarning, listSessions } from "./lib/api";
  import type { SessionSummary } from "./lib/types";
  import SessionList from "./lib/components/SessionList.svelte";
  import SessionDetail from "./lib/components/SessionDetail.svelte";
  import SettingsPanel from "./lib/components/SettingsPanel.svelte";

  let sessions = $state<SessionSummary[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let view = $state<"list" | "detail" | "settings">("list");
  let selected = $state<SessionSummary | null>(null);

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
      <SessionList {sessions} {loading} {error} onopen={open} onreload={load} />
    {/if}
  </main>
</div>

<style>
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
