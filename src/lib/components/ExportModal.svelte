<script lang="ts">
  import { onMount } from "svelte";
  import type { SessionDetail, SessionFilter, SessionSummary, Source } from "../types";
  import { getSessionsDetail, saveCsv } from "../api";
  import { keyOf } from "../filters";
  import { toCsv } from "../export/csv";
  import { fieldsFor, type Level } from "../export/fields";
  import {
    buildHeader,
    buildRows,
    defaultFileName,
    estimateCount,
    selectSessions,
    type ExportOptions,
  } from "../export/rows";
  import { fmtInt } from "../format";

  interface Props {
    sessions: SessionSummary[];
    initial: SessionFilter;
    onclose: () => void;
  }
  let { sessions, initial, onclose }: Props = $props();

  let dialog: HTMLDialogElement;

  const init = (() => initial)();
  let claude = $state(init.source === "all" || init.source === "claude");
  let codex = $state(init.source === "all" || init.source === "codex");
  let from = $state(init.from ?? "");
  let to = $state(init.to ?? "");
  let search = $state(init.search);

  const allModels = $derived([...new Set(sessions.flatMap((s) => s.models))].sort());
  const allProjects = $derived([...new Set(sessions.map((s) => s.project ?? ""))].sort());
  let modelSel = $state<string[] | null>(null); // null = all, [] = none
  let projectSel = $state<string[] | null>(null);

  let includeWorkers = $state(true);
  let onlyPriced = $state(false);
  let minCost = $state("");

  let level = $state<Level>("session");
  const defaultsFor = (l: Level) => fieldsFor(l).filter((f) => f.default).map((f) => f.id);
  let fields = $state<string[]>(defaultsFor("session"));
  const fieldDefs = $derived(fieldsFor(level));

  let delimiter = $state<"," | ";">(",");
  let decimalTouched = $state(false);
  let bomTouched = $state(false);
  let decimalPick = $state<"." | ",">(".");
  let bomPick = $state(false);
  const decimal = $derived(decimalTouched ? decimalPick : delimiter === "," ? "." : ",");
  const bom = $derived(bomTouched ? bomPick : delimiter === ";");
  const decimalClash = $derived(decimal === delimiter);

  let busy = $state(false);
  let progress = $state("");
  let error = $state("");

  const minCostNum = $derived.by(() => {
    const t = minCost.trim().replace(",", ".");
    if (!t) return null;
    const n = Number(t);
    return Number.isFinite(n) ? n : null;
  });
  const minCostBad = $derived(minCost.trim() !== "" && minCostNum == null);

  const opts = $derived<ExportOptions>({
    sources: [...(claude ? ["claude"] : []), ...(codex ? ["codex"] : [])] as Source[],
    from: from || null,
    to: to || null,
    search,
    models: modelSel,
    projects: projectSel,
    includeWorkers,
    onlyPriced,
    minCostUsd: minCostNum,
    level,
    fields,
    delimiter,
    decimal,
    bom,
  });
  const selected = $derived(selectSessions(sessions, opts));
  const count = $derived(estimateCount(selected, opts));
  const canExport = $derived(!busy && count > 0 && fields.length > 0 && !minCostBad);

  onMount(() => {
    dialog.showModal();
  });

  function setLevel(l: Level) {
    level = l;
    fields = defaultsFor(l);
  }

  function toggle(list: string[], v: string): string[] {
    return list.includes(v) ? list.filter((x) => x !== v) : [...list, v];
  }
  // Model/project selection: null = all, [] = none.
  const isOn = (list: string[] | null, v: string) => list === null || list.includes(v);
  function toggleFilter(list: string[] | null, all: string[], v: string): string[] | null {
    const next = toggle(list ?? all, v).filter((x) => all.includes(x));
    return next.length === all.length ? null : next;
  }

  const projectLabel = (p: string) => (p === "" ? "(unknown)" : p);

  async function doExport() {
    if (!canExport) return;
    busy = true;
    error = "";
    progress = "";
    try {
      let details: Map<string, SessionDetail> | undefined;
      if (level === "turn") {
        const keys = selected.map((s) => ({ source: s.source, id: s.id }));
        const CHUNK = 100;
        const total = keys.length;
        details = new Map();
        for (let i = 0; i < total; i += CHUNK) {
          progress = `Loading turns ${fmtInt(Math.min(i + CHUNK, total))}/${fmtInt(total)}…`;
          const list = await getSessionsDetail(keys.slice(i, i + CHUNK));
          for (const d of list) details.set(keyOf(d.summary), d);
        }
        progress = "Building CSV…";
      }
      const csv = toCsv(buildHeader(opts), buildRows(selected, opts, details), opts);
      const saved = await saveCsv(defaultFileName(level), csv);
      if (saved !== null) onclose();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
      progress = "";
    }
  }

  function onBackdrop(e: MouseEvent) {
    if (e.target === dialog) onclose();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog bind:this={dialog} class="export" onclose={onclose} onclick={onBackdrop}>
  <form method="dialog" onsubmit={(e) => e.preventDefault()}>
    <h2>Export CSV</h2>

    <div class="body">
      <fieldset>
        <legend>Sessions</legend>
        <div class="row">
          <label><input type="checkbox" bind:checked={claude} /> Claude</label>
          <label><input type="checkbox" bind:checked={codex} /> Codex</label>
          <label>From <input type="date" bind:value={from} /></label>
          <label>To <input type="date" bind:value={to} /></label>
        </div>
        <div class="row">
          <label class="grow">Search <input type="search" bind:value={search} placeholder="project, id, model, worker" /></label>
        </div>
        <div class="row">
          <label><input type="checkbox" bind:checked={includeWorkers} /> Include workers</label>
          <label><input type="checkbox" bind:checked={onlyPriced} /> Only priced</label>
          <label>Min cost USD <input type="text" inputmode="decimal" class="narrow" bind:value={minCost} placeholder="none" aria-invalid={minCostBad} /></label>
        </div>
      </fieldset>

      <div class="two">
        <fieldset>
          <legend>
            Models
            <button type="button" class="link" onclick={() => (modelSel = null)}>All</button>
            <button type="button" class="link" onclick={() => (modelSel = [])}>None</button>
          </legend>
          <div class="list">
            {#each allModels as m (m)}
              <label><input type="checkbox" checked={isOn(modelSel, m)} onchange={() => (modelSel = toggleFilter(modelSel, allModels, m))} /> {m}</label>
            {:else}
              <span class="muted">none</span>
            {/each}
          </div>
        </fieldset>
        <fieldset>
          <legend>
            Projects
            <button type="button" class="link" onclick={() => (projectSel = null)}>All</button>
            <button type="button" class="link" onclick={() => (projectSel = [])}>None</button>
          </legend>
          <div class="list">
            {#each allProjects as p (p)}
              <label><input type="checkbox" checked={isOn(projectSel, p)} onchange={() => (projectSel = toggleFilter(projectSel, allProjects, p))} /> {projectLabel(p)}</label>
            {:else}
              <span class="muted">none</span>
            {/each}
          </div>
        </fieldset>
      </div>

      <fieldset>
        <legend>Level</legend>
        <div class="row">
          {#each ["session", "transcript", "turn"] as l (l)}
            <label><input type="radio" name="level" value={l} checked={level === l} onchange={() => setLevel(l as Level)} /> {l}</label>
          {/each}
        </div>
      </fieldset>

      <fieldset>
        <legend>
          Fields
          <button type="button" class="link" onclick={() => (fields = defaultsFor(level))}>Defaults</button>
          <button type="button" class="link" onclick={() => (fields = fieldDefs.map((f) => f.id))}>All</button>
          <button type="button" class="link" onclick={() => (fields = [])}>None</button>
        </legend>
        <div class="fields">
          {#each fieldDefs as f (f.id)}
            <label><input type="checkbox" checked={fields.includes(f.id)} onchange={() => (fields = toggle(fields, f.id))} /> {f.header}</label>
          {/each}
        </div>
      </fieldset>

      <fieldset>
        <legend>Format</legend>
        <div class="row">
          <span>Delimiter</span>
          <label><input type="radio" name="delim" checked={delimiter === ","} onchange={() => (delimiter = ",")} /> ,</label>
          <label><input type="radio" name="delim" checked={delimiter === ";"} onchange={() => (delimiter = ";")} /> ;</label>
          <span class="sep">Decimal</span>
          <label><input type="radio" name="dec" checked={decimal === "."} onchange={() => { decimalTouched = true; decimalPick = "."; }} /> .</label>
          <label><input type="radio" name="dec" checked={decimal === ","} onchange={() => { decimalTouched = true; decimalPick = ","; }} /> ,</label>
          <label class="sep"><input type="checkbox" checked={bom} onchange={(e) => { bomTouched = true; bomPick = e.currentTarget.checked; }} /> UTF-8 BOM</label>
        </div>
        {#if decimalClash}
          <div class="warn">Decimal separator equals delimiter: numbers with decimals will be quoted.</div>
        {/if}
      </fieldset>
    </div>

    <div class="foot">
      <span class="status">
        {#if error}<span class="err">{error}</span>
        {:else if progress}{progress}
        {:else if level === "turn"}~{fmtInt(count)} turns
        {:else}{fmtInt(count)} {level === "session" ? "sessions" : "transcripts"}{/if}
      </span>
      <button type="button" onclick={onclose}>Cancel</button>
      <button type="button" class="primary" disabled={!canExport} onclick={doExport}>{busy ? "Exporting…" : "Export"}</button>
    </div>
  </form>
</dialog>

<style>
  dialog.export {
    width: min(760px, 94vw);
    max-height: 90vh;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    color: var(--text);
  }
  dialog.export::backdrop {
    background: rgba(0, 0, 0, 0.45);
  }
  form {
    display: flex;
    flex-direction: column;
    max-height: 90vh;
  }
  h2 {
    margin: 0;
    padding: 14px 18px;
    font-size: 16px;
    border-bottom: 1px solid var(--border);
  }
  .body {
    padding: 12px 18px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  fieldset {
    border: 1px solid var(--border);
    border-radius: 8px;
    margin: 0;
    padding: 8px 12px 10px;
    min-width: 0;
  }
  legend {
    padding: 0 6px;
    color: var(--muted);
    font-size: 12px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 16px;
    margin-top: 4px;
  }
  .row label,
  .list label,
  .fields label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .grow {
    flex: 1;
  }
  .grow input {
    flex: 1;
  }
  .narrow {
    width: 90px;
  }
  .sep {
    margin-left: 12px;
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 130px;
    overflow-y: auto;
    font-size: 13px;
  }
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: 2px 12px;
    font-size: 13px;
  }
  button.link {
    border: none;
    background: none;
    color: var(--accent);
    padding: 0 4px;
    font-size: 12px;
    cursor: pointer;
  }
  .muted {
    color: var(--muted);
  }
  .warn {
    margin-top: 6px;
    padding: 4px 8px;
    border-radius: 6px;
    background: var(--warn-bg);
    color: var(--warn);
    font-size: 12px;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 18px;
    border-top: 1px solid var(--border);
  }
  .status {
    flex: 1;
    color: var(--muted);
    font-size: 13px;
  }
  .err {
    color: var(--danger);
  }
  input[aria-invalid="true"] {
    border-color: var(--danger);
  }
</style>
