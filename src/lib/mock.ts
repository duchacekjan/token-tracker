import type {
  SessionDetail,
  SessionSummary,
  Settings,
  SettingsView,
  TokenUsage,
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

function build(
  id: string,
  source: "claude" | "codex",
  project: string,
  start: number,
  models: string[],
  n: number,
  scale: number,
): SessionDetail {
  const turns = mkTurns(start, models, n, scale);
  const usage: TokenUsage = { input: 0, output: 0, cacheCreation: 0, cacheRead: 0, reasoning: 0 };
  let cost = 0;
  let anyPriced = false;
  for (const t of turns) {
    usage.input += t.usage.input;
    usage.output += t.usage.output;
    usage.cacheCreation += t.usage.cacheCreation;
    usage.cacheRead += t.usage.cacheRead;
    usage.reasoning += t.usage.reasoning;
    if (t.costUsd != null) {
      cost += t.costUsd;
      anyPriced = true;
    }
  }
  const summary: SessionSummary = {
    id,
    source,
    project,
    filePath:
      source === "claude"
        ? `/home/dev/.claude/projects/${project.replace(/\//g, "-")}/${id}.jsonl`
        : `/home/dev/.codex/sessions/2025/01/rollout-${id}.jsonl`,
    models,
    startedAt: turns[0].timestamp,
    endedAt: turns[turns.length - 1].timestamp,
    usage,
    costUsd: anyPriced ? cost : null,
    unpricedModels: models.filter((m) => rateOf(m) == null),
    turnCount: n,
  };
  return { summary, turns };
}

const details: SessionDetail[] = [
  build("3f9a1c52-aaaa-4b1e-9d11-000000000001", "claude", "/home/dev/work/token-tracker", 0, ["claude-sonnet-4-5"], 42, 1),
  build("7b21de90-bbbb-4c2f-8e22-000000000002", "claude", "/home/dev/work/api-gateway", 600, ["claude-opus-4-1", "claude-haiku-4-5"], 18, 2),
  build("c0de1234-cccc-4d3a-8f33-000000000003", "claude", "/home/dev/side/blog", 1500, ["claude-sonnet-4-5", "claude-experimental-x"], 9, 0.6),
  build("019a-dddd-0001", "codex", "/home/dev/work/api-gateway", 2300, ["gpt-5-codex"], 25, 1.4),
  build("019a-eeee-0002", "codex", "/home/dev/side/dotfiles", 3100, ["gpt-5-codex"], 6, 0.4),
  build("019a-ffff-0003", "codex", "/home/dev/side/preview-test", 3600, ["codex-next-preview"], 5, 0.5),
];

let mockSettings: Settings = { extraClaudeDirs: ["/mnt/backup/.claude/projects"], extraCodexDirs: [] };
let pricingUpdated: string | null = new Date().toISOString();

const delay = <T>(v: T, ms = 150) => new Promise<T>((r) => setTimeout(() => r(v), ms));

const settingsView = (): SettingsView => ({
  settings: structuredClone(mockSettings),
  claudeDirs: ["/home/dev/.claude/projects", ...mockSettings.extraClaudeDirs],
  codexDirs: ["/home/dev/.codex/sessions", ...mockSettings.extraCodexDirs],
  pricing: {
    source: "bundled",
    updatedAt: pricingUpdated,
    modelCount: 128,
    error: "offline: request timed out",
  },
});

export const mockApi = {
  listSessions: () => delay(details.map((d) => d.summary)),
  getSession: (filePath: string) => {
    const d = details.find((x) => x.summary.filePath === filePath);
    return d ? delay(d) : Promise.reject(new Error(`session not found: ${filePath}`));
  },
  getSettings: () => delay(settingsView()),
  saveSettings: (s: Settings) => {
    mockSettings = structuredClone(s);
    return delay(settingsView());
  },
  refreshPricing: () => {
    pricingUpdated = new Date().toISOString();
    return delay(settingsView().pricing, 600);
  },
};
