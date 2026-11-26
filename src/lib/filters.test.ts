import { describe, expect, it } from "vitest";
import { applyFilter, defaultFilter, inDateRange, matchesSearch } from "./filters";
import type { SessionSummary, TranscriptSummary } from "./types";

const usage = { input: 0, output: 0, cacheCreation: 0, cacheRead: 0, reasoning: 0 };
const tr = (o: Partial<TranscriptSummary> = {}): TranscriptSummary => ({
  filePath: "/x.jsonl",
  agentId: null,
  agentLabel: null,
  agentType: null,
  isSubagent: false,
  logDeleted: false,
  models: [],
  startedAt: null,
  endedAt: null,
  usage,
  costUsd: null,
  unpricedModels: [],
  turnCount: 0,
  ...o,
});
const ses = (o: Partial<SessionSummary> = {}): SessionSummary => ({
  id: "abc-123",
  source: "claude",
  project: "/home/dev/Proj",
  filePath: "/x.jsonl",
  models: ["claude-sonnet-4-5"],
  startedAt: null,
  endedAt: null,
  usage,
  costUsd: null,
  unpricedModels: [],
  turnCount: 0,
  subagentCount: 0,
  logDeleted: false,
  transcripts: [tr()],
  ...o,
});
const local = (y: number, mo: number, d: number, h = 0, mi = 0, s = 0, ms = 0) =>
  new Date(y, mo - 1, d, h, mi, s, ms).toISOString();

describe("matchesSearch", () => {
  it("empty/whitespace matches all", () => {
    expect(matchesSearch(ses(), "")).toBe(true);
    expect(matchesSearch(ses(), "   ")).toBe(true);
  });
  it("matches project, id, model case-insensitively", () => {
    expect(matchesSearch(ses(), "PROJ")).toBe(true);
    expect(matchesSearch(ses(), "ABC-1")).toBe(true);
    expect(matchesSearch(ses(), "sonnet")).toBe(true);
    expect(matchesSearch(ses(), "nope")).toBe(false);
  });
  it("null project does not match", () => {
    expect(matchesSearch(ses({ project: null }), "proj")).toBe(false);
  });
  it("matches worker label, agentId, agentType", () => {
    const s = ses({
      transcripts: [
        tr(),
        tr({ isSubagent: true, agentLabel: "Implement parser", agentId: "a7", agentType: "itixo:builder" }),
      ],
    });
    expect(matchesSearch(s, "parser")).toBe(true);
    expect(matchesSearch(s, "a7")).toBe(true);
    expect(matchesSearch(s, "itixo:build")).toBe(true);
  });
  it("worker with no label falls back to agentId then 'worker'", () => {
    const s = ses({ transcripts: [tr({ isSubagent: true })] });
    expect(matchesSearch(s, "worker")).toBe(true);
  });
  it("main transcript label 'Main' is not searchable", () => {
    expect(matchesSearch(ses(), "main")).toBe(false);
  });
});

describe("inDateRange", () => {
  it("no bounds: null startedAt passes", () => {
    expect(inDateRange(null, null, null)).toBe(true);
  });
  it("any bound: null startedAt excluded", () => {
    expect(inDateRange(null, "2025-01-01", null)).toBe(false);
    expect(inDateRange(null, null, "2025-01-01")).toBe(false);
    expect(inDateRange(null, "2025-01-01", "2025-01-02")).toBe(false);
  });
  it("to day inclusive at 00:00 and 23:59 local; next day excluded", () => {
    expect(inDateRange(local(2025, 3, 10, 0, 0), null, "2025-03-10")).toBe(true);
    expect(inDateRange(local(2025, 3, 10, 23, 59, 59, 999), null, "2025-03-10")).toBe(true);
    expect(inDateRange(local(2025, 3, 11, 0, 0), null, "2025-03-10")).toBe(false);
  });
  it("from boundary inclusive", () => {
    expect(inDateRange(local(2025, 3, 10, 0, 0), "2025-03-10", null)).toBe(true);
    expect(inDateRange(local(2025, 3, 9, 23, 59, 59, 999), "2025-03-10", null)).toBe(false);
  });
  it("from and to same day", () => {
    expect(inDateRange(local(2025, 3, 10, 12), "2025-03-10", "2025-03-10")).toBe(true);
    expect(inDateRange(local(2025, 3, 11, 12), "2025-03-10", "2025-03-10")).toBe(false);
  });
  it("month rollover on to", () => {
    expect(inDateRange(local(2025, 1, 31, 23, 59), null, "2025-01-31")).toBe(true);
    expect(inDateRange(local(2025, 2, 1, 0, 0), null, "2025-01-31")).toBe(false);
  });
});

describe("applyFilter", () => {
  const a = ses({ id: "a", source: "claude", startedAt: local(2025, 3, 10, 12) });
  const b = ses({ id: "b", source: "codex", models: ["gpt-5-codex"], startedAt: local(2025, 3, 12, 12) });
  const c = ses({ id: "c", source: "codex", models: ["gpt-5-codex"], startedAt: null });
  const all = [a, b, c];
  it("default returns everything", () => {
    expect(applyFilter(all, defaultFilter())).toEqual(all);
  });
  it("source filter", () => {
    expect(applyFilter(all, { ...defaultFilter(), source: "codex" }).map((s) => s.id)).toEqual(["b", "c"]);
    expect(applyFilter(all, { ...defaultFilter(), source: "claude" }).map((s) => s.id)).toEqual(["a"]);
  });
  it("date bound drops null startedAt", () => {
    expect(applyFilter(all, { ...defaultFilter(), from: "2025-03-01" }).map((s) => s.id)).toEqual(["a", "b"]);
  });
  it("combined", () => {
    const f = { source: "codex" as const, search: "gpt", from: "2025-03-11", to: "2025-03-12" };
    expect(applyFilter(all, f).map((s) => s.id)).toEqual(["b"]);
  });
});
