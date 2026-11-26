import type { SessionFilter, SessionSummary, TranscriptSummary } from "./types";

export const defaultFilter = (): SessionFilter => ({ source: "all", search: "", from: null, to: null });

export const keyOf = (s: SessionSummary) => `${s.source}:${s.id}`;

export const trLabel = (t: TranscriptSummary) =>
  t.isSubagent ? (t.agentLabel ?? t.agentId ?? "worker") : "Main";

export function matchesSearch(s: SessionSummary, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    (s.project ?? "").toLowerCase().includes(q) ||
    s.id.toLowerCase().includes(q) ||
    s.models.some((m) => m.toLowerCase().includes(q)) ||
    s.transcripts.some(
      (t) =>
        t.isSubagent &&
        (trLabel(t).toLowerCase().includes(q) ||
          (t.agentId ?? "").toLowerCase().includes(q) ||
          (t.agentType ?? "").toLowerCase().includes(q)),
    )
  );
}

function localDayStart(ymd: string, addDays = 0): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(ymd);
  if (!m) return null;
  return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]) + addDays).getTime();
}

/** Inclusive local-day range. Any bound set excludes a null/invalid startedAt. */
export function inDateRange(startedAt: string | null, from: string | null, to: string | null): boolean {
  const lo = from ? localDayStart(from) : null;
  const hi = to ? localDayStart(to, 1) : null;
  if (lo == null && hi == null) return true;
  if (!startedAt) return false;
  const t = new Date(startedAt).getTime();
  if (Number.isNaN(t)) return false;
  if (lo != null && t < lo) return false;
  if (hi != null && t >= hi) return false;
  return true;
}

export function applyFilter(sessions: SessionSummary[], f: SessionFilter): SessionSummary[] {
  return sessions.filter(
    (s) =>
      (f.source === "all" || s.source === f.source) &&
      matchesSearch(s, f.search) &&
      inDateRange(s.startedAt, f.from, f.to),
  );
}
