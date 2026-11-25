<script lang="ts">
  import "./app.css";
  import { listSessions } from "./lib/api";
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
