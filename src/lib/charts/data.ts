import type { SessionDetail, SessionSummary, Source } from "../types";
import { keyOf } from "../filters";
import { projectName, totalTokens } from "../format";

export type Bucket = "day" | "week" | "month";
export type Metric = "tokens" | "cost";
export type Dim = "source" | "model" | "project";

export const TOP_N = 10;
export const OTHER = "Other";

export const SOURCE_LABEL: Record<Source, string> = { claude: "Claude Code", codex: "Codex", gemini: "Gemini", antigravity: "Antigravity" };

const p2 = (n: number) => String(n).padStart(2, "0");
const ymd = (d: Date) => `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}`;

/** Bucket key in local time; null for unparsable input. */
export function bucketKey(iso: string, b: Bucket): string | null {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return null;
  if (b === "month") return `${d.getFullYear()}-${p2(d.getMonth() + 1)}`;
  if (b === "week") {
    const back = (d.getDay() + 6) % 7; // Monday = 0
    return ymd(new Date(d.getFullYear(), d.getMonth(), d.getDate() - back));
  }
  return ymd(d);
}

function parseKey(key: string): Date {
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y, m - 1, d ?? 1);
}

/** Continuous list of bucket keys from min to max inclusive (keys from bucketKey). */
export function bucketRange(min: string, max: string, b: Bucket): string[] {
  const out: string[] = [];
  if (min > max) return out;
  const start = parseKey(min);
  let i = 0;
  for (;;) {
    const d =
      b === "month"
        ? new Date(start.getFullYear(), start.getMonth() + i, 1)
        : new Date(start.getFullYear(), start.getMonth(), start.getDate() + i * (b === "week" ? 7 : 1));
    const k = b === "month" ? `${d.getFullYear()}-${p2(d.getMonth() + 1)}` : ymd(d);
    if (k > max) break;
    out.push(k);
    i++;
  }
  return out;
}

interface Keyed {
  key: string;
  s: SessionSummary;
}

function keyedSessions(sessions: SessionSummary[], b: Bucket): { items: Keyed[]; skipped: number } {
  const items: Keyed[] = [];
  let skipped = 0;
  for (const s of sessions) {
    const key = s.startedAt ? bucketKey(s.startedAt, b) : null;
    if (key == null) skipped++;
    else items.push({ key, s });
  }
  return { items, skipped };
}

function bucketsOf(items: Keyed[], b: Bucket): string[] {
  if (items.length === 0) return [];
  let min = items[0].key;
  let max = min;
  for (const { key } of items) {
    if (key < min) min = key;
    if (key > max) max = key;
  }
  return bucketRange(min, max, b);
}

export interface TimeSeries {
  buckets: string[];
  tokens: number[];
  cost: number[];
  skipped: number;
}

export function timeSeries(sessions: SessionSummary[], b: Bucket): TimeSeries {
  const { items, skipped } = keyedSessions(sessions, b);
  const buckets = bucketsOf(items, b);
  const idx = new Map(buckets.map((k, i) => [k, i]));
  const tokens = buckets.map(() => 0);
  const cost = buckets.map(() => 0);
  for (const { key, s } of items) {
    const i = idx.get(key)!;
    tokens[i] += totalTokens(s.usage);
    if (s.costUsd != null) cost[i] += s.costUsd;
  }
  return { buckets, tokens, cost, skipped };
}

export interface NamedSeries {
  name: string;
  data: number[];
}

export interface TokenTypeSeries {
  buckets: string[];
  series: NamedSeries[];
  skipped: number;
}

export function tokenTypeSeries(sessions: SessionSummary[], b: Bucket): TokenTypeSeries {
  const { items, skipped } = keyedSessions(sessions, b);
  const buckets = bucketsOf(items, b);
  const idx = new Map(buckets.map((k, i) => [k, i]));
  const mk = (name: string): NamedSeries => ({ name, data: buckets.map(() => 0) });
  const input = mk("Input");
  const output = mk("Output");
  const cacheCreation = mk("Cache creation");
  const cacheRead = mk("Cache read");
  for (const { key, s } of items) {
    const i = idx.get(key)!;
    input.data[i] += s.usage.input;
    output.data[i] += s.usage.output;
    cacheCreation.data[i] += s.usage.cacheCreation;
    cacheRead.data[i] += s.usage.cacheRead;
  }
  return { buckets, series: [input, output, cacheCreation, cacheRead], skipped };
}

export type Details = Map<string, SessionDetail>;

/** Value contributions of one session split by dim: [name, value][]. Empty when detail missing for model dim. */
function contributions(
  s: SessionSummary,
  dim: Dim,
  metric: Metric,
  details: Details | undefined,
): [string, number][] {
  if (dim === "source") {
    const v = metric === "tokens" ? totalTokens(s.usage) : (s.costUsd ?? 0);
    return [[SOURCE_LABEL[s.source], v]];
  }
  if (dim === "project") {
    const v = metric === "tokens" ? totalTokens(s.usage) : (s.costUsd ?? 0);
    return [[projectName(s.project), v]];
  }
  const d = details?.get(keyOf(s));
  if (!d) return [];
  const acc = new Map<string, number>();
  for (const t of d.turns) {
    const v = metric === "tokens" ? totalTokens(t.usage) : (t.costUsd ?? 0);
    acc.set(t.model, (acc.get(t.model) ?? 0) + v);
  }
  return [...acc];
}

export interface StackedSeries {
  buckets: string[];
  series: NamedSeries[];
  skipped: number;
}

export function stackedBy(
  sessions: SessionSummary[],
  b: Bucket,
  dim: "source" | "model",
  metric: Metric,
  details?: Details,
): StackedSeries {
  const { items, skipped } = keyedSessions(sessions, b);
  const buckets = bucketsOf(items, b);
  const idx = new Map(buckets.map((k, i) => [k, i]));
  const byName = new Map<string, number[]>();
  for (const { key, s } of items) {
    const i = idx.get(key)!;
    for (const [name, v] of contributions(s, dim, metric, details)) {
      let arr = byName.get(name);
      if (!arr) byName.set(name, (arr = buckets.map(() => 0)));
      arr[i] += v;
    }
  }
  let series: NamedSeries[] = [...byName].map(([name, data]) => ({ name, data }));
  const sum = (a: number[]) => a.reduce((x, y) => x + y, 0);
  series.sort((a, b2) => sum(b2.data) - sum(a.data) || a.name.localeCompare(b2.name));
  if (series.length > TOP_N) {
    const rest = series.slice(TOP_N);
    const other = buckets.map((_, i) => rest.reduce((x, r) => x + r.data[i], 0));
    series = [...series.slice(0, TOP_N), { name: OTHER, data: other }];
  }
  return { buckets, series, skipped };
}

export interface Slice {
  name: string;
  value: number;
}

export function breakdown(
  sessions: SessionSummary[],
  dim: Dim,
  metric: Metric,
  details?: Details,
): Slice[] {
  const acc = new Map<string, number>();
  for (const s of sessions) {
    for (const [name, v] of contributions(s, dim, metric, details)) {
      acc.set(name, (acc.get(name) ?? 0) + v);
    }
  }
  const all = [...acc]
    .map(([name, value]) => ({ name, value }))
    .filter((x) => x.value > 0)
    .sort((a, b) => b.value - a.value || a.name.localeCompare(b.name));
  if (all.length <= TOP_N) return all;
  const rest = all.slice(TOP_N).reduce((x, r) => x + r.value, 0);
  return [...all.slice(0, TOP_N), { name: OTHER, value: rest }];
}
