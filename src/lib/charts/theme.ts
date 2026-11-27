export interface Theme {
  bg: string;
  surface: string;
  border: string;
  text: string;
  muted: string;
  accent: string;
  claude: string;
  codex: string;
  gemini: string;
  antigravity: string;
  dark: boolean;
  palette: string[];
}

const LIGHT_PALETTE = [
  "#4f46e5",
  "#0891b2",
  "#d97706",
  "#be185d",
  "#65a30d",
  "#7c3aed",
  "#dc2626",
  "#0d9488",
  "#a16207",
  "#475569",
];
const DARK_PALETTE = [
  "#818cf8",
  "#22d3ee",
  "#fbbf24",
  "#f472b6",
  "#a3e635",
  "#a78bfa",
  "#f87171",
  "#2dd4bf",
  "#facc15",
  "#94a3b8",
];

const query = () => window.matchMedia("(prefers-color-scheme: dark)");

export function isDark(): boolean {
  return query().matches;
}

export function readTheme(): Theme {
  const cs = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) => cs.getPropertyValue(name).trim() || fallback;
  const dark = isDark();
  return {
    bg: v("--bg", dark ? "#12141a" : "#f6f7f9"),
    surface: v("--surface", dark ? "#1a1d26" : "#ffffff"),
    border: v("--border", dark ? "#2f3445" : "#d9dde4"),
    text: v("--text", dark ? "#e6e8ee" : "#1c2230"),
    muted: v("--muted", dark ? "#98a2b3" : "#667085"),
    accent: v("--accent", "#4f46e5"),
    claude: v("--claude", "#c2570c"),
    codex: v("--codex", "#0b7a5b"),
    gemini: v("--gemini", "#2563eb"),
    antigravity: v("--antigravity", "#c026d3"),
    dark,
    palette: dark ? DARK_PALETTE : LIGHT_PALETTE,
  };
}

/** Color for a named series; harness names map to harness colors, others cycle the palette. */
export function colorFor(theme: Theme, name: string, index: number): string {
  if (name === "Claude Code") return theme.claude;
  if (name === "Codex") return theme.codex;
  if (name === "Gemini") return theme.gemini;
  if (name === "Antigravity") return theme.antigravity;
  if (name === "Other") return theme.muted;
  return theme.palette[index % theme.palette.length];
}

/** Calls cb when the OS color scheme changes. Returns unsubscribe. */
export function onThemeChange(cb: () => void): () => void {
  const mq = query();
  const handler = () => cb();
  mq.addEventListener("change", handler);
  return () => mq.removeEventListener("change", handler);
}
