import { describe, expect, it } from "vitest";
import type { SessionDetail, SessionSummary, TokenUsage } from "../types";
import { keyOf } from "../filters";
import { breakdown, bucketKey, bucketRange, stackedBy, timeSeries, tokenTypeSeries } from "./data";

const local = (y: number, m: number, d: number, h = 12) => new Date(y, m - 1, d, h).toISOString();
const usage = (o: Partial<TokenUsage> = {}): TokenUsage => ({
  input: 0,
  output: 0,
  cacheCreation: 0,
  cacheRead: 0,
  reasoning: 0,
  ...o,
});

function sess(id: string, startedAt: string | null, o: Partial<SessionSummary> = {}): SessionSummary {
  return {
    id,
    source: "claude",
    project: "/home/u/proj",
    filePath: `/f/${id}`,
    models: [],
    startedAt,
    endedAt: null,
    usage: usage({ input: 10 }),
    costUsd: 1,
    unpricedModels: [],
    turnCount: 1,
    subagentCount: 0,
    logDeleted: false,
    transcripts: [],
    ...o,
  };
}

describe("bucketKey", () => {
  it("day and month at edges", () => {
    expect(bucketKey(local(2024, 12, 31, 23), "day")).toBe("2024-12-31");
    expect(bucketKey(local(2025, 1, 1, 0), "day")).toBe("2025-01-01");
    expect(bucketKey(local(2024, 2, 29), "month")).toBe("2024-02");
    expect(bucketKey(local(2025, 1, 1, 0), "month")).toBe("2025-01");
  });
  it("week is Monday, across year boundary", () => {
    // 2025-01-01 is a Wednesday -> Monday 2024-12-30
    expect(bucketKey(local(2025, 1, 1), "week")).toBe("2024-12-30");
    expect(bucketKey(local(2024, 12, 30), "week")).toBe("2024-12-30");
    // Sunday 2025-01-05 still in that week
    expect(bucketKey(local(2025, 1, 5, 23), "week")).toBe("2024-12-30");
    expect(bucketKey(local(2025, 1, 6), "week")).toBe("2025-01-06");
  });
  it("invalid -> null", () => {
    expect(bucketKey("nope", "day")).toBeNull();
  });
});

describe("bucketRange", () => {
  it("fills gaps", () => {
    expect(bucketRange("2024-12-30", "2025-01-02", "day")).toEqual([
      "2024-12-30",
      "2024-12-31",
      "2025-01-01",
      "2025-01-02",
    ]);
    expect(bucketRange("2024-11", "2025-02", "month")).toEqual(["2024-11", "2024-12", "2025-01", "2025-02"]);
    expect(bucketRange("2024-12-23", "2025-01-06", "week")).toEqual(["2024-12-23", "2024-12-30", "2025-01-06"]);
  });
});

describe("timeSeries", () => {
  it("gap fills, skips null start, ignores null cost", () => {
    const ss = [
      sess("a", local(2025, 3, 1), { usage: usage({ input: 5, output: 5 }), costUsd: 2 }),
      sess("b", local(2025, 3, 4), { usage: usage({ cacheRead: 7, reasoning: 99 }), costUsd: null }),
      sess("c", null),
    ];
    const r = timeSeries(ss, "day");
    expect(r.buckets).toEqual(["2025-03-01", "2025-03-02", "2025-03-03", "2025-03-04"]);
    expect(r.tokens).toEqual([10, 0, 0, 7]);
    expect(r.cost).toEqual([2, 0, 0, 0]);
    expect(r.skipped).toBe(1);
  });
  it("empty", () => {
    expect(timeSeries([], "day").buckets).toEqual([]);
  });
});

describe("tokenTypeSeries", () => {
  it("has four series, no reasoning", () => {
    const r = tokenTypeSeries(
      [sess("a", local(2025, 3, 1), { usage: usage({ input: 1, output: 2, cacheCreation: 3, cacheRead: 4, reasoning: 9 }) })],
      "month",
    );
    expect(r.series.map((s) => s.name)).toEqual(["Input", "Output", "Cache creation", "Cache read"]);
    expect(r.series.map((s) => s.data[0])).toEqual([1, 2, 3, 4]);
  });
});

describe("model split", () => {
  it("splits one transcript with two models exactly", () => {
    const s = sess("a", local(2025, 3, 1), { usage: usage({ input: 30 }), costUsd: 3 });
    const detail: SessionDetail = {
      summary: s,
      turns: [
        { timestamp: null, model: "m1", usage: usage({ input: 10 }), costUsd: 1, transcript: 0 },
        { timestamp: null, model: "m2", usage: usage({ input: 15, output: 5 }), costUsd: null, transcript: 0 },
        { timestamp: null, model: "m1", usage: usage({ input: 5 }), costUsd: 2, transcript: 0 },
      ],
    };
    const details = new Map([[keyOf(s), detail]]);
    expect(breakdown([s], "model", "tokens", details)).toEqual([
      { name: "m2", value: 20 },
      { name: "m1", value: 15 },
    ]);
    expect(breakdown([s], "model", "cost", details)).toEqual([{ name: "m1", value: 3 }]);
    const st = stackedBy([s], "day", "model", "tokens", details);
    expect(st.series).toEqual([
      { name: "m2", data: [20] },
      { name: "m1", data: [15] },
    ]);
  });
});

describe("breakdown", () => {
  it("top 10 + Other, projects by name, includes null start", () => {
    const ss = Array.from({ length: 12 }, (_, i) =>
      sess(`s${i}`, i === 0 ? null : local(2025, 3, 1), {
        project: `/x/p${i}`,
        usage: usage({ input: 100 - i }),
      }),
    );
    const r = breakdown(ss, "project", "tokens");
    expect(r).toHaveLength(11);
    expect(r[0]).toEqual({ name: "p0", value: 100 });
    expect(r[10]).toEqual({ name: "Other", value: 90 + 89 });
  });
  it("source labels", () => {
    const r = breakdown([sess("a", null), sess("b", null, { source: "codex" })], "source", "tokens");
    expect(r.map((x) => x.name).sort()).toEqual(["Claude Code", "Codex"]);
  });
});
