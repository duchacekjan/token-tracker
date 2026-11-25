#!/usr/bin/env node
// Build per-model price timeline from git history of LiteLLM's
// model_prices_and_context_window.json.
//
// Usage: node scripts/build-price-history.mjs --repo <litellm-clone> --out <file>
//   --repo  path to a (blobless ok) clone of BerriAI/litellm with history
//   --out   output JSON path (e.g. src-tauri/resources/price_history.json)
//
// Path history (found via `git log --follow`): the file lived at
// community_resources/max_tokens.json -> cookbook/community-resources/max_tokens.json
// -> litellm/model_prices_and_context_window.json -> model_prices_and_context_window.json (root).
// `--follow` supplies the path each commit used. litellm/model_prices_and_context_window_backup.json
// is a mirror copy and is ignored.
//
// Method: sample last commit of each ISO week, then bisect between samples whose
// tracked-model state differs, so `from` is the exact first commit with the new price.
//
// Limitation: weekly sampling can miss an A->B->A price flip that starts and ends
// inside one week (both week-boundary samples show A, so no bisect happens and B is
// never recorded).
//
// Order: `git log --first-parent` follows mainline only, giving a linear, monotonic history
// for bisect (plain --topo-order interleaves stale side-branch states and yields bogus
// price flip-flops). --diff-merges=first-parent keeps merge commits that change the file.
import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';

const args = process.argv.slice(2);
const arg = (n) => { const i = args.indexOf(n); return i >= 0 ? args[i + 1] : undefined; };
const repo = arg('--repo'), out = arg('--out');
if (!repo || !out) { console.error('usage: build-price-history.mjs --repo <path> --out <file>'); process.exit(2); }

const git = (...a) => execFileSync('git', ['-C', repo, ...a], { encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'ignore'] });

const PREFIXES = ['claude-', 'gpt-5', 'gpt-4.1', 'gpt-4o', 'o1', 'o3', 'o4-mini', 'codex-', 'gpt-6'];
const PROVIDER_PREFIX = 'openai.';
const FIELDS = [
  ['input', 'input_cost_per_token'],
  ['output', 'output_cost_per_token'],
  ['cacheCreation', 'cache_creation_input_token_cost'],
  ['cacheRead', 'cache_read_input_token_cost'],
];

// commits oldest -> newest with the file path at that commit
const raw = git('log', '--first-parent', '--diff-merges=first-parent', '--follow', '--name-only', '--format=@@%H %cI', '--', 'model_prices_and_context_window.json');
const commits = [];
for (const blk of raw.split('@@').slice(1)) {
  const [head, ...rest] = blk.split('\n');
  const [sha, date] = head.split(' ');
  const path = rest.find((l) => l.trim());
  if (path) commits.push({ sha, date, path: path.trim() });
}
commits.reverse();
console.error(`${commits.length} commits`);

const cache = new Map();
function state(i) {
  if (cache.has(i)) return cache.get(i);
  const c = commits[i];
  let s = null;
  try {
    const j = JSON.parse(git('show', `${c.sha}:${c.path}`));
    s = {};
    const lower = new Set(Object.keys(j).map((k) => k.toLowerCase()));
    for (const [k, v] of Object.entries(j)) {
      let key = k.toLowerCase();
      if (key.startsWith(PROVIDER_PREFIX)) {
        const bare = key.slice(PROVIDER_PREFIX.length);
        if (lower.has(bare)) continue; // bare key wins
        key = bare;
      }
      if (key.includes('/') || !PREFIXES.some((p) => key.startsWith(p))) continue;
      if (key.startsWith('gpt-oss')) continue;
      if (!v || typeof v !== 'object') continue;
      // bare keys served via Bedrock are excluded; openai.-prefixed keys are the allowed source
      if (k.toLowerCase() === key && String(v.litellm_provider ?? '').startsWith('bedrock')) continue;
      if (v.input_cost_per_token == null || v.output_cost_per_token == null) continue;
      const e = {};
      for (const [o, f] of FIELDS) e[o] = typeof v[f] === 'number' ? v[f] : null;
      s[key] = e;
    }
  } catch { s = null; }
  cache.set(i, s);
  return s;
}
// state at i; if invalid JSON, fall back to nearest valid commit before i (up to lo)
function valid(i, lo) { for (let k = i; k >= lo; k--) if (state(k)) return k; return -1; }
const sig = (i) => JSON.stringify(state(i));

// weekly samples (last commit per ISO week, by commit date UTC)
function weekKey(d) {
  const t = new Date(d); const x = new Date(Date.UTC(t.getUTCFullYear(), t.getUTCMonth(), t.getUTCDate()));
  const day = x.getUTCDay() || 7; x.setUTCDate(x.getUTCDate() + 4 - day);
  const y0 = new Date(Date.UTC(x.getUTCFullYear(), 0, 1));
  return `${x.getUTCFullYear()}-${Math.ceil(((x - y0) / 864e5 + 1) / 7)}`;
}
const samples = [];
commits.forEach((c, i) => {
  const wk = weekKey(c.date);
  if (samples.length && samples[samples.length - 1].wk === wk) samples[samples.length - 1].i = i;
  else samples.push({ wk, i });
});
let idx = samples.map((s) => s.i);
if (idx[0] !== 0) idx.unshift(0);

const points = new Set(idx);
function bisect(lo, hi) { // lo, hi valid indices with differing state
  if (hi - lo <= 1) { points.add(hi); return; }
  const mid = (lo + hi) >> 1;
  const m = valid(mid, lo);
  if (m <= lo) { // everything in (lo, mid] invalid; probe above
    const m2 = (() => { for (let k = mid + 1; k < hi; k++) if (state(k)) return k; return -1; })();
    if (m2 < 0) { points.add(hi); return; }
    if (sig(m2) !== sig(lo)) bisect(lo, m2);
    if (sig(m2) !== sig(hi)) bisect(m2, hi);
    return;
  }
  if (sig(m) !== sig(lo)) bisect(lo, m);
  if (sig(m) !== sig(hi)) bisect(m, hi);
}
let prev = -1;
for (const i of idx) {
  const v = valid(i, prev < 0 ? 0 : prev);
  if (v < 0) continue;
  if (prev >= 0 && sig(prev) !== sig(v)) bisect(prev, v);
  prev = v;
}

const ordered = [...points].sort((a, b) => a - b).filter((i) => state(i));
const models = {};
for (const i of ordered) {
  const s = state(i);
  for (const [k, e] of Object.entries(s)) {
    const tl = (models[k] ??= []);
    const last = tl[tl.length - 1];
    if (last && FIELDS.every(([o]) => last[o] === e[o])) continue;
    tl.push({ from: commits[i].date, ...e });
  }
}
for (const tl of Object.values(models)) tl.sort((a, b) => new Date(a.from) - new Date(b.from));

const head = git('rev-parse', 'HEAD').trim();
const result = {
  source: 'BerriAI/litellm model_prices_and_context_window.json git history',
  generatedAt: new Date().toISOString(),
  headCommit: head,
  models: Object.fromEntries(Object.entries(models).sort(([a], [b]) => (a < b ? -1 : 1))),
};
writeFileSync(out, JSON.stringify(result));
console.error(`models=${Object.keys(models).length} entries=${Object.values(models).reduce((n, t) => n + t.length, 0)} parsed=${cache.size}`);
