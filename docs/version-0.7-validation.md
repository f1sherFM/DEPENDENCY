# DEPENDENCY 0.7.0 — validation

## Implementation boundaries

Git integration is read-only and local. `git2` 0.21 is built with `default-features = false`; HTTPS/SSH transports are not enabled. Production does not spawn Git CLI, checkout files, install packages or execute source/configuration scripts. Git CLI is used only by an explicit fixture test as an independent status oracle.

Raw commit blobs are materialized into a private `TempDir`; existing parsing, resolution and diagnostics operate on that bounded input. A fenced resolver filesystem denies lexical paths outside staging; staging never contains links. Capture owns inventory and logical project identity. Temporary paths are sanitized from saved resolver evidence. This reuses the existing engine instead of introducing a second historical analyzer or a complete VFS rewrite.

SQLite schema 2 stores basis metadata plus file inventory/import facts in the same transaction as the snapshot/cache. Commit snapshots do not advance the latest working snapshot. Schema migration is transactional; legacy provenance stays unknown. Parser version remains `js-ts-parser-2`; resolver version becomes `resolver-3` because config candidate normalization/boundary checks changed. Existing unchanged AST results remain reusable.

The fixed repository-only policy excludes generated/dependency directories and does not apply user/global ignore rules to commit content. Worktree candidates come from tracked index entries and non-ignored untracked paths. Thus tracked ignored files can be captured. Parent configuration outside a selected subproject is intentionally unavailable. Package graphs describe captured declarations/workspaces rather than the currently installed dependency tree.

## Automated checks

Run from the repository root:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm test
npm run build
```

The Git fixtures exercise independent staged/unstaged states, unborn/detached HEAD, conflict stages, untracked/ignored files, immutable commit configuration versus dirty host files, resolver isolation, captured-byte race detection, cancellation and its commit boundary, subproject scopes, linked worktrees, nested repositories, unsafe paths, case collisions, symlinks/gitlinks/LFS, missing objects and oversized blobs. Storage checks cover schema 1 migration, failed inventory-write rollback, latest-pointer preservation, commit-only restore and Impact from stored graphs after source removal. Comparison checks cover exact moves, duplicate ambiguity, line shifts, unresolved targets, changed rules and config-only resolution changes with reused AST results.

Export real production API results and verify status against Git CLI:

```powershell
$env:DEPENDENCY_DUMP_DIR = "$env:TEMP\dependency-070-check"
cargo test --manifest-path src-tauri/Cargo.toml export_git_ui_and_check_local_oracle -- --ignored --nocapture
node tests/ui-smoke.mjs "$env:DEPENDENCY_DUMP_DIR\git-ui.json" 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
```

The browser test uses real exported snapshots, inventory, statuses and comparisons. Tauri IPC and job timing are mocked in the browser; native jobs/cancellation/storage are checked by Rust tests. It covers status, cancel/complete, selected commit, comparison filtering, historical graph navigation, preserved comparison state, inventory and existing graph/diagnostics/legacy scenarios. This is not a claim of automated native WebView2/folder-picker testing.

Build the Windows installer with `npm run tauri build`.

On the Windows development machine, the regular suite passed 47 Rust tests (3 explicit profiling/export tests excluded) and all 11 Node graph tests. Clippy with warnings denied, TypeScript/Vite build and both Git/previous Impact browser scenarios passed. The explicit Git oracle/export test also passed. A read-only run on DEPENDENCY itself captured/analyzed HEAD in about 2.7 s, working content in 1.5 s and compared them in 30 ms; 50 of 54 source parses were reused. The selected `src` subproject also completed independently (about 0.3/0.4 s and 14 ms). These are observations on this checkout, not performance guarantees.

## Limits and deliberate omissions

- 100,000 capture/tree/status entries, 10 MiB per captured file, 512 MiB total. Oversized input fails rather than publishing a truncated graph.
- Snapshot JSON and inventory are read independently with 64 MiB bounds. Comparisons cache one result, cap changes at 250,000 and return pages of at most 200. These are input/work limits, not a total process-memory guarantee.
- Worktree consistency is a double verification, not an atomic snapshot. Git status is a separate timestamped observation. It cannot prove that a file never changed and reverted between checks.
- Cancellation is cooperative between stages/files/imports. A single resolver operation and SQLite commit are not interruptible; publication is serialized with cancel acceptance.
- Symlinks, junction traversal, submodules and LFS content are not resolved. Omitted entries reduce coverage; Windows-unrepresentable paths fail. Git SHA-256 object format is not promised by this release; adapter errors are surfaced.
- Exact-content move matching does not infer rename+edit or pick among duplicate copies. Changed edge location alone is not changed identity. Parse errors and ambiguous requests cannot prove import removal.
- Diagnostic comparisons require compatible profiles/versions and identical effective rule configuration. Metrics remain observations of each side's coverage, not a risk score or proof of architectural improvement.
- No staged/index analysis, merge-base comparison, automatic fetch, GitHub API, checkout, writer operations, symbol lineage or execution of project tests.
