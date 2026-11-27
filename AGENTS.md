# Agent instructions

- Use Dirigent and Caveman.
- Commit each meaningful unit of work using Caveman Commit.
- Ask the user when requirements or design choices are unclear. Use Grilling for unresolved product decisions.
- For GitHub work, use the GitHub connector or MCP first.
- When addressing pull request findings, resolve all relevant unresolved conversations. For an irrelevant conversation, explain why in a reply and leave it unresolved. After fixing a finding, reply with how it was fixed, then mark its conversation resolved.
- Never bypass pull request rules unless the user specifically asks.

## App versioning and pull requests

- Apply Semantic Versioning to the app version. Use major for incompatible changes, minor for backward-compatible features, and patch for backward-compatible fixes.
- Keep the version synchronized across `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and applicable lockfile entries.
- Before opening a pull request, recommend the appropriate version bump, explain why, state the current and proposed versions, and ask the user to approve the bump or skip it. Wait for the user's choice. An explicit choice already given in the current task is sufficient.
- When approved, update all app version locations and applicable lockfile entries, then commit the version change before opening the pull request. When the user skips, continue with the current version.
