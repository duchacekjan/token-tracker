import { describe, expect, it } from "vitest";
import type { SessionDetail, SessionSummary, TokenUsage, TranscriptSummary, Turn } from "../types";
import { keyOf } from "../filters";
import { fieldsFor } from "./fields";
import { buildHeader, buildRows, defaultFileName, selectSessions, type ExportOptions } from "./rows";

const u = (n: number): TokenUsage => ({ input: n, output: n, cacheCreation: 0, cacheRead: 0, reasoning: 0 });

const tr = (p: Partial<TranscriptSummary>): TranscriptSummary => ({
  filePath: "/f",
  agentId: null,
  agentLabel: null,
  agentType: null,
  isSubagent: false,
  logDeleted: false,
  models: ["m1"],
  startedAt: "2025-01-01T00:00:00Z",
  endedAt: "2025-01-01T00:01:00Z",
  usage: u(10),
  costUsd: 1,
  unpricedModels: [],
  turnCount: 2,
  ...p,
});

function sess(id: string, transcripts: TranscriptSummary[], p: Partial<SessionSummary> = {}): SessionSummary {
  return {
    id,
    source: "claude",
    project: "/p/a",
    filePath: "/f",
    models: [...new Set(transcripts.flatMap((t) => t.models))],
    startedAt: "2025-01-01T00:00:00Z",
    endedAt: "2025-01-01T00:01:00Z",
    usage: u(0),
    costUsd: null,
    unpricedModels: [],
    turnCount: transcripts.reduce((a, t) => a + t.turnCount, 0),
    subagentCount: transcripts.filter((t) => t.isSubagent).length,
    logDeleted: false,
    transcripts,
    ...p,
  };
}

const main = tr({ usage: u(10), costUsd: 1, models: ["m1"] });
const worker = tr({ isSubagent: true, agentId: "w1", agentLabel: "Wk", usage: u(5), costUsd: 0.5, models: ["m2"], unpricedModels: ["m2"] });
const S1 = sess("s1", [main, worker], { usage: u(15), costUsd: 1.5, unpricedModels: ["m2"] });
const S2 = sess("s2", [worker], { usage: u(5), costUsd: 0.5, project: null, source: "codex" });

const opts = (p: Partial<ExportOptions> = {}): ExportOptions => ({
  sources: ["claude", "codex"],
  from: null,
  to: null,
  search: "",
  models: [],
  projects: [],
  includeWorkers: true,
  onlyPriced: false,
  minCostUsd: null,
  level: "session",
  fields: fieldsFor("session").map((f) => f.id),
  delimiter: ",",
  decimal: ".",
  bom: false,
  ...p,
});
const col = (level: "session" | "transcript" | "turn", id: string) => fieldsFor(level).findIndex((f) => f.id === id);

describe("rows", () => {
  it("selectSessions filters source/project", () => {
    expect(selectSessions([S1, S2], opts({ sources: ["codex"] })).map((s) => s.id)).toEqual(["s2"]);
    expect(selectSessions([S1, S2], opts({ projects: [""] })).map((s) => s.id)).toEqual(["s2"]);
    expect(selectSessions([S1, S2], opts({ projects: ["/p/a"] })).map((s) => s.id)).toEqual(["s1"]);
  });

  it("session level includes workers", () => {
    const r = buildRows([S1], opts());
    expect(r[0][col("session", "input")]).toBe(15);
    expect(r[0][col("session", "cost_status")]).toBe("partial");
    expect(r[0][col("session", "total_tokens")]).toBe(30);
  });

  it("excluding workers recomputes from main and drops worker-only", () => {
    const r = buildRows([S1, S2], opts({ includeWorkers: false }));
    expect(r).toHaveLength(1);
    expect(r[0][col("session", "input")]).toBe(10);
    expect(r[0][col("session", "cost_usd")]).toBe(1);
    expect(r[0][col("session", "cost_status")]).toBe("priced");
    expect(r[0][col("session", "models")]).toBe("m1");
    expect(r[0][col("session", "worker_count")]).toBe(0);
    expect(r[0][col("session", "turn_count")]).toBe(2);
  });

  it("model filter per level", () => {
    expect(buildRows([S1], opts({ models: ["m2"] }))).toHaveLength(1);
    expect(buildRows([S1], opts({ models: ["m2"], includeWorkers: false }))).toHaveLength(0);
    const t = buildRows([S1], opts({ level: "transcript", fields: fieldsFor("transcript").map((f) => f.id), models: ["m2"] }));
    expect(t).toHaveLength(1);
    expect(t[0][col("transcript", "role")]).toBe("worker");
    const turns: Turn[] = [
      { timestamp: "2025-01-01T00:00:05Z", model: "m1", usage: u(1), costUsd: 0.1, transcript: 0 },
      { timestamp: null, model: "m2", usage: u(2), costUsd: null, transcript: 1 },
    ];
    const det = new Map<string, SessionDetail>([[keyOf(S1), { summary: S1, turns }]]);
    const to = (p: Partial<ExportOptions>) =>
      opts({ level: "turn", fields: fieldsFor("turn").map((f) => f.id), ...p });
    expect(buildRows([S1], to({ models: ["m2"] }), det)).toHaveLength(1);
    expect(buildRows([S1], to({}), det)).toHaveLength(2);
    expect(buildRows([S1], to({ includeWorkers: false }), det)).toHaveLength(1);
    const r = buildRows([S1], to({ models: ["m1"] }), det);
    expect(r[0][col("turn", "timestamp")]).toBe("2025-01-01T00:00:05.000Z");
    expect(buildRows([S1], to({}), det)[1][col("turn", "timestamp")]).toBeNull();
    expect(buildRows([S1], to({}))).toHaveLength(0);
  });

  it("onlyPriced and minCost", () => {
    const nul = sess("s3", [tr({ costUsd: null })]);
    const a = [S1, nul];
    expect(buildRows(a, opts({ onlyPriced: true }))).toHaveLength(1);
    expect(buildRows([nul], opts({ minCostUsd: 0 }))).toHaveLength(0);
    expect(buildRows([S1], opts({ minCostUsd: 1.5 }))).toHaveLength(1);
    expect(buildRows([S1], opts({ minCostUsd: 1.6 }))).toHaveLength(0);
  });

  it("transcript level: labels and exclusion", () => {
    const o = opts({ level: "transcript", fields: fieldsFor("transcript").map((f) => f.id) });
    const r = buildRows([S1], o);
    expect(r.map((x) => x[col("transcript", "agent_label")])).toEqual(["Main", "Wk"]);
    expect(buildRows([S1], { ...o, includeWorkers: false })).toHaveLength(1);
  });

  it("ISO UTC timestamps", () => {
    const r = buildRows([S1], opts());
    expect(r[0][col("session", "started_at")]).toBe("2025-01-01T00:00:00.000Z");
  });

  it("field order follows definition order, not selection order", () => {
    const o = opts({ fields: ["cost_usd", "session_id", "source"] });
    expect(buildHeader(o)).toEqual(["source", "session_id", "cost_usd"]);
    expect(buildRows([S1], o)[0]).toEqual(["claude", "s1", 1.5]);
  });

  it("default file name", () => {
    expect(defaultFileName("turn", new Date(2025, 2, 5))).toBe("token-tracker-turn-20250305.csv");
  });
});
