<script lang="ts">
  import { onMount } from "svelte";
  import type { SessionDetail, SessionSummary } from "../types";
  import { keyOf } from "../filters";
  import { getSessionsDetail } from "../api";
  import { fmtCompact, fmtInt, fmtUsd } from "../format";
  import {
    breakdown,
    stackedBy,
    timeSeries,
    tokenTypeSeries,
    type Bucket,
    type Dim,
    type Metric,
  } from "../charts/data";
  import { colorFor, onThemeChange, readTheme } from "../charts/theme";
  import type { EChartsOption } from "../charts/echarts";
  import EChart from "./EChart.svelte";

  interface Props {
    sessions: SessionSummary[];
  }
  let { sessions }: Props = $props();

  let bucket = $state<Bucket>("day");
  let metric = $state<Metric>("tokens");
  let pieDim = $state<Dim>("source");
  let stackDim = $state<"source" | "model">("source");

  let themeTick = $state(0);
  onMount(() => onThemeChange(() => themeTick++));

  // ---- model detail cache ----
  const cache = new Map<string, SessionDetail>();
  let cacheVer = $state(0);
  let loading = $state(false);
  let loadError = $state<string | null>(null);

  const needModels = $derived(pieDim === "model" || stackDim === "model");

  $effect(() => {
    if (!needModels) return;
    const missing = sessions.filter((s) => !cache.has(keyOf(s)));
    if (missing.length === 0) return;
    let cancelled = false;
    loading = true;
    loadError = null;
    (async () => {
      try {
        const CHUNK = 100;
        for (let i = 0; i < missing.length; i += CHUNK) {
          const keys = missing.slice(i, i + CHUNK).map((s) => ({ source: s.source, id: s.id }));
          const res = await getSessionsDetail(keys);
          for (const d of res) cache.set(keyOf(d.summary), d);
          if (cancelled) return;
          cacheVer++;
        }
      } catch (e) {
        if (!cancelled) loadError = String(e);
      } finally {
        if (!cancelled) loading = false;
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  const modelsReady = $derived(!needModels || (!loading && loadError == null));

  // ---- formatting ----
  const fmtVal = (m: Metric) => (m === "cost" ? (v: number) => fmtUsd(v) : (v: number) => fmtInt(v));
  const fmtAxis = (m: Metric) => (m === "cost" ? (v: number) => fmtUsd(v) : (v: number) => fmtCompact(v));

  // ---- data ----
  const ts = $derived(timeSeries(sessions, bucket));
  const tt = $derived(tokenTypeSeries(sessions, bucket));
  const stacked = $derived.by(() => {
    cacheVer;
    return stackedBy(sessions, bucket, stackDim, metric, cache);
  });
  const pie = $derived.by(() => {
    cacheVer;
    return breakdown(sessions, pieDim, metric, cache);
  });
  const skipped = $derived(ts.skipped);

  // ---- options ----
  function baseAxes(buckets: string[], th: ReturnType<typeof readTheme>) {
    return {
      grid: { left: 8, right: 12, top: 36, bottom: 8, containLabel: true },
      xAxis: {
        type: "category" as const,
        data: buckets,
        axisLine: { lineStyle: { color: th.border } },
        axisLabel: { color: th.muted },
      },
      legend: { top: 0, textStyle: { color: th.muted }, type: "scroll" as const },
    };
  }
  const tooltipBase = (th: ReturnType<typeof readTheme>) => ({
    backgroundColor: th.surface,
    borderColor: th.border,
    textStyle: { color: th.text },
  });
  const yAxis = (th: ReturnType<typeof readTheme>, m: Metric, opts: object = {}) => ({
    type: "value" as const,
    axisLabel: { color: th.muted, formatter: fmtAxis(m) },
    splitLine: { lineStyle: { color: th.border } },
    ...opts,
  });

  const optA = $derived.by((): EChartsOption => {
    themeTick;
    const th = readTheme();
    const other: Metric = metric === "tokens" ? "cost" : "tokens";
    const label = (m: Metric) => (m === "tokens" ? "Tokens" : "Cost (USD)");
    const vals = (m: Metric) => (m === "tokens" ? ts.tokens : ts.cost);
    return {
      ...baseAxes(ts.buckets, th),
      color: [th.accent, th.palette[2]],
      tooltip: { ...tooltipBase(th), trigger: "axis" },
      yAxis: [
        yAxis(th, metric),
        yAxis(th, other, { splitLine: { show: false } }),
      ],
      series: [
        {
          name: label(metric),
          type: "bar",
          data: vals(metric),
          tooltip: { valueFormatter: fmtVal(metric) },
        },
        {
          name: label(other),
          type: "line",
          yAxisIndex: 1,
          data: vals(other),
          showSymbol: ts.buckets.length < 60,
          tooltip: { valueFormatter: fmtVal(other) },
        },
      ],
    };
  });

  const optB = $derived.by((): EChartsOption => {
    themeTick;
    const th = readTheme();
    const total = pie.reduce((a, p) => a + p.value, 0);
    return {
      tooltip: {
        ...tooltipBase(th),
        trigger: "item",
        formatter: (p: any) =>
          `${p.marker}${p.name}<br/><b>${fmtVal(metric)(p.value)}</b> (${total ? ((p.value / total) * 100).toFixed(1) : "0"}%)`,
      },
      legend: { type: "scroll", orient: "vertical", right: 0, top: 8, textStyle: { color: th.muted } },
      series: [
        {
          type: "pie",
          radius: ["40%", "70%"],
          center: ["38%", "50%"],
          label: { show: false },
          itemStyle: { borderColor: th.surface, borderWidth: 2 },
          data: pie.map((p, i) => ({
            name: p.name,
            value: p.value,
            itemStyle: { color: colorFor(th, p.name, i) },
          })),
        },
      ],
    };
  });

  const optC = $derived.by((): EChartsOption => {
    themeTick;
    const th = readTheme();
    return {
      ...baseAxes(tt.buckets, th),
      color: [th.palette[0], th.palette[1], th.palette[2], th.palette[4]],
      tooltip: { ...tooltipBase(th), trigger: "axis", valueFormatter: (v: any) => fmtInt(Number(v)) },
      yAxis: yAxis(th, "tokens"),
      series: tt.series.map((s) => ({ name: s.name, type: "bar", stack: "t", data: s.data })),
    };
  });

  const optD = $derived.by((): EChartsOption => {
    themeTick;
    const th = readTheme();
    return {
      ...baseAxes(stacked.buckets, th),
      tooltip: { ...tooltipBase(th), trigger: "axis", valueFormatter: (v: any) => fmtVal(metric)(Number(v)) },
      yAxis: yAxis(th, metric),
      series: stacked.series.map((s, i) => ({
        name: s.name,
        type: "bar",
        stack: "s",
        data: s.data,
        itemStyle: { color: colorFor(th, s.name, i) },
      })),
    };
  });
</script>

<div class="charts">
  <div class="controls">
    <label>
      Bucket
      <select bind:value={bucket}>
        <option value="day">Day</option>
        <option value="week">Week</option>
        <option value="month">Month</option>
      </select>
    </label>
    <label>
      Metric
      <select bind:value={metric}>
        <option value="tokens">Tokens</option>
        <option value="cost">USD</option>
      </select>
    </label>
    <label>
      Pie by
      <select bind:value={pieDim}>
        <option value="source">Harness</option>
        <option value="model">Model</option>
        <option value="project">Project</option>
      </select>
    </label>
    <label>
      Stacked by
      <select bind:value={stackDim}>
        <option value="source">Harness</option>
        <option value="model">Model</option>
      </select>
    </label>
    {#if skipped > 0}
      <span class="note">{skipped} sessions without start time not shown</span>
    {/if}
  </div>

  {#if sessions.length === 0}
    <div class="state">No sessions match the filters.</div>
  {:else}
    <div class="grid">
      <section class="card">
        <h3>{metric === "tokens" ? "Tokens" : "Cost"} over time</h3>
        <EChart option={optA} />
      </section>
      <section class="card">
        <h3>{metric === "tokens" ? "Tokens" : "Cost"} by {pieDim === "source" ? "harness" : pieDim}</h3>
        {#if pieDim === "model" && !modelsReady}
          <div class="state">{loadError ? `Failed to load turns: ${loadError}` : "Loading turns…"}</div>
        {:else}
          <EChart option={optB} />
        {/if}
      </section>
      <section class="card">
        <h3>Tokens by type</h3>
        <EChart option={optC} />
      </section>
      <section class="card">
        <h3>{metric === "tokens" ? "Tokens" : "Cost"} by {stackDim === "source" ? "harness" : "model"} over time</h3>
        {#if stackDim === "model" && !modelsReady}
          <div class="state">{loadError ? `Failed to load turns: ${loadError}` : "Loading turns…"}</div>
        {:else}
          <EChart option={optD} />
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .charts {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px;
  }
  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
  }
  .controls label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
  }
  .note {
    color: var(--warn);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }
  @media (max-width: 900px) {
    .grid {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px 12px;
    min-width: 0;
  }
  .card h3 {
    margin: 0 0 6px;
    font-size: 13px;
    font-weight: 600;
  }
  .state {
    padding: 24px;
    text-align: center;
    color: var(--muted);
  }
</style>
