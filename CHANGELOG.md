# Changelog

## 0.3.0

### Added

- Track Gemini CLI sessions from JSONL and legacy JSON logs, including model pricing, source filtering and configurable log directories.
- Track Antigravity transcripts, including per-step model attribution, pricing, source filtering and configurable log directories.
- Group Codex subagent sessions under their invoking session, including nested workers.
- Paginate the session table with selectable page sizes and persistent filtered totals.

### Changed

- Widen the time-series chart and remove the separate type chart.
- Simplify session table columns and display start times in 24-hour format.
- Normalize Antigravity model names by removing reasoning-effort suffixes.

### Fixed

- Skip malformed Gemini log lines, resolve project roots and deduplicate legacy and JSONL session copies.
- Deduplicate Antigravity steps, resolve quoted working directories and reparse when conversation database WAL files change.
- Show transcript badges using the session source and align the session table footer and paginator.
