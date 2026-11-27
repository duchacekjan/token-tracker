import type {
  SessionDetail,
  SessionKey,
  SessionSummary,
  Settings,
  SettingsView,
  Source,
  TokenUsage,
  TranscriptSummary,
  Turn,
} from "./types";

const MIN = 60_000;
const base = Date.now() - 3 * 24 * 60 * MIN;
const at = (offsetMin: number) => new Date(base + offsetMin * MIN).toISOString();

const rates: Record<string, number> = {
  "claude-sonnet-4-5": 3,
  "claude-opus-4-1": 15,
  "claude-haiku-4-5": 1,
  "gpt-5-codex": 1.25,
};
const rateOf = (m: string): number | null => rates[m] ?? null;

function mkTurns(start: number, models: string[], n: number, scale: number): Turn[] {
  const turns: Turn[] = [];
  for (let i = 0; i < n; i++) {
    const model = models[i % models.length];
    const usage: TokenUsage = {
      input: Math.round((200 + ((i * 137) % 900)) * scale),
      output: Math.round((300 + ((i * 251) % 1800)) * scale),
      cacheCreation: Math.round(((i * 977) % 6000) * scale),
      cacheRead: Math.round((15000 + i * 2300) * scale),
      reasoning: model.startsWith("gpt") ? Math.round((i * 90) % 700) : 0,
    };
    const rate = rateOf(model);
    turns.push({
      transcript: 0,
      timestamp: at(start + i * 2),
      model,
      usage,
      costUsd:
        rate == null
          ? null
          : ((usage.input + usage.cacheCreation * 1.25 + usage.cacheRead * 0.1) * rate +
              usage.output * rate * 5) /
            1e6,
    });
  }
  return turns;
}

const sumUsage = (turns: Turn[]): TokenUsage => {
  const u: TokenUsage = { input: 0, output: 0, cacheCreation: 0, cacheRead: 0, reasoning: 0 };
  for (const t of turns) {
    u.input += t.usage.input;
    u.output += t.usage.output;
    u.cacheCreation += t.usage.cacheCreation;
    u.cacheRead += t.usage.cacheRead;
    u.reasoning += t.usage.reasoning;
  }
  return u;
};

const sumCost = (turns: Turn[]): number | null => {
  const priced = turns.filter((t) => t.costUsd != null);
  return priced.length ? priced.reduce((a, t) => a + (t.costUsd ?? 0), 0) : null;
};

interface TSpec {
  models: string[];
  n: number;
  scale: number;
  start: number;
  label?: string;
  agentType?: string;
  logDeleted?: boolean;
}

function build(
  id: string,
  source: "claude" | "codex",
  project: string,
  main: TSpec,
  subs: TSpec[] = [],
): SessionDetail {
  const dir =
    source === "claude"
      ? `/home/dev/.claude/projects/${project.replace(/\//g, "-")}`
      : "/home/dev/.codex/sessions/2025/01";
  const specs = [main, ...subs];
  const turns: Turn[] = [];
  const transcripts: TranscriptSummary[] = specs.map((sp, i) => {
    const isSub = i > 0;
    const ts = mkTurns(sp.start, sp.models, sp.n, sp.scale).map((t) => ({ ...t, transcript: i }));
    turns.push(...ts);
    return {
      filePath: isSub
        ? `${dir}/${id}/subagents/agent-a${i}.jsonl`
        : source === "claude"
          ? `${dir}/${id}.jsonl`
          : `${dir}/rollout-${id}.jsonl`,
      agentId: isSub ? `a${i}` : null,
      agentLabel: sp.label ?? null,
      agentType: sp.agentType ?? null,
      isSubagent: isSub,
      logDeleted: sp.logDeleted ?? false,
      models: sp.models,
      startedAt: ts[0].timestamp,
      endedAt: ts[ts.length - 1].timestamp,
      usage: sumUsage(ts),
      costUsd: sumCost(ts),
      unpricedModels: sp.models.filter((m) => rateOf(m) == null),
      turnCount: ts.length,
    };
  });
  turns.sort((a, b) => (a.timestamp ?? "").localeCompare(b.timestamp ?? ""));
  const models = [...new Set(specs.flatMap((sp) => sp.models))];
  const stamps = turns.map((t) => t.timestamp).filter((t): t is string => t != null);
  const summary: SessionSummary = {
    id,
    source,
    project,
    filePath: transcripts[0].filePath,
    models,
    startedAt: stamps[0] ?? null,
    endedAt: stamps[stamps.length - 1] ?? null,
    usage: sumUsage(turns),
    costUsd: sumCost(turns),
    unpricedModels: models.filter((m) => rateOf(m) == null),
    turnCount: turns.length,
    subagentCount: subs.length,
    logDeleted: transcripts.every((t) => t.logDeleted),
    transcripts,
  };
  return { summary, turns };
}

const details: SessionDetail[] = [
  build(
    "3f9a1c52-aaaa-4b1e-9d11-000000000001",
    "claude",
    "/home/dev/work/token-tracker",
    { models: ["claude-sonnet-4-5"], n: 42, scale: 1, start: 0 },
    [
      { models: ["claude-sonnet-4-5"], n: 14, scale: 0.8, start: 4, label: "Scaffold app", agentType: "itixo:itixo-builder" },
      { models: ["claude-sonnet-4-5", "claude-haiku-4-5"], n: 20, scale: 0.7, start: 12, label: "Implement parser", agentType: "itixo:itixo-builder" },
      { models: ["claude-haiku-4-5"], n: 8, scale: 0.4, start: 30, label: "Review diff", agentType: "itixo:itixo-reviewer" },
    ],
  ),
  build("7b21de90-bbbb-4c2f-8e22-000000000002", "claude", "/home/dev/work/api-gateway", { models: ["claude-opus-4-1", "claude-haiku-4-5"], n: 18, scale: 2, start: 600, logDeleted: true }),
  build("c0de1234-cccc-4d3a-8f33-000000000003", "claude", "/home/dev/side/blog", { models: ["claude-sonnet-4-5", "claude-experimental-x"], n: 9, scale: 0.6, start: 1500 }),
  build("019a-dddd-0001", "codex", "/home/dev/work/api-gateway", { models: ["gpt-5-codex"], n: 25, scale: 1.4, start: 2300 }),
  build("019a-eeee-0002", "codex", "/home/dev/side/dotfiles", { models: ["gpt-5-codex"], n: 6, scale: 0.4, start: 3100 }),
  build("019a-ffff-0003", "codex", "/home/dev/side/preview-test", { models: ["codex-next-preview"], n: 5, scale: 0.5, start: 3600 }),
];

let mockSettings: Settings = { extraClaudeDirs: ["/mnt/backup/.claude/projects"], extraCodexDirs: [], extraGeminiDirs: [] };
let pricingUpdated: string | null = new Date().toISOString();

const delay = <T>(v: T, ms = 150) => new Promise<T>((r) => setTimeout(() => r(v), ms));

const settingsView = (): SettingsView => ({
  settings: structuredClone(mockSettings),
  claudeDirs: ["/home/dev/.claude/projects", ...mockSettings.extraClaudeDirs],
  codexDirs: ["/home/dev/.codex/sessions", ...mockSettings.extraCodexDirs],
  geminiDirs: ["/home/dev/.gemini/tmp", ...mockSettings.extraGeminiDirs],
  pricing: {
    source: "bundled",
    updatedAt: pricingUpdated,
    modelCount: 128,
    error: "offline: request timed out",
  },
});

export const mockApi = {
  listSessions: () => delay(details.map((d) => d.summary)),
  getSession: (source: Source, id: string) => {
    const d = details.find((x) => x.summary.source === source && x.summary.id === id);
    return d ? delay(d) : Promise.reject(new Error(`session not found: ${source}:${id}`));
  },
  getSessionsDetail: (keys: SessionKey[]) => {
    const found: SessionDetail[] = [];
    for (const k of keys) {
      const d = details.find((x) => x.summary.source === k.source && x.summary.id === k.id);
      if (d) found.push(d);
    }
    return delay(found);
  },
  getSettings: () => delay(settingsView()),
  saveSettings: (s: Settings) => {
    mockSettings = structuredClone(s);
    return delay(settingsView());
  },
  // To preview the banner, return a string instead of null, e.g.
  // delay("The database could not be opened. It was renamed to /home/dev/token-tracker.sqlite.broken-20250101-120000 ...")
  getDbWarning: (): Promise<string | null> => delay(null),
  refreshPricing: () => {
    pricingUpdated = new Date().toISOString();
    return delay(settingsView().pricing, 600);
  },
};
