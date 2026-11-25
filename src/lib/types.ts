export type Source = "claude" | "codex";

export interface TokenUsage {
  input: number;
  output: number;
  cacheCreation: number;
  cacheRead: number;
  reasoning: number;
}

export interface Turn {
  timestamp: string | null;
  model: string;
  usage: TokenUsage;
  costUsd: number | null;
}

export interface SessionSummary {
  id: string;
  source: Source;
  project: string | null;
  filePath: string;
  models: string[];
  startedAt: string | null;
  endedAt: string | null;
  usage: TokenUsage;
  costUsd: number | null;
  unpricedModels: string[];
  turnCount: number;
}

export interface SessionDetail {
  summary: SessionSummary;
  turns: Turn[];
}

export interface Settings {
  extraClaudeDirs: string[];
  extraCodexDirs: string[];
}

export interface PricingStatus {
  source: "online" | "cache" | "bundled";
  updatedAt: string | null;
  modelCount: number;
  error: string | null;
}

export interface SettingsView {
  settings: Settings;
  claudeDirs: string[];
  codexDirs: string[];
  pricing: PricingStatus;
}
