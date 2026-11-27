<script lang="ts">
  import { getSettings, refreshPricing, saveSettings } from "../api";
  import type { Settings, SettingsView } from "../types";
  import { fmtDateTime } from "../format";

  interface Props {
    onback: () => void;
    onpricing: () => void;
  }
  let { onback, onpricing }: Props = $props();

  let view = $state<SettingsView | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let saving = $state(false);
  let refreshing = $state(false);
  let newClaude = $state("");
  let newCodex = $state("");
  let newGemini = $state("");
  let newAntigravity = $state("");

  async function load() {
    loading = true;
    error = null;
    try {
      view = await getSettings();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }
  load();

  async function save(next: Settings) {
    saving = true;
    error = null;
    try {
      view = await saveSettings(next);
      onpricing(); // dirs changed -> sessions may change
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function add(kind: "claude" | "codex" | "gemini" | "antigravity") {
    if (!view) return;
    const s = structuredClone($state.snapshot(view.settings));
    const list = kind === "claude" ? s.extraClaudeDirs : kind === "codex" ? s.extraCodexDirs : kind === "gemini" ? s.extraGeminiDirs : s.extraAntigravityDirs;
    const v = (kind === "claude" ? newClaude : kind === "codex" ? newCodex : kind === "gemini" ? newGemini : newAntigravity).trim();
    if (!v || list.includes(v)) return;
    list.push(v);
    if (kind === "claude") newClaude = "";
    else if (kind === "codex") newCodex = "";
    else if (kind === "gemini") newGemini = "";
    else newAntigravity = "";
    save(s);
  }

  function remove(kind: "claude" | "codex" | "gemini" | "antigravity", i: number) {
    if (!view) return;
    const s = structuredClone($state.snapshot(view.settings));
    (kind === "claude" ? s.extraClaudeDirs : kind === "codex" ? s.extraCodexDirs : kind === "gemini" ? s.extraGeminiDirs : s.extraAntigravityDirs).splice(i, 1);
    save(s);
  }

  async function refresh() {
    refreshing = true;
    error = null;
    try {
      const p = await refreshPricing();
      if (view) view.pricing = p;
      onpricing();
    } catch (e) {
      error = String(e);
    } finally {
      refreshing = false;
    }
  }
</script>

<div class="toolbar">
  <button onclick={onback}>← Back</button>
  <strong>Settings</strong>
</div>

{#if error}
  <div class="banner error">{error}</div>
{/if}

{#if loading}
  <div class="state">Loading settings…</div>
{:else if view}
  {#each [{ kind: "claude" as const, title: "Claude Code", dirs: view.claudeDirs, extra: view.settings.extraClaudeDirs }, { kind: "codex" as const, title: "Codex CLI", dirs: view.codexDirs, extra: view.settings.extraCodexDirs }, { kind: "gemini" as const, title: "Gemini CLI", dirs: view.geminiDirs, extra: view.settings.extraGeminiDirs }, { kind: "antigravity" as const, title: "Antigravity", dirs: view.antigravityDirs, extra: view.settings.extraAntigravityDirs }] as g (g.kind)}
    <h2>{g.title} directories</h2>
    <div class="section">
      <p class="muted">Effective directories (read-only)</p>
      <ul class="dirlist">
        {#each g.dirs as d (d)}
          <li><span class="path mono">{d}</span></li>
        {:else}
          <li class="muted">None found</li>
        {/each}
      </ul>
      <p class="muted">Extra directories</p>
      <p class="muted">Folder containing .jsonl session files (scanned recursively).</p>
      <ul class="dirlist">
        {#each g.extra as d, i (d)}
          <li>
            <span class="path mono">{d}</span>
            <button disabled={saving} onclick={() => remove(g.kind, i)}>Remove</button>
          </li>
        {:else}
          <li class="muted">No extra directories</li>
        {/each}
      </ul>
      <form
        class="addrow"
        onsubmit={(e) => {
          e.preventDefault();
          add(g.kind);
        }}
      >
        {#if g.kind === "claude"}
          <input type="text" placeholder="/path/to/.claude/projects" bind:value={newClaude} />
          <button class="primary" disabled={saving || !newClaude.trim()}>Add</button>
        {:else if g.kind === "codex"}
          <input type="text" placeholder="/path/to/.codex/sessions" bind:value={newCodex} />
          <button class="primary" disabled={saving || !newCodex.trim()}>Add</button>
        {:else if g.kind === "gemini"}
          <input type="text" placeholder="/path/to/.gemini/tmp" bind:value={newGemini} />
          <button class="primary" disabled={saving || !newGemini.trim()}>Add</button>
        {:else}
          <input type="text" placeholder="/path/to/.gemini/antigravity" bind:value={newAntigravity} />
          <button class="primary" disabled={saving || !newAntigravity.trim()}>Add</button>
        {/if}
      </form>
    </div>
  {/each}

  <h2>Pricing</h2>
  <div class="section">
    <dl class="kv">
      <dt>Source</dt>
      <dd><span class="badge {view.pricing.source === 'online' ? 'codex' : 'warn'}">{view.pricing.source}</span></dd>
      <dt>Updated</dt>
      <dd>{fmtDateTime(view.pricing.updatedAt)}</dd>
      <dt>Models</dt>
      <dd>{view.pricing.modelCount}</dd>
      {#if view.pricing.error}
        <dt>Error</dt>
        <dd style="color: var(--danger)">{view.pricing.error}</dd>
      {/if}
    </dl>
    <button class="primary" onclick={refresh} disabled={refreshing}>
      {refreshing ? "Refreshing…" : "Refresh prices"}
    </button>
  </div>
{/if}
