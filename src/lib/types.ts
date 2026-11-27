export type Source = "claude" | "codex" | "gemini" | "antigravity";

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
  /** Index into SessionSummary.transcripts. */
  transcript: number;
}

export interface TranscriptSummary {
  filePath: string;
  agentId: string | null;
  agentLabel: string | null;
  agentType: string | null;
  isSubagent: boolean;
  logDeleted: boolean;
  models: string[];
  startedAt: string | null;
  endedAt: string | null;
  usage: TokenUsage;
  costUsd: number | null;
  unpricedModels: string[];
  turnCount: number;
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
  subagentCount: number;
  /** True only if ALL transcripts' logs are deleted. */
  logDeleted: boolean;
  /** Main first, then subagents by startedAt. */
  transcripts: TranscriptSummary[];
}

export interface SessionDetail {
  summary: SessionSummary;
  turns: Turn[];
}

export interface Settings {
  extraClaudeDirs: string[];
  extraCodexDirs: string[];
  extraGeminiDirs: string[];
  extraAntigravityDirs: string[];
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
  geminiDirs: string[];
  antigravityDirs: string[];
  pricing: PricingStatus;
}

export interface SessionKey {
  source: Source;
  id: string;
}

export type HarnessFilter = "all" | Source;

export interface SessionFilter {
  source: HarnessFilter;
  search: string;
  /** YYYY-MM-DD, local time, inclusive. */
  from: string | null;
  /** YYYY-MM-DD, local time, inclusive. */
  to: string | null;
}
