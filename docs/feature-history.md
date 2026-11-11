# Подробная история возможностей

Предыдущий подробный README сохранён здесь как справочник по этапам 0.1–0.8. Текущие инструкции: [README](../README.md), [разработка](development.md).

# DEPENDENCY

DEPENDENCY is a local desktop tool for exploring software architecture, dependency graphs, code health, and the impact of changes.

The initial target is Windows 10/11. The application uses Tauri 2, React, TypeScript, and Rust. Analysis is local and never executes the inspected project's code.

## Project documents

- [Product specification](product-spec.md)
- [Technical architecture](architecture.md)

## Run on Windows

Install Node.js, Rust, the Microsoft C++ Build Tools, and WebView2. Then run:

```powershell
npm install
npm run tauri dev
```

To build the Windows desktop bundle:

```powershell
npm run tauri build
```

## First working slice

- Open a local project folder with the native Windows folder picker.
- Index files while respecting `.gitignore` and skipping generated/dependency directories.
- Parse JavaScript, JSX, TypeScript, and TSX with Tree-sitter without executing project code.
- Resolve relative source imports and imported local assets, and identify npm packages declared in the root `package.json`.
- Explore the file/package graph, inspect incoming and outgoing links, and see strongly connected dependency groups.

## Version 0.2: persistent analysis snapshots

- Projects and complete analysis snapshots are stored in a local SQLite database in the application's Windows data directory (`%APPDATA%\com.f1sherfm.dependency\dependency.sqlite3`). The latest project snapshot is restored automatically when the app starts; saved projects and earlier snapshots can also be opened in the sidebar.
- A snapshot is published only after the graph has been built. The snapshot row, parser cache updates, and latest-snapshot pointer commit in one SQLite transaction, so a failed scan or write leaves the last successful snapshot available.
- Each snapshot records analyzer and diagnostic-rules versions. Parsed imports are cached per project file by SHA-256 content hash and analyzer version; unchanged file contents reuse cached parser results while file resolution and project-level diagnostics run again.
- Project source is read and parsed as data only; DEPENDENCY does not execute inspected project files or package scripts.

## Version 0.3: project-aware module resolution

- The resolver follows `tsconfig.json` and `jsconfig.json` `baseUrl`/`paths`, inherited configuration, JS/TS extension substitution, package `exports`/`imports`, and distinct `import`/`require` conditions.
- Monorepo workspace packages resolve through installed workspace links or unique `workspaces` declarations, including their export maps. Duplicate workspace names are reported as ambiguous and are not turned into guessed graph edges.
- Resolution failures include the source location, resolver reason, candidate/config paths when available, and a targeted hint. URL imports are identified separately from unresolved local imports.
- Resolver and diagnostic rules have their own snapshot version. AST cache version is unchanged, so resolver-only changes reuse stored parse results.
- Node module resolution is implemented by the MIT-licensed `oxc_resolver` Rust crate. DEPENDENCY reads JSON configuration and package metadata without executing project or bundler configuration code.

## Version 0.4: interactive graph exploration

- Directory view groups files by their next path segment. Click a directory to drill down, use Up to return, or switch to the file view. Arrows preserve direction and import kind; their counts represent original imports. Imports inside collapsed directories and across view boundaries are counted separately.
- Selecting a file or package opens its immediate incoming/outgoing neighborhood. The context panel lists source locations and allows navigation to connected nodes; longer lists load 50 links at a time. This is one-hop exploration, not transitive impact analysis.
- Search matches file paths before grouping. Filter by import kind or hide external packages. Filters affect the current graph view, not the stored snapshot.
- Drag the canvas background to pan, use the wheel/buttons to zoom, and Fit to frame the current graph. Connected nodes are arranged in breadth-first columns; arrowheads indicate import direction.
- Rendering is bounded to 200 nodes and 1,000 aggregated arrows. Omitted nodes/imports are explicitly counted; narrow the search, drill into a directory, or focus a node to inspect large projects. The complete snapshot remains available in SQLite.
- No schema or analyzer version change is required. Existing snapshots open in the new graph view without rescanning.

Run `npm test` (Node.js 22.18+ or 24+) for graph projection, aggregation, cycle, navigation, filter, and large/dense graph regressions; `npm run build` checks TypeScript and builds the UI. Engine tests run with `cargo test` in `src-tauri`.

## Version 0.5: evidence-based architecture diagnostics

- Rules run over the full snapshot, independently of graph display limits. Problems separates architectural warnings from analysis coverage and includes info findings, rule/severity/category filters, saved settings, and skipped-rule reasons.
- Cycles show strongly connected components plus one actual closed route with original import types and source lines. Static and full (including type-only/dynamic) graphs are distinguished. Legacy snapshots show SCC membership without inventing an import sequence.
- Fan-out and fan-in count unique internal file neighbors, excluding repeated imports, self references, type-only edges and npm packages. Initial advisory thresholds are **more than 20** dependencies and **more than 30** consumers; high fan-in is informational. These defaults are conservative review prompts, not universal quality standards.
- Files without known consumers are informational candidates, never proven unused code. Declared package entry points, tests, declarations and conventional config files are excluded. The rule is skipped if parsing, source reading or resolution is incomplete. Framework conventions, ignored files and computed imports remain limitations; emitted `dist` entry points are not automatically mapped back to `src`.
- Rules and effective configuration are saved with each atomic snapshot. Historical diagnostics are not silently recomputed. `js-ts-parser-2` fixes type-only reexports and inline type specifiers; the first new analysis refreshes old parser caches once. Later rule/config changes reuse unchanged AST results.

Optional `dependency.config.json` at the project root (strict JSON, maximum 64 KiB):

```json
{
  "rules": {
    "high-fan-out": { "threshold": 20, "exclude": ["src/generated/**"] },
    "high-fan-in": { "threshold": 30 },
    "no-known-consumers": { "exclude": ["src/routes/**", "src/main.tsx"] },
    "dependency-cycle": { "enabled": true },
    "resolution": { "enabled": true },
    "parse-error": { "enabled": true }
  }
}
```

All rules are enabled by default. Unknown fields/rules, invalid patterns and invalid thresholds fail analysis before publishing a snapshot. Exclusions affect findings, not the underlying graph; a cycle is excluded only when every SCC member matches that rule's exclusion. Thresholds apply only to fan-in/out. Disabling coverage diagnostics does not hide coverage gaps from rule eligibility or snapshot metadata.

Diagnostics can open their complete set of nodes in the graph and return to Problems. Long evidence/member lists and result lists load in batches; graph display limits remain explicit. The engine does not execute the inspected project's code or config.

Implementation notes and validation: [0.5 design](version-0.5-design.md), [0.5 checks](version-0.5-validation.md). Optional browser UI checks use Node's built-in WebSocket client and an installed Chromium browser, with real engine JSON and mocked Tauri IPC; see the validation document.

## Version 0.6: file impact analysis

- Select a local file or resource in the graph and choose **Анализ влияния**, or open that section and search for up to 32 seed files. Impact runs over the full saved snapshot, including historical snapshots whose project folder no longer exists. It never rescans or executes inspected code.
- Reverse breadth-first search finds direct and transitive consumers. Each result has a minimum distance and one deterministic shortest chain in the original import direction: consumer → dependency → seed. Seeds are excluded from affected counts, including when cycles return to them.
- Choose all known local import kinds, static imports, individual kinds, or a maximum depth. Search and test-candidate filters only affect the result list. File names identify potential tests; this does not establish which tests are sufficient or executable.
- Pages and proof segments contain at most 200 items. Open a page or proof segment in the graph, with seed files highlighted separately. Counts for computed results and hidden drawing elements remain distinct.
- Work budgets: 100,000 visited nodes, 1,000,000 inspected local edges and 2 seconds of traversal. Depth/work limits produce an explicit partial result with lower-bound counts. Reading/indexing and sorting are measured separately from the traversal deadline; these are not full-request latency guarantees.
- One snapshot index and one query result are cached in memory. Repeating identical parameters reuses the result; replacing it expires old query IDs. Cache data is derived from SQLite and does not modify snapshots, AST caches or rule results. Parser/rules/schema versions are unchanged from 0.5.
- Snapshot JSON reading is bounded to 64 MiB and the index to a conservative 256 MiB estimate; oversized data returns an explicit error rather than a silently truncated graph. These limits do not cap total process memory.

Coverage gaps and legacy analyzer versions are visible. A reachable consumer **may** be affected; file reachability cannot predict whether a particular export change breaks it. Package seeds, symbol-level impact, Git diff and execution of tests are outside 0.6. Implementation and reproducible checks: [design](version-0.6-design.md), [validation](version-0.6-validation.md).

## Version 0.7: local Git integration and snapshot comparison

- The Git section shows HEAD/branch, independent staged and unstaged statuses, untracked files and conflicts. Local refs and commit history are paged. The engine uses `git2`/libgit2 with network transports disabled; Git CLI is not required at runtime.
- Analyze a local commit directly from raw Git blobs, without checkout, hooks, filters or executing project code. Working-directory analysis captures actual files, including allowed untracked content, then checks the file set, HEAD/index fingerprints and bytes again. This detects common concurrent edits; it is explicitly not an atomic filesystem snapshot or a staged/index analysis.
- Git inputs use the same isolated `repository-only-v1` resolution profile. Current installed `node_modules` and configuration outside the selected project scope do not contaminate historical results. Undeclared installed packages and executable bundler configuration remain outside this profile.
- SQLite schema 2 adds immutable per-snapshot inventory and parsed import facts, independently of the mutable AST cache. Migration and snapshot publication are transactional. Old snapshots retain unknown provenance; analyzing a commit preserves the latest working-snapshot pointer. Commit-only projects also reopen from saved history.
- Compare two saved snapshots of one project: content/mode changes, globally unique exact-content moves, dependency counts, confirmed edges and compatible diagnostics. Duplicate-content moves remain ambiguous. A line-number change does not create a false dependency change. Missing resolution is distinguished from an import actually absent in the new AST.
- Explore either side in its historical graph or run Impact Analysis in that specific snapshot. Results stay available after restart without the original files. Comparisons are derived on demand, with paged lists and explicit profile, rule and coverage warnings.
- Git jobs run in the background with progress and cooperative cancellation. Cancellation before publication leaves history unchanged; SQLite commit is the final non-cancellable boundary. Capture limits are 100,000 entries, 10 MiB per file and 512 MiB total; unsupported symlinks/gitlinks/LFS pointers are recorded, never followed or downloaded. Unsafe/unrepresentable Windows paths fail explicitly.

Implementation details and reproducible checks: [0.7 design](version-0.7-design.md), [0.7 validation](version-0.7-validation.md).

## Version 0.8: Rust and Python

- Analyze mixed JS/TS, Rust and Python projects in the shared engine. Native language facts and per-language parser versions persist in cache and immutable snapshot inventory; unchanged JS/TS does not reparse when a Python analyzer changes.
- Rust follows captured Cargo package lib/bin roots, module declarations and conservatively resolved use requests. Direct local path dependencies can link to captured library items. Cargo workspace dependency metadata, aliases and external package declarations are read without running Cargo/build scripts/macros.
- Python follows indexed modules, relative imports and package initializers in flat/src or configured source roots. Ambiguity is explicit. From-import attributes, dynamic paths and distribution-name mapping are not guessed. pyproject.toml and simple requirements/include declarations remain separate from confirmed imports.
- The UI shows the language profile/limits, ecosystem-qualified packages and distinct graph/Impact relation kinds. Structural Rust mod edges are excluded from dependency-cycle/coupling diagnostics. Native no-known-consumers guesses are disabled through explicit coverage limitations.
- Profiles are stored in existing schema-2 JSON without a new migration. Old snapshots stay readable; incompatible profiles compare as observations with warnings. Git excludes Python environments/caches under a new capture policy.

Optional source roots in `dependency.config.json` (alongside existing `rules`):

```json
{ "python": { "sourceRoots": ["src", "tools"] } }
```

Roots must stay inside the selected project scope. This describes the analyzer's static search profile, not an executed Python environment. Detailed supported cases, deliberate limitations and reproducible checks: [0.8 validation](version-0.8-validation.md).

Full Rust compiler name resolution/cfg/macros, Python runtime imports, package lockfile resolution, arbitrary executable bundler aliases, staged/index snapshots, network Git operations and symbol-level impact analysis remain outside this release. Cooperative cancellation applies to Git analysis jobs.
