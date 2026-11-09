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

Python, Rust, package lockfile resolution, TypeScript path aliases, persistent snapshots, cancellation, and incremental rescans are not implemented yet. Metrics describe the files and edges that this first analyzer can verify; they do not imply analysis of unsupported languages.
