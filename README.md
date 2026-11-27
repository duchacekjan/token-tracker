# Token Tracker

Token Tracker is a desktop app that reads session logs from Claude Code, OpenAI Codex CLI, Gemini CLI and Antigravity and shows token usage and estimated USD cost for each session.

It is built with Tauri v2 (Rust backend), Svelte 5 and TypeScript. The app reads log files. Parsed token data is kept in a local SQLite database.

## What it does

- **Table.** Lists the sessions found in the log directories, with token totals and estimated cost. Claude Code worker transcripts are grouped under their parent session.
- **Charts.** Shows token and cost totals over time, a breakdown by harness, model or project, and a stacked timeline by harness or model. See [Charts](#charts).
- **Shared filters.** The filter bar applies to both tabs. See [Filters](#filters).
- **CSV export.** Writes the filtered sessions, transcripts or turns to a CSV file. See [CSV export](#csv-export).
- **Session detail.** Shows the token usage of one session, broken down by transcript.
- **Settings.** Adds extra log directories for Claude Code, Codex CLI, Gemini CLI or Antigravity.
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
npm test              # vitest unit tests (filters, charts, CSV export)
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

**Gemini CLI**

- If `GEMINI_CLI_HOME` is set and not empty, the app reads `$GEMINI_CLI_HOME/.gemini/tmp`. Otherwise, it reads `~/.gemini/tmp`.
- Session files are named `session-*.jsonl`, or legacy `session-*.json`, anywhere under that directory. No `chats` folder is required.

**Antigravity**

- The app reads `~/.gemini/antigravity` and `~/.gemini/antigravity-acp`.
- Only `brain/<id>/.system_generated/logs/transcript.jsonl` is read. The model and project come from `conversations/<id>.meta` in the same root.

Only directories that exist are used. Each one is scanned recursively for files ending in `.jsonl` (case-insensitive), and Gemini CLI also takes `.json` files. Symbolic links are not followed. Claude Code worker transcripts are found by the same scan. See [Sessions and worker transcripts](#sessions-and-worker-transcripts).

**Extra directories**

Settings lets you add more directories for Claude Code, Codex CLI, Gemini CLI or Antigravity. Extra directories are added to the default directories above. Each one is scanned as given, recursively. The app does not append `projects`, `sessions` or `chats`, so add the directory that contains the `.jsonl` files itself, for example:

- Claude Code: `/path/to/.claude/projects`
- Codex CLI: `/path/to/.codex/sessions`
- Gemini CLI: `/path/to/.gemini/tmp`
- Antigravity: `/path/to/.gemini/antigravity`

Duplicate and missing paths are ignored.

Extra directories are stored in `settings.json` in the app config directory:

```json
{
  "extraClaudeDirs": ["/mnt/backup/claude/projects"],
  "extraCodexDirs": [],
  "extraGeminiDirs": [],
  "extraAntigravityDirs": []
}
```

If `settings.json` is missing or invalid, the app uses no extra directories.

**Reload and local database**

Reload runs the same scan as on load. It finds the log files, syncs them into the database, then builds the session list from the database.

The database is the file `token-tracker.sqlite` in the app data directory. It stores:

- per transcript file: path, source, session ID, project, worker label and agent type, file size, modification time, parser version, and whether the file was on disk at the last scan;
- per turn: timestamp, model, token counts and deduplication key.

It does not store costs. Costs are computed at read time from the price timeline (see [Pricing](#pricing)), so a price refresh also applies to sessions parsed earlier.

A file is parsed again only when its size or modification time differs from the stored values, or when the stored parser version differs from `PARSER_VERSION` in `src-tauri/src/db.rs`. For a transcript in a `subagents` folder, the sibling `.meta.json` size and modification time are compared too. Otherwise its stored turns are used. A path that is not valid UTF-8 cannot be stored, so it is skipped and counted as failed. If parsing fails, the stored data for that file is kept and the file is retried on the next scan.

When a stored file is no longer on disk, for example because Claude Code deleted an old transcript, its data is kept and marked deleted. Its session shows a `log deleted` badge. If the file comes back, it is marked present again. A file that is still on disk but no longer in the scan, for example after its extra directory is removed in Settings, is kept and stays listed.

Session detail uses the groups from the last scan. If the session is not among them, the app runs a scan and looks again.

If the database file cannot be opened, the app handles the failure by its cause.

- **Schema from a newer app version.** The database `user_version` is higher than this build supports. The file is left untouched. The app uses a temporary in-memory database, so nothing is saved, and a warning tells the user to update the app.
- **Other open failures**, for example `file is not a database` or a migration error. The app renames the file to `token-tracker.sqlite.broken-<YYYYMMDD-HHMMSS>` in the same folder. Its `-wal` and `-shm` siblings are renamed the same way. Nothing is deleted. The app then creates a new database, refills it from the logs, and shows a warning banner that explains this. History of logs that were already deleted remains only in the renamed file. If the old file cannot be renamed, or no new database can be created, the app uses an in-memory database and the banner says that persistence is off. Data in that database is lost on exit.

No integrity check runs at startup, so damage deeper in the file surfaces later as a load error in the UI.

## How tokens are counted

**Claude Code** (`src-tauri/src/parsers/claude.rs`)

- Reads `assistant` entries that have `message.usage` and `message.model`. Entries with the model `<synthetic>` are skipped.
- Counts `input_tokens`, `output_tokens`, `cache_creation_input_tokens` and `cache_read_input_tokens`.
- Entries that repeat the same response are merged. The key is `message.id` plus `requestId`. Within one file, the last entry's usage is kept, at the position of the first entry.
- Across Claude Code transcripts, main and worker, a turn whose key already appeared in an earlier transcript is dropped, so the copy in the oldest transcript is kept. Transcripts are ordered by their first turn timestamp, oldest first. Transcripts with no timestamp come first, and ties are ordered by file path. Turns without a key are kept. Transcripts left with no turns are removed.
- Turn-level deduplication across sessions applies to Claude Code only. Codex CLI keeps one transcript per session ID (see below).
- The session ID is the first `sessionId` in the file, or the file name if there is none. The project is the first `cwd`. Claude worker transcripts are grouped with their parent by session ID (see below).

**Codex CLI** (`src-tauri/src/parsers/codex.rs`)

- Reads `token_count` events. Uses `last_token_usage` when present. Otherwise, uses the difference from the previous `total_token_usage`. Repeated identical totals are skipped.
- OpenAI's `input_tokens` includes cached input. The app counts `input` as input minus cached input, and counts the cached part as cache read.
- `output_tokens` already includes reasoning tokens. Reasoning is reported separately and is not added again to output or cost.
- The model comes from the latest `turn_context`, then from `session_meta`, then is `unknown`.
- The session ID comes from `session_meta`. If that is missing, it comes from the UUID at the end of the file name, or else the file name.
- Codex moves rollouts from `sessions/` to `archived_sessions/`, so one session ID can have two copies. One transcript is kept per session ID. The order is: the file still on disk over a log-deleted one, then more turns, then a later last turn, then the smaller file path. The other copy is dropped.
- Codex subagents are detected from `source.subagent.thread_spawn` or `thread_source: "subagent"`. Their `parent_thread_id` links them to the session that invoked them, and `agent_role` supplies the worker label and type. Nested workers stay under the top-level invoking session, while each worker retains its direct parent link. If a parent log is missing, its workers remain visible in a group keyed by that parent ID.

**Gemini CLI** (`src-tauri/src/parsers/gemini.rs`)

- Reads the `gemini` messages that have a `tokens` object. Lines are applied in file order. A line with an `id` replaces the message with that ID, or is added if the ID is new. `$set` replaces the message list or metadata. `$patch` with `removeIds` removes messages. `$rewindTo` drops the messages from that ID on. An unknown ID clears all messages. Lines that are not valid JSON are skipped.
- Legacy `.json` files hold one object with a `messages` array. They are read the same way. A `.json` and a `.jsonl` file with the same session ID are counted once, as for Codex.
- Input is `input` minus `cached`, plus `tool`. Cache read is `cached`. Output is `output` plus `thoughts`, and `thoughts` is also reported as reasoning.
- The session ID is `sessionId`, or the file name without its extension if that is empty. The project is the text of `.project_root` in the `<project>` folder above `chats/`, or else `projectHash`.

**Antigravity** (`src-tauri/src/parsers/antigravity.rs`)

- Reads `PLANNER_RESPONSE` lines. Input is `input_tokens`, cache read is `cache_read_tokens`, and output is `output_tokens`. Cache creation and reasoning are zero.
- Lines with the same `step_index` are merged. The last line's usage is kept, at the position of the first line. A later line with no token fields keeps the earlier usage. Lines without a step index are each kept.
- The session ID is the `<id>` folder name. The model is `model_id` from the `.meta` file, or `unknown`. The project is `cwd` from the `.meta` file, or else the first tool call's `Cwd` argument. A JSON-quoted `Cwd` is unquoted.

## Sessions and worker transcripts

Claude Code runs subagents as separate transcripts. They are stored as `agent-<id>.jsonl` in a `subagents` folder under a folder named after the session ID, with a sibling `agent-<id>.meta.json`.

- **Worker detection.** A transcript is a worker when it lives in a `subagents` directory, or when its entries have `agentId` and the first entry with an `isSidechain` field has `isSidechain` set to `true`. Inline sidechain turns in a main transcript do not make it a worker; they stay in the main transcript. The label and agent type come from the sibling `.meta.json` fields `description` and `agentType`. A missing or invalid meta file leaves both empty.
- **Grouping.** Claude transcripts with the same source and session ID form one session. Within it, the main transcript comes first, then workers by first turn timestamp (no timestamp last), then by file path. Codex groups subagent transcripts under their top-level invoking session using parent IDs. A session can consist of worker transcripts only. Its file path is that of its first transcript.
- **Session list.** One row per session. Tokens, turns and cost are summed over its transcripts. Models are the union. Start is the earliest turn and end is the latest. A `+N workers` badge shows the worker count. The chevron expands one row per transcript, with its label, agent type, models, time range, tokens and cost. Search matches the label, agent ID and agent type of worker transcripts. The `Main` label is not searched. A session shows `log deleted` only when all its transcripts are deleted. Each transcript row shows its own badge.
- **Session detail.** The Transcripts table lists each transcript with its turns, tokens, cost and share of the session cost. The File card shows the main transcript path and the worker count. The Turns table has a Transcript column. When the session has more than one transcript, a filter shows the turns of one transcript. Turns are sorted by time, with turns that have no timestamp last.

## Filters

The Table and Charts tabs share one filter bar above them. The same filters apply to both tabs.

- **Source.** All sources, or only Claude Code, Codex CLI, Gemini CLI or Antigravity.
- **Search.** Matches the project, session ID, model, and the label, agent ID and agent type of worker transcripts. See [Sessions and worker transcripts](#sessions-and-worker-transcripts).
- **From and To.** A local-time date range. Both ends are inclusive. A session is kept when its start time falls on a local day within the range. A session with no start time is left out while either date is set. Clear dates removes both dates.

The count next to the buttons shows how many sessions match, out of the total. Reload runs the scan again. Export CSV opens the export dialog. See [CSV export](#csv-export).

## Charts

The Charts tab draws the sessions that pass the shared filters. Sessions with no start time are not drawn, and the controls note how many were skipped.

The controls are:

- **Bucket.** Day, week or month. Buckets use local time, and weeks start on Monday. Buckets with no sessions between the first and last session are shown as zero.
- **Metric.** Tokens or USD. Tokens is input + output + cache creation + cache read. Reasoning is not added, because Codex reports it inside output tokens. USD is the estimated cost from [Pricing](#pricing). Sessions without a price count as zero.
- **Pie by.** Harness, model or project.
- **Stacked by.** Harness or model.

The pie and the stacked chart show the 10 largest values, and group the rest as `Other`.

The chart grid places the first card across both columns. The breakdown pie and stacked timeline appear side by side below it. The grid uses one column on screens narrower than 900px.

The cards are:

- **Tokens or cost over time.** Bars show the selected metric per bucket. The line shows the other metric on a second axis.
- **Breakdown.** A pie of the selected metric, split by the chosen dimension.
- **Stacked over time.** The selected metric per bucket, stacked by harness or model.

Breakdowns and stacks by model need the turns of each session. When one of these is selected, the app loads the turns of the shown sessions in batches of 100. The card shows `Loading turns…` until this finishes, or the error if it fails.

## Versioning and releases

The app follows Semantic Versioning. Increment the major version for incompatible changes, the minor version for backward-compatible features, and the patch version for backward-compatible fixes. Keep the app version synchronized in `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, along with any applicable lockfile entries.

Before opening a pull request, the contributor should recommend a version bump with its reason and proposed version, then ask the user to choose that bump or skip it. Wait for the choice. If approved, update all app version locations and applicable lockfile entries, then commit the version change before opening the pull request. If the user explicitly skips the bump, continue with the current version.

Releases are started manually from the GitHub Actions Release workflow. The workflow uses the Tauri app version to create a `v<version>` tag.

## CSV export

Export CSV opens a dialog. It starts with the current filter bar values. Changing them in the dialog does not change the main view.

**Sessions**

- **Sources, From, To and Search.** Same meaning as in the filter bar.
- **Models and Projects.** Keep all (the default) or only the ticked values. A project with no `cwd` is listed as `(unknown)`.
- **Include workers.** Off keeps only main transcripts. Session totals are then summed from the main transcripts only.
- **Only priced.** Keeps rows that have a cost.
- **Min cost USD.** Keeps rows with a cost at or above this value. A comma or a point is accepted as the decimal separator. Rows without a cost are left out. An invalid value disables Export.

**Level**

- **Session.** One row per session.
- **Transcript.** One row per transcript, main and worker.
- **Turn.** One row per turn. Before writing, the app loads the turns of the selected sessions. The count shown is an estimate from the session summaries, so it does not apply the model, only-priced or min cost filters.

**Fields**

Each level has its own fields. The Defaults, All and None buttons change the selection. Field names are the CSV column headers.

- **Session.** Default: `source`, `session_id`, `project`, `models`, `started_at`, `ended_at`, `worker_count`, `turn_count`, `input`, `output`, `cache_creation`, `cache_read`, `reasoning`, `total_tokens`, `cost_usd`, `cost_status`. Optional: `project_name`, `duration_sec`, `transcript_count`, `unpriced_models`, `log_deleted`, `file_path`.
- **Transcript.** Default: `source`, `session_id`, `project`, `role`, `agent_label`, `agent_type`, `models`, `started_at`, `ended_at`, `turn_count`, `input`, `output`, `cache_creation`, `cache_read`, `reasoning`, `total_tokens`, `cost_usd`. Optional: `transcript_index`, `agent_id`, `duration_sec`, `cost_status`, `unpriced_models`, `log_deleted`, `file_path`.
- **Turn.** Default: `source`, `session_id`, `project`, `role`, `agent_label`, `timestamp`, `model`, `input`, `output`, `cache_creation`, `cache_read`, `reasoning`, `total_tokens`, `cost_usd`. Optional: `transcript_index`, `priced`.

`role` is `main` or `worker`. `cost_status` is `priced`, `partial` or `unpriced`, as in the session list.

**Format**

- **Delimiter.** Comma (default) or semicolon.
- **Decimal.** `.` or `,`. By default it follows the delimiter: `.` with a comma, `,` with a semicolon. If the decimal separator equals the delimiter, numbers with decimals are quoted, and the dialog shows a warning.
- **UTF-8 BOM.** Off by default with a comma, on by default with a semicolon. The BOM can be switched either way.

**File contents**

- The first line is the header. Every line, including the last, ends with CRLF.
- Quoting follows RFC 4180. A cell that contains the delimiter, a double quote, CR or LF is wrapped in double quotes, and each double quote inside it is doubled.
- Timestamps are ISO 8601 in UTC, for example `2026-11-26T09:30:00.000Z`.
- A missing value is an empty cell. Whole numbers are written as they are. Other numbers keep up to 10 decimal places, without trailing zeros.
- Text cells that start with `=`, `+`, `-`, `@`, tab or CR get a leading apostrophe, so a spreadsheet does not run them as formulas. The header and numeric cells are not changed.

**Saving**

- The app opens a native save dialog from Rust. The default name is `token-tracker-<level>-<YYYYMMDD>.csv`, and the dialog offers CSV files.
- If the chosen name does not end in `.csv`, `.csv` is appended. For example, `report.txt` is saved as `report.txt.csv`.
- The file is written to a temporary file in the same folder and then renamed over the target.
- Cancelling the save dialog writes nothing, and the export dialog stays open. The export dialog closes after a successful save.
- In browser preview (`npm run dev`), there is no native dialog. The browser downloads the file under the default name.

## Pricing

Each turn, including Gemini CLI and Antigravity turns, is priced at the LiteLLM rate that was valid at the turn's timestamp. Rates come from LiteLLM's [`model_prices_and_context_window.json`](https://github.com/BerriAI/litellm), using the same bundled history and Refresh prices flow for every supported harness.

**Price timeline**

Each model has a timeline of price entries. An entry has a `from` date and is valid until the next entry starts.

- The bundled timeline is `src-tauri/resources/price_history.json`. It is built from the git history of LiteLLM's price file. A `from` date is the date of the LiteLLM commit that first had that price. This can lag the vendor's actual price change or model release.
- Refresh prices adds entries to a local timeline. At startup, the local timeline is merged with the bundled one.
- When merging, consecutive entries with the same price are collapsed into one.
- Only these model families are kept: `claude-`, `gpt-5`, `gpt-4.1`, `gpt-4o`, `o1`, `o3`, `o4-mini`, `codex-`, `gpt-6`, `gemini-`. LiteLLM keys that contain `/` are skipped.
- Entries without both an input and an output price are ignored. A missing cache-write or cache-read price uses the input price.

**Which price a turn gets**

- The latest entry whose `from` is on or before the turn's timestamp.
- A turn before the model's first entry uses the first entry.
- A turn with no timestamp uses the latest entry.

**Model name matching** is case-insensitive. The app tries these names in order and uses the first one that has a timeline:

1. The exact name.
2. The name without trailing tags such as `[1m]`.
3. The name without a trailing `-YYYYMMDD` date.
4. The name without a trailing effort suffix (`-minimal`, `-low`, `-medium` or `-high`), tried after the date is removed. Only for `gemini-` models.

There is no other matching. A model that matches none of these is unpriced and shows `—`. Gemini models receive prices when their model names match a bundled or refreshed `gemini-` entry. Unknown or unmatched model names remain unpriced.

**Refresh prices**

- Downloads the current LiteLLM price file from GitHub (20-second timeout). Startup does not download anything.
- For each tracked model that is new, or whose price differs from its latest entry, adds one entry dated with the refresh time.
- Saves the result to `price_history_local.json` in the app cache directory, using a temp file and a rename, and uses it right away. If the save fails, the new prices are still used and the error is reported in the pricing status.
- A corrupt `price_history_local.json` is replaced on refresh. At startup, a corrupt file is ignored, the bundled timeline is used, and the error is reported.
- The pricing status is `online` after a refresh, `cache` when the local file was loaded at startup, and `bundled` otherwise.

**Caveats**

- **Refresh dates.** A price added by Refresh prices gets `from` set to the refresh time, not the date the vendor changed the price. Turns between an actual price change and the next refresh are priced at the old rate. The same applies until an app release regenerates the bundled history.
- **Corrupt local file.** A corrupt `price_history_local.json` is ignored at startup. The bundled timeline is used and the error is shown in the pricing status. The next refresh replaces the file, so earlier local entries are lost.
- **Weekly sampling.** The bundled timeline samples LiteLLM weekly and bisects changes to the exact commit. A price that changes and reverts within one week can be missed.

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

Each turn is priced separately, using the price for its timestamp. Prices are flat per token. Tiered long-context prices are not applied, including for Gemini models.

**Session and total costs**

- A session's cost is the sum of its priced turns across all its transcripts. If no turn has a price, the cost shows `—` and the session is fully unpriced.
- If some turns have a price, the cost is shown with a `partial` badge. The badge tooltip names the unpriced models.
- The footer total sums the sessions that match the current filters (source, search and dates). Fully unpriced sessions are excluded. Partially priced sessions add only their priced turns.
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
│   ├── App.svelte                tabs, shared filter state, export dialog
│   └── lib/
│       ├── api.ts                Tauri calls, with browser fallbacks
│       ├── filters.ts            search and local-time date range
│       ├── charts/               time buckets, breakdowns, ECharts theme
│       ├── components/           tabs, filter bar, export dialog, session detail
│       └── export/               CSV fields, row builder, quoting
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── resources/
    │   └── price_history.json     bundled price timeline
    └── src/
        ├── lib.rs                app setup and command registration
        ├── commands.rs           Tauri commands called by the frontend
        ├── aggregate.rs          cross-session deduplication, worker grouping, summaries and detail
        ├── db.rs                 SQLite store of parsed transcripts (tokens only)
        ├── discovery.rs          log directories, JSONL scan, settings
        ├── model.rs              shared data types
        ├── pricing.rs            price timeline, lookup and cost
        ├── startup.rs            database open and recovery
        └── parsers/
            ├── antigravity.rs    Antigravity transcript parser
            ├── claude.rs         Claude Code log parser
            ├── codex.rs          Codex CLI log parser
            └── gemini.rs         Gemini CLI log parser
```

## License

BSD 3-Clause. See [LICENSE](LICENSE).
