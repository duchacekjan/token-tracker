import type { SessionSummary, TokenUsage } from "./types";

const intFmt = new Intl.NumberFormat("en-US");

export const fmtInt = (n: number): string => intFmt.format(n);

export function fmtCompact(n: number): string {
  const abs = Math.abs(n);
  if (abs < 1000) return String(n);
  const units: [number, string][] = [
    [1e9, "B"],
    [1e6, "M"],
    [1e3, "K"],
  ];
  for (const [v, s] of units) {
    if (abs >= v) {
      const x = n / v;
      return `${Math.abs(x) >= 100 ? x.toFixed(0) : x.toFixed(1).replace(/\.0$/, "")}${s}`;
    }
  }
  return String(n);
}

export function fmtUsd(n: number | null | undefined): string {
  if (n == null) return "—";
  const digits = n !== 0 && Math.abs(n) < 0.01 ? 4 : 2;
  return `$${n.toLocaleString("en-US", {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  })}`;
}

export function fmtDateTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "—";
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function durationMs(start: string | null, end: string | null): number | null {
  if (!start || !end) return null;
  const ms = new Date(end).getTime() - new Date(start).getTime();
  return isNaN(ms) || ms < 0 ? null : ms;
}

export function fmtDuration(ms: number | null): string {
  if (ms == null) return "—";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${String(m % 60).padStart(2, "0")}m`;
  return `${Math.floor(h / 24)}d ${h % 24}h`;
}

export function projectName(project: string | null): string {
  if (!project) return "(unknown)";
  const parts = project.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? project;
}

export function totalTokens(u: TokenUsage): number {
  return u.input + u.output + u.cacheCreation + u.cacheRead;
}

export const sessionTotal = (s: SessionSummary): number => totalTokens(s.usage);
