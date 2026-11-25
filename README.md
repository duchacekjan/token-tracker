# Token Tracker

Token Tracker is a desktop app that reads session logs from Claude Code and OpenAI Codex CLI and shows token usage and estimated USD cost for each session.

It is built with Tauri v2 (Rust backend), Svelte 5 and TypeScript. The app only reads log files.

## What it does

- **Session list.** Shows the sessions found in the log directories, with token totals and estimated cost.
- **Session detail.** Shows the token usage of one session.
- **Settings.** Adds extra log directories for Claude Code or Codex CLI.
- **Pricing refresh.** Downloads the current model price table. See [Pricing](#pricing).

## Prerequisites

- Rust stable, installed with [rustup](https://rustup.rs/).
- Node.js 20 or newer, with npm.
- Tauri v2 system dependencies for your OS. See https://v2.tauri.app/start/prerequisites/.

On Linux, the Tauri v2 prerequisites include these packages (Debian/Ubuntu names):

```sh
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  librsvg2-dev build-essential libssl-dev pkg-config
```

## Development

```sh
npm install

npm run tauri dev     # run the desktop app in development mode
npm run dev           # browser preview with mock data (no Tauri backend)
npm run check         # svelte-check type checks
cd src-tauri && cargo test

npm run tauri build   # build a release bundle for your platform
```

`npm run dev` starts Vite only. When the page is not running inside Tauri, the frontend uses mock data from `src/lib/mock.ts`.

## Where logs are read from

**Claude Code**

- If `CLAUDE_CONFIG_DIR` is set and not empty, it can hold several comma-separated directories. For each one, the app reads `<dir>/projects`.
- Otherwise, the app reads `~/.config/claude/projects` and `~/.claude/projects`.

**Codex CLI**

- If `CODEX_HOME` is set and not empty, the app reads `$CODEX_HOME/sessions` and `$CODEX_HOME/archived_sessions`.
- Otherwise, the app reads `~/.codex/sessions` and `~/.codex/archived_sessions`.

Only directories that exist are used. Each one is scanned recursively for files ending in `.jsonl` (case-insensitive). Symbolic links are not followed.

**Extra directories**

Settings lets you add more directories for Claude Code or Codex CLI. An extra directory is scanned as given, recursively. The app does not add `projects` or `sessions` to it. Duplicate and missing paths are ignored.

Extra directories are stored in `settings.json` in the app config directory:

```json
{
  "extraClaudeDirs": ["/mnt/backup/claude-logs"],
  "extraCodexDirs": []
}
```

If `settings.json` is missing or invalid, the app uses no extra directories.

## How tokens are counted

**Claude Code** (`src-tauri/src/parsers/claude.rs`)

- Reads `assistant` entries that have `message.usage` and `message.model`. Entries with the model `<synthetic>` are skipped.
- Counts `input_tokens`, `output_tokens`, `cache_creation_input_tokens` and `cache_read_input_tokens`.
- Entries that repeat the same response are merged. The key is `message.id` plus `requestId`. Within one file, the last entry's usage is kept, at the position of the first entry.
- Deduplication runs per file.
- The session ID is the first `sessionId` in the file, or the file name if there is none. The project is the first `cwd`.

**Codex CLI** (`src-tauri/src/parsers/codex.rs`)

- Reads `token_count` events. Uses `last_token_usage` when present. Otherwise, uses the difference from the previous `total_token_usage`. Repeated identical totals are skipped.
- OpenAI's `input_tokens` includes cached input. The app counts `input` as input minus cached input, and counts the cached part as cache read.
- `output_tokens` already includes reasoning tokens. Reasoning is reported separately and is not added again to output or cost.
- The model comes from the latest `turn_context`, then from `session_meta`, then is `unknown`.
- The session ID comes from `session_meta`. If that is missing, it comes from the UUID at the end of the file name, or else the file name.

## Pricing

Prices come from LiteLLM's [`model_prices_and_context_window.json`](https://github.com/BerriAI/litellm).

- **On startup**, the app loads the cached copy (`litellm_prices.json` in the app cache directory). If there is no cache, it uses the bundled snapshot at `src-tauri/resources/bundled_prices.json`. If the cache cannot be read, it uses the bundled snapshot and reports the error.
- **Refresh prices** downloads the current file from GitHub (20-second timeout) and saves it to the cache.
- Entries without both an input and an output price are ignored. A missing cache-write or cache-read price uses the input price.

**Model name matching** is case-insensitive:

1. The exact name.
2. The name without trailing tags such as `[1m]`.
3. The name without a trailing `-YYYYMMDD` date.
4. If none of these match, the longest price key (at least 5 characters) that the name starts with.

Steps 1 to 3 also try the `anthropic/` and `openai/` prefixes.

**Cost formula**

```
cost = input × input price
     + output × output price
     + cache creation × cache-write price
     + cache read × cache-read price
```

Prices are flat per token. Tiered long-context prices are not applied.

**Costs are estimates.** They use API list prices. Subscription plans are not billed per token, so the totals will not match a subscription bill. Models without a price show `—`.

## Project layout

```
.
├── LICENSE
├── package.json
├── src/                          Svelte 5 frontend
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── resources/
    │   └── bundled_prices.json   fallback price table
    └── src/
        ├── lib.rs                app setup and command registration
        ├── commands.rs           Tauri commands called by the frontend
        ├── discovery.rs          log directories, JSONL scan, settings
        ├── model.rs              shared data types
        ├── pricing.rs            price table, lookup and cost
        └── parsers/
            ├── claude.rs         Claude Code log parser
            └── codex.rs          Codex CLI log parser
```

## License

BSD 3-Clause. See [LICENSE](LICENSE).
