import { inDateRange, keyOf, matchesSearch } from "../filters";
import type { SessionDetail, SessionSummary, Source, TokenUsage } from "../types";
import type { Cell, CsvOptions } from "./csv";
import { fieldsFor, type FieldDef, type FieldRow, type Level } from "./fields";

export interface ExportOptions extends CsvOptions {
  sources: Source[];
  from: string | null;
  to: string | null;
  search: string;
  /** Empty = all. */
  models: string[];
  /** Empty = all. "" = unknown project. */
  projects: string[];
  includeWorkers: boolean;
  onlyPriced: boolean;
  minCostUsd: number | null;
  level: Level;
  fields: string[];
}

export function selectSessions(all: SessionSummary[], o: ExportOptions): SessionSummary[] {
  const projects = new Set(o.projects);
  return all.filter(
    (s) =>
      o.sources.includes(s.source) &&
      inDateRange(s.startedAt, o.from, o.to) &&
      matchesSearch(s, o.search) &&
      (projects.size === 0 || projects.has(s.project ?? "")),
  );
}

export function selectedFields(level: Level, ids: string[]): FieldDef[] {
  const set = new Set(ids);
  return fieldsFor(level).filter((f) => set.has(f.id));
}

export function buildHeader(o: ExportOptions): string[] {
  return selectedFields(o.level, o.fields).map((f) => f.header);
}

const addUsage = (a: TokenUsage, b: TokenUsage): TokenUsage => ({
  input: a.input + b.input,
  output: a.output + b.output,
  cacheCreation: a.cacheCreation + b.cacheCreation,
  cacheRead: a.cacheRead + b.cacheRead,
  reasoning: a.reasoning + b.reasoning,
});

const union = (lists: string[][]): string[] => [...new Set(lists.flat())];

/** Session totals from main (non-worker) transcripts only; null if no main transcript. */
function mainOverride(s: SessionSummary): FieldRow["override"] | null {
  const mains = s.transcripts.filter((t) => !t.isSubagent);
  if (mains.length === 0) return null;
  const costs = mains.map((t) => t.costUsd).filter((c): c is number => c != null);
  const starts = mains.map((t) => t.startedAt).filter((x): x is string => !!x);
  const ends = mains.map((t) => t.endedAt).filter((x): x is string => !!x);
  const ts = (xs: string[], pick: 1 | -1) =>
    xs.length === 0 ? null : xs.reduce((a, b) => (pick * (new Date(b).getTime() - new Date(a).getTime()) < 0 ? b : a));
  return {
    models: union(mains.map((t) => t.models)),
    startedAt: ts(starts, 1),
    endedAt: ts(ends, -1),
    usage: mains.map((t) => t.usage).reduce(addUsage, { input: 0, output: 0, cacheCreation: 0, cacheRead: 0, reasoning: 0 }),
    costUsd: costs.length === 0 ? null : costs.reduce((a, b) => a + b, 0),
    unpricedModels: union(mains.map((t) => t.unpricedModels)),
    turnCount: mains.reduce((a, t) => a + t.turnCount, 0),
    transcriptCount: mains.length,
    workerCount: 0,
    logDeleted: mains.every((t) => t.logDeleted),
  };
}

function rowsFor(s: SessionSummary, o: ExportOptions, details?: Map<string, SessionDetail>): { fr: FieldRow; cost: number | null }[] {
  const models = new Set(o.models);
  const modelOk = (list: string[]) => models.size === 0 || list.some((m) => models.has(m));
  const out: { fr: FieldRow; cost: number | null }[] = [];
  if (o.level === "session") {
    let override: FieldRow["override"];
    if (!o.includeWorkers) {
      const m = mainOverride(s);
      if (!m) return out;
      override = m;
    }
    const eff = override ?? s;
    if (!modelOk(eff.models)) return out;
    out.push({ fr: { session: s, override }, cost: eff.costUsd });
  } else if (o.level === "transcript") {
    s.transcripts.forEach((t, i) => {
      if (!o.includeWorkers && t.isSubagent) return;
      if (!modelOk(t.models)) return;
      out.push({ fr: { session: s, transcript: t, transcriptIndex: i }, cost: t.costUsd });
    });
  } else {
    const d = details?.get(keyOf(s));
    if (!d) return out;
    for (const turn of d.turns) {
      const t = s.transcripts[turn.transcript];
      if (!t) continue;
      if (!o.includeWorkers && t.isSubagent) continue;
      if (!modelOk([turn.model])) continue;
      out.push({ fr: { session: s, transcript: t, transcriptIndex: turn.transcript, turn }, cost: turn.costUsd });
    }
  }
  return out;
}

export function buildRows(
  sessions: SessionSummary[],
  o: ExportOptions,
  details?: Map<string, SessionDetail>,
): Cell[][] {
  const fields = selectedFields(o.level, o.fields);
  const rows: Cell[][] = [];
  for (const s of sessions) {
    for (const { fr, cost } of rowsFor(s, o, details)) {
      if (o.onlyPriced && cost == null) continue;
      if (o.minCostUsd != null && !(cost != null && cost >= o.minCostUsd)) continue;
      rows.push(fields.map((f) => f.get(fr)));
    }
  }
  return rows;
}

/** Row count without details; turn level estimated from summary turnCount (ignores model/priced filters). */
export function estimateCount(sessions: SessionSummary[], o: ExportOptions): number {
  if (o.level === "turn") {
    let n = 0;
    for (const s of sessions) {
      n += o.includeWorkers ? s.turnCount : s.transcripts.filter((t) => !t.isSubagent).reduce((a, t) => a + t.turnCount, 0);
    }
    return n;
  }
  return buildRows(sessions, { ...o, fields: [fieldsFor(o.level)[0].id] }).length;
}

export function defaultFileName(level: Level, now: Date = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `token-tracker-${level}-${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}.csv`;
}
