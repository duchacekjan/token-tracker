<script lang="ts">
  import { getSession } from "../api";
  import type { SessionDetail, SessionSummary, TranscriptSummary } from "../types";
  import {
    durationMs,
    fmtDateTime,
    fmtDuration,
    fmtInt,
    fmtUsd,
    projectName,
    sessionTotal,
    totalTokens,
  } from "../format";

  interface Props {
    session: SessionSummary;
    onback: () => void;
  }
  let { session, onback }: Props = $props();

  let detail = $state<SessionDetail | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  $effect(() => {
    const { source, id } = session;
    loading = true;
    error = null;
    detail = null;
    getSession(source, id)
      .then((d) => {
        if (source === session.source && id === session.id) detail = d;
      })
      .catch((e) => (error = String(e)))
      .finally(() => (loading = false));
  });

  const s = $derived(detail?.summary ?? session);

  const LOG_DELETED_TIP = "Original log file was removed; data kept in local database";
  const trLabel = (t: TranscriptSummary) => (t.isSubagent ? (t.agentLabel ?? t.agentId ?? "worker") : "Main");
  const workerCount = $derived(s.transcripts.filter((t) => t.isSubagent).length);

  let trFilter = $state("all");
  $effect(() => {
    session.id;
    trFilter = "all";
  });
  const visibleTurns = $derived.by(() => {
    const turns = detail?.turns ?? [];
    if (trFilter === "all") return turns.map((t, i) => ({ t, i }));
    const idx = Number(trFilter);
    return turns.map((t, i) => ({ t, i })).filter((x) => x.t.transcript === idx);
  });
</script>

<div class="toolbar">
  <button onclick={onback}>← Back</button>
  <strong>{projectName(s.project)}</strong>
  <span class="badge {s.source}">{s.source}</span>
</div>

<div class="cards">
  <div class="card"><div class="label">Input</div><div class="value">{fmtInt(s.usage.input)}</div></div>
  <div class="card"><div class="label">Output</div><div class="value">{fmtInt(s.usage.output)}</div></div>
  <div class="card"><div class="label">Cache write</div><div class="value">{fmtInt(s.usage.cacheCreation)}</div></div>
  <div class="card"><div class="label">Cache read</div><div class="value">{fmtInt(s.usage.cacheRead)}</div></div>
  <div class="card"><div class="label">Reasoning</div><div class="value">{fmtInt(s.usage.reasoning)}</div></div>
  <div class="card"><div class="label">Total tokens</div><div class="value">{fmtInt(sessionTotal(s))}</div></div>
  <div class="card">
    <div class="label">Cost</div>
    <div class="value" title={s.unpricedModels.length ? `No price for ${s.unpricedModels.join(", ")}` : ""}>
      {fmtUsd(s.costUsd)}
    </div>
  </div>
  <div class="card"><div class="label">Turns</div><div class="value">{fmtInt(s.turnCount)}</div></div>
  <div class="card wide">
    <div class="label">Started</div><div class="value small">{fmtDateTime(s.startedAt)}</div>
  </div>
  <div class="card wide">
    <div class="label">Ended · Duration</div>
    <div class="value small">{fmtDateTime(s.endedAt)} · {fmtDuration(durationMs(s.startedAt, s.endedAt))}</div>
  </div>
  <div class="card full">
    <div class="label">Models</div>
    <div class="value small">
      {s.models.join(", ") || "—"}
      {#if s.unpricedModels.length}
        <span class="badge warn">unpriced: {s.unpricedModels.join(", ")}</span>
      {/if}
    </div>
  </div>
  <div class="card full">
    <div class="label">Session id</div><div class="value small mono">{s.id}</div>
  </div>
  <div class="card full">
    <div class="label">File</div>
    <div class="value small mono">
      {s.transcripts[0]?.filePath ?? s.filePath}
      {#if workerCount > 0}
        <span class="muted">+{workerCount} worker transcript{workerCount === 1 ? "" : "s"}</span>
      {/if}
      {#if s.logDeleted}
        <span class="badge muted-badge" title={LOG_DELETED_TIP}>log deleted</span>
      {/if}
    </div>
  </div>
</div>

{#if s.transcripts.length > 0}
  <h2>Transcripts</h2>
  <div class="table-wrap">
    <table>
      <thead>
        <tr>
          <th>Transcript</th>
          <th>Agent type</th>
          <th>Models</th>
          <th class="num">Turns</th>
          <th class="num">Input</th>
          <th class="num">Output</th>
          <th class="num">Cache write</th>
          <th class="num">Cache read</th>
          <th class="num">Cost USD</th>
          <th class="num">Share</th>
        </tr>
      </thead>
      <tbody>
        {#each s.transcripts as t, ti (ti)}
          <tr>
            <td title={t.filePath}>
              {trLabel(t)}
              {#if t.logDeleted}
                <span class="badge muted-badge" title={LOG_DELETED_TIP}>log deleted</span>
              {/if}
            </td>
            <td>{#if t.isSubagent && t.agentType}<span class="tag nomargin">{t.agentType}</span>{:else}<span class="muted">—</span>{/if}</td>
            <td class="ellipsis" title={t.models.join(", ")}>{t.models.join(", ") || "—"}</td>
            <td class="num">{fmtInt(t.turnCount)}</td>
            <td class="num">{fmtInt(t.usage.input)}</td>
            <td class="num">{fmtInt(t.usage.output)}</td>
            <td class="num">{fmtInt(t.usage.cacheCreation)}</td>
            <td class="num">{fmtInt(t.usage.cacheRead)}</td>
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
            <td class="num">
              {#if t.costUsd != null && s.costUsd}
                {((t.costUsd / s.costUsd) * 100).toFixed(1)}%
              {:else}
                <span class="muted">—</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<div class="section-head">
  <h2>Turns</h2>
  {#if s.transcripts.length > 1}
    <select bind:value={trFilter} aria-label="Transcript filter">
      <option value="all">All transcripts</option>
      {#each s.transcripts as t, ti (ti)}
        <option value={String(ti)}>{trLabel(t)}</option>
      {/each}
    </select>
  {/if}
</div>
{#if error}
  <div class="state error">Failed to load session: {error}</div>
{:else if loading}
  <div class="state">Loading turns…</div>
{:else if !detail || detail.turns.length === 0}
  <div class="state">No turns recorded.</div>
{:else}
  <div class="table-wrap">
    <table>
      <thead>
        <tr>
          <th>#</th>
          <th>Time</th>
          <th>Transcript</th>
          <th>Model</th>
          <th class="num">Input</th>
          <th class="num">Output</th>
          <th class="num">Cache write</th>
          <th class="num">Cache read</th>
          <th class="num">Reasoning</th>
          <th class="num">Cost USD</th>
        </tr>
      </thead>
      <tbody>
        {#each visibleTurns as { t, i } (i)}
          <tr>
            <td class="muted">{i + 1}</td>
            <td>{fmtDateTime(t.timestamp)}</td>
            <td>{s.transcripts[t.transcript] ? trLabel(s.transcripts[t.transcript]) : "—"}</td>
            <td>{t.model}</td>
            <td class="num">{fmtInt(t.usage.input)}</td>
            <td class="num">{fmtInt(t.usage.output)}</td>
            <td class="num">{fmtInt(t.usage.cacheCreation)}</td>
            <td class="num">{fmtInt(t.usage.cacheRead)}</td>
            <td class="num">{fmtInt(t.usage.reasoning)}</td>
            <td class="num">
              {#if t.costUsd == null}
                <span title="No price for {t.model}">—</span>
              {:else}
                {fmtUsd(t.costUsd)}
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}
