import { totalTokens } from "../format";
import { trLabel } from "../filters";
import type { SessionSummary, TokenUsage, TranscriptSummary, Turn } from "../types";
import type { Cell } from "./csv";

export type Level = "session" | "transcript" | "turn";

export interface FieldRow {
  session: SessionSummary;
  transcript?: TranscriptSummary;
  transcriptIndex?: number;
  turn?: Turn;
  /** Session-level values when workers are excluded (override session fields). */
  override?: {
    models: string[];
    startedAt: string | null;
    endedAt: string | null;
    usage: TokenUsage;
    costUsd: number | null;
    unpricedModels: string[];
    turnCount: number;
    transcriptCount: number;
    workerCount: number;
    logDeleted: boolean;
  };
}

export interface FieldDef {
  id: string;
  header: string;
  default: boolean;
  get: (row: FieldRow) => Cell;
}

const iso = (x: string | null | undefined): Cell => {
  if (!x) return null;
  const d = new Date(x);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
};

const durationSec = (a: string | null, b: string | null): Cell => {
  if (!a || !b) return null;
  const ms = new Date(b).getTime() - new Date(a).getTime();
  return Number.isNaN(ms) || ms < 0 ? null : ms / 1000;
};

export function costStatus(cost: number | null, unpriced: string[]): string {
  if (cost == null) return "unpriced";
  return unpriced.length > 0 ? "partial" : "priced";
}

const f = (id: string, def: boolean, get: (r: FieldRow) => Cell): FieldDef => ({ id, header: id, default: def, get });
const j = (a: string[]) => a.join(" | ");

// session-level accessors honor override
const sv = (r: FieldRow) =>
  r.override ?? {
    models: r.session.models,
    startedAt: r.session.startedAt,
    endedAt: r.session.endedAt,
    usage: r.session.usage,
    costUsd: r.session.costUsd,
    unpricedModels: r.session.unpricedModels,
    turnCount: r.session.turnCount,
    transcriptCount: r.session.transcripts.length,
    workerCount: r.session.subagentCount,
    logDeleted: r.session.logDeleted,
  };

const usageFields = (u: (r: FieldRow) => TokenUsage): FieldDef[] => [
  f("input", true, (r) => u(r).input),
  f("output", true, (r) => u(r).output),
  f("cache_creation", true, (r) => u(r).cacheCreation),
  f("cache_read", true, (r) => u(r).cacheRead),
  f("reasoning", true, (r) => u(r).reasoning),
  f("total_tokens", true, (r) => totalTokens(u(r))),
];

const common = (): FieldDef[] => [
  f("source", true, (r) => r.session.source),
  f("session_id", true, (r) => r.session.id),
  f("project", true, (r) => r.session.project),
];

const SESSION: FieldDef[] = [
  ...common(),
  f("project_name", false, (r) => {
    const p = r.session.project;
    if (!p) return null;
    const parts = p.split(/[\\/]+/).filter(Boolean);
    return parts[parts.length - 1] ?? p;
  }),
  f("models", true, (r) => j(sv(r).models)),
  f("started_at", true, (r) => iso(sv(r).startedAt)),
  f("ended_at", true, (r) => iso(sv(r).endedAt)),
  f("duration_sec", false, (r) => durationSec(sv(r).startedAt, sv(r).endedAt)),
  f("transcript_count", false, (r) => sv(r).transcriptCount),
  f("worker_count", true, (r) => sv(r).workerCount),
  f("turn_count", true, (r) => sv(r).turnCount),
  ...usageFields((r) => sv(r).usage),
  f("cost_usd", true, (r) => sv(r).costUsd),
  f("cost_status", true, (r) => costStatus(sv(r).costUsd, sv(r).unpricedModels)),
  f("unpriced_models", false, (r) => j(sv(r).unpricedModels)),
  f("log_deleted", false, (r) => sv(r).logDeleted),
  f("file_path", false, (r) => r.session.filePath),
];

const tr = (r: FieldRow) => r.transcript as TranscriptSummary;
const role = (r: FieldRow): string => (tr(r).isSubagent ? "worker" : "main");

const TRANSCRIPT: FieldDef[] = [
  ...common(),
  f("transcript_index", false, (r) => r.transcriptIndex ?? null),
  f("role", true, role),
  f("agent_id", false, (r) => tr(r).agentId),
  f("agent_label", true, (r) => trLabel(tr(r))),
  f("agent_type", true, (r) => tr(r).agentType),
  f("models", true, (r) => j(tr(r).models)),
  f("started_at", true, (r) => iso(tr(r).startedAt)),
  f("ended_at", true, (r) => iso(tr(r).endedAt)),
  f("duration_sec", false, (r) => durationSec(tr(r).startedAt, tr(r).endedAt)),
  f("turn_count", true, (r) => tr(r).turnCount),
  ...usageFields((r) => tr(r).usage),
  f("cost_usd", true, (r) => tr(r).costUsd),
  f("cost_status", false, (r) => costStatus(tr(r).costUsd, tr(r).unpricedModels)),
  f("unpriced_models", false, (r) => j(tr(r).unpricedModels)),
  f("log_deleted", false, (r) => tr(r).logDeleted),
  f("file_path", false, (r) => tr(r).filePath),
];

const turn = (r: FieldRow) => r.turn as Turn;
const TURN: FieldDef[] = [
  ...common(),
  f("transcript_index", false, (r) => r.transcriptIndex ?? null),
  f("role", true, role),
  f("agent_label", true, (r) => trLabel(tr(r))),
  f("timestamp", true, (r) => iso(turn(r).timestamp)),
  f("model", true, (r) => turn(r).model),
  ...usageFields((r) => turn(r).usage),
  f("cost_usd", true, (r) => turn(r).costUsd),
  f("priced", false, (r) => turn(r).costUsd != null),
];

export function fieldsFor(level: Level): FieldDef[] {
  return level === "session" ? SESSION : level === "transcript" ? TRANSCRIPT : TURN;
}
