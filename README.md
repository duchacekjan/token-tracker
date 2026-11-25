# Token Tracker

Token Tracker is a desktop app that reads session logs from Claude Code and OpenAI Codex CLI and shows token usage and estimated USD cost for each session.

It is built with Tauri v2 (Rust backend), Svelte 5 and TypeScript. The app only reads log files.

## What it does

- **Session list.** Shows the sessions found in the log directories, with token totals and estimated cost.
- **Session detail.** Shows the token usage of one session.
- **Settings.** Adds extra log directories for Claude Code or Codex CLI.
- **Pricing refresh.** Downloads current model prices and adds changes to the price history. See [Pricing](#pricing).

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

Settings lets you add more directories for Claude Code or Codex CLI. Extra directories are added to the default directories above. Each one is scanned as given, recursively. The app does not append `projects` or `sessions`, so add the directory that contains the `.jsonl` files itself, for example:

- Claude Code: `/path/to/.claude/projects`
- Codex CLI: `/path/to/.codex/sessions`

Duplicate and missing paths are ignored.

Extra directories are stored in `settings.json` in the app config directory:

```json
{
  "extraClaudeDirs": ["/mnt/backup/claude/projects"],
  "extraCodexDirs": []
}
```

If `settings.json` is missing or invalid, the app uses no extra directories.

**Reload and caching**

Reload runs the same scan as on load. The parse result of each `.jsonl` file is kept in memory, keyed by path, modification time and size. A file whose key has not changed is not parsed again. Entries for files that no longer exist are dropped. Deduplication and totals are recomputed on every scan. The cache is not saved to disk, so it is empty after restart.

Session detail uses the result from the last scan. If the file was not in that scan, the app parses that file alone, without cross-session deduplication.

## How tokens are counted

**Claude Code** (`src-tauri/src/parsers/claude.rs`)

- Reads `assistant` entries that have `message.usage` and `message.model`. Entries with the model `<synthetic>` are skipped.
- Counts `input_tokens`, `output_tokens`, `cache_creation_input_tokens` and `cache_read_input_tokens`.
- Entries that repeat the same response are merged. The key is `message.id` plus `requestId`. Within one file, the last entry's usage is kept, at the position of the first entry.
- Across sessions, a turn whose key already appeared in an earlier session is dropped, so the copy in the oldest session is kept. Sessions are ordered by their first turn timestamp, oldest first. Sessions with no timestamp come first, and ties are ordered by file path. Turns without a key are kept. Sessions left with no turns are removed.
- Cross-session deduplication applies to Claude Code only. Codex CLI sessions are not deduplicated.
- The session ID is the first `sessionId` in the file, or the file name if there is none. The project is the first `cwd`.

**Codex CLI** (`src-tauri/src/parsers/codex.rs`)

- Reads `token_count` events. Uses `last_token_usage` when present. Otherwise, uses the difference from the previous `total_token_usage`. Repeated identical totals are skipped.
- OpenAI's `input_tokens` includes cached input. The app counts `input` as input minus cached input, and counts the cached part as cache read.
- `output_tokens` already includes reasoning tokens. Reasoning is reported separately and is not added again to output or cost.
- The model comes from the latest `turn_context`, then from `session_meta`, then is `unknown`.
- The session ID comes from `session_meta`. If that is missing, it comes from the UUID at the end of the file name, or else the file name.

## Pricing

Each turn is priced at the LiteLLM rate that was valid at the turn's timestamp. Rates come from LiteLLM's [`model_prices_and_context_window.json`](https://github.com/BerriAI/litellm).

**Price timeline**

Each model has a timeline of price entries. An entry has a `from` date and is valid until the next entry starts.

- The bundled timeline is `src-tauri/resources/price_history.json`. It is built from the git history of LiteLLM's price file. A `from` date is the date of the LiteLLM commit that first had that price. This can lag the vendor's actual price change or model release.
- Refresh prices adds entries to a local timeline. At startup, the local timeline is merged with the bundled one.
- When merging, consecutive entries with the same price are collapsed into one.
- Only these model families are kept: `claude-`, `gpt-5`, `gpt-4.1`, `gpt-4o`, `o1`, `o3`, `o4-mini`, `codex-`. LiteLLM keys that contain `/` are skipped.
- Entries without both an input and an output price are ignored. A missing cache-write or cache-read price uses the input price.

**Which price a turn gets**

- The latest entry whose `from` is on or before the turn's timestamp.
- A turn before the model's first entry uses the first entry.
- A turn with no timestamp uses the latest entry.

**Model name matching** is case-insensitive. The app tries these names in order and uses the first one that has a timeline:

1. The exact name.
2. The name without trailing tags such as `[1m]`.
3. The name without a trailing `-YYYYMMDD` date.

For each name, the app tries the name as is, then with the `anthropic/` prefix, then with the `openai/` prefix. There is no other matching. A model that matches none of these is unpriced and shows `—`.

**Refresh prices**

- Downloads the current LiteLLM price file from GitHub (20-second timeout). Startup does not download anything.
- For each tracked model that is new, or whose price differs from its latest entry, adds one entry dated with the refresh time.
- Saves the result to `price_history_local.json` in the app cache directory, using a temp file and a rename, and uses it right away. If the save fails, the new prices are still used and the error is reported in the pricing status.
- A corrupt `price_history_local.json` is replaced on refresh. At startup, a corrupt file is ignored, the bundled timeline is used, and the error is reported.
- The pricing status is `online` after a refresh, `cache` when the local file was loaded at startup, and `bundled` otherwise.

**Regenerate the bundled timeline** (maintainers)

1. Make a blobless clone of LiteLLM. The clone keeps the full commit history and fetches file contents as needed:

   ```sh
   git clone --filter=blob:none https://github.com/BerriAI/litellm /path/to/litellm
   ```

2. From the project root, run:

   ```sh
   npm run prices:history -- --repo /path/to/litellm --out src-tauri/resources/price_history.json
   ```

   The script follows the price file through its past paths (`git log --follow`), samples one commit per week, and bisects where tracked prices change. Each `from` is therefore the first commit with the new price.

3. Rebuild the app. The file is compiled in with `include_str!`.

**Cost formula**

```
cost = input × input price
     + output × output price
     + cache creation × cache-write price
     + cache read × cache-read price
```

Each turn is priced separately, using the price for its timestamp. Prices are flat per token. Tiered long-context prices are not applied.

**Session and total costs**

- A session's cost is the sum of its priced turns. If no turn has a price, the cost shows `—` and the session is fully unpriced.
- If some turns have a price, the cost is shown with a `partial` badge. The badge tooltip names the unpriced models.
- The footer total sums the sessions that match the current source filter and search. Fully unpriced sessions are excluded. Partially priced sessions add only their priced turns.
- The footer shows a badge such as `+2 unpriced, 1 partial` when any session is unpriced or partial. Its tooltip explains both counts.

**Costs are estimates.** They use API list prices. Subscription plans are not billed per token, so the totals will not match a subscription bill. Models without a price show `—`.

## Project layout

```
.
├── LICENSE
├── package.json
├── scripts/
│   └── build-price-history.mjs   rebuilds the bundled price timeline
├── src/                          Svelte 5 frontend
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── resources/
    │   └── price_history.json     bundled price timeline
    └── src/
        ├── lib.rs                app setup and command registration
        ├── commands.rs           Tauri commands called by the frontend
        ├── aggregate.rs          cross-session deduplication, summaries and detail
        ├── discovery.rs          log directories, JSONL scan, settings
        ├── model.rs              shared data types
        ├── pricing.rs            price timeline, lookup and cost
        └── parsers/
            ├── claude.rs         Claude Code log parser
            └── codex.rs          Codex CLI log parser
```

## License

BSD 3-Clause. See [LICENSE](LICENSE).
