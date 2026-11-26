<script lang="ts">
  import type { SessionFilter } from "../types";

  interface Props {
    filter: SessionFilter;
    matched: number;
    total: number;
    loading: boolean;
    onreload: () => void;
    onexport: () => void;
  }
  let { filter = $bindable(), matched, total, loading, onreload, onexport }: Props = $props();

  // Empty date input yields "" -> store null.
  let from = $state(filter.from ?? "");
  let to = $state(filter.to ?? "");
  $effect(() => {
    from = filter.from ?? "";
    to = filter.to ?? "";
  });
</script>

<div class="toolbar">
  <select bind:value={filter.source} aria-label="Source filter">
    <option value="all">All sources</option>
    <option value="claude">Claude</option>
    <option value="codex">Codex</option>
  </select>
  <input
    class="grow"
    type="search"
    placeholder="Search project, session id, model, worker…"
    aria-label="Search sessions"
    bind:value={filter.search}
  />
  <label class="date">
    From
    <input
      type="date"
      bind:value={from}
      max={filter.to ?? undefined}
      oninput={() => (filter.from = from || null)}
    />
  </label>
  <label class="date">
    To
    <input
      type="date"
      bind:value={to}
      min={filter.from ?? undefined}
      oninput={() => (filter.to = to || null)}
    />
  </label>
  <button
    onclick={() => {
      filter.from = null;
      filter.to = null;
    }}
    disabled={!filter.from && !filter.to}>Clear dates</button
  >
  <span class="count">{matched} of {total} sessions</span>
  <button onclick={onreload} disabled={loading}>{loading ? "Loading…" : "Reload"}</button>
  <button onclick={onexport} disabled={total === 0}>Export CSV</button>
</div>

<style>
  .date {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
  }
</style>
