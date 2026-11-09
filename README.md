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

Python, Rust, package lockfile resolution, TypeScript path aliases, Git comparisons, scan cancellation, and full impact analysis are not implemented yet. Metrics describe the files and edges that this analyzer can verify; they do not imply analysis of unsupported languages.
