# DEPENDENCY

DEPENDENCY is a local desktop tool for exploring software architecture, dependency graphs, code health, and the impact of changes.

The initial target is Windows 10/11. The application uses Tauri 2, React, TypeScript, and Rust. Analysis is local and never executes the inspected project's code.

## Project documents

- [Product specification](docs/product-spec.md)
- [Technical architecture](docs/architecture.md)

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

Python, Rust, package lockfile resolution, arbitrary executable bundler aliases, Git comparisons, scan cancellation, and full impact analysis are not implemented yet. Metrics describe the files and edges that this analyzer can verify; they do not imply analysis of unsupported languages.
