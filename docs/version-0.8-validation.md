# DEPENDENCY 0.8.0 — implementation and validation

## What works

The shared engine reads JS/JSX/TS/TSX, Rust and Python with Tree-sitter. JS/TS retains its existing resolver and parser cache version. Rust/Python parse facts preserve language context in SQLite cache and immutable Git snapshot inventory. Resolution runs again over current captured manifests/settings without reparsing unchanged source files.

Rust discovery uses captured Cargo.toml packages. Standard lib/bin roots, explicit lib/bin paths, inline/external modules, ordinary literal path attributes, grouped/aliased use requests, explicit local declarations and direct captured path dependencies are supported within the conservative resolver. Workspace dependency metadata can be inherited. A physical file may have several target/module contexts; disagreement yields ambiguity. Reexports are observations of use declarations, not a complete exported-symbol index. Macro expansion, active cfg evaluation, generated files and compiler type/name resolution are not performed.

Python resolves indexed .py modules and package initializer dependencies within flat/src or explicitly configured source roots. Conflicting roots, module-vs-package candidates and module files shadowing package hierarchies do not choose arbitrary targets. Relative levels and aliases are parsed; conditional/function-local imports have a separate kind. `from pkg import name` confirms only the known base module/package and reports that name may be an attribute. Runtime hooks, dynamic imports, sys.path manipulation, stdlib version classification and import-name-to-distribution mapping are not inferred from the host environment.

The Packages view includes npm, Cargo and PyPI declarations with their source manifests. Cargo dependencies have scoped ecosystem IDs; Python project dependencies/optional groups and simple requirements/include files are read as data. Direct URLs/local paths without a confirmed distribution name are not invented PyPI packages. Unsupported directives/dynamic metadata become coverage observations. No packages are installed or fetched.

## Storage and semantic boundaries

SQLite schema remains 2. Existing result JSON and imports JSON support backward-compatible optional profile/facts fields, so a new migration/table would add no value. Old snapshots open without newly invented metadata. Stored parse versions are read and written per file; changing a Python parser does not invalidate JS/TS/Rust AST cache entries.

Profiles record graph semantics, per-language parser/resolver versions, effective Python roots and limitations. Comparison checks structured descriptors; different/unknown coverage remains observed-only. Native facts cannot trigger the JS/TS-specific removed-import proof. AST fact lists include module/item declarations, so the inventory column is labelled facts rather than claiming every record is an import.

Import, module-declaration, conditional-import and external-package relations remain distinct. Structural/conditional relations do not silently enter SCC/coupling. Impact supports the known relation kinds explicitly; file reachability is not proof that a change breaks consumers. With native languages present, no-known-consumers is conservatively skipped: incomplete compiler/runtime knowledge cannot establish unused code.

Git capture uses the existing raw-blob/private-directory pipeline and new capture policy v2, excluding Python environments/caches. Old capture policies remain distinct. Cancellation is checked between traversal, parse/resolution and capture stages; individual parser/resolver/SQLite operations remain cooperative boundaries rather than interruptible processes.

## Checks

The complete run passed 58 Rust tests (3 explicit profiling/export checks excluded), all 11 Node graph tests, Clippy with warnings denied, formatting, TypeScript/Vite, and mixed Git plus previous Impact browser scenarios in Chrome. Explicit Git oracle/mixed export and a separate Python-project read-only profile also passed. Windows NSIS/MSI bundles were built; the final source state is rebuilt after the checks.

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
npm test
npm run build
git diff --check
```

Native regressions cover mixed graph targets/kinds, independent cache reuse and invalidation, Cargo workspace dependency metadata, direct local path dependency item resolution, Python root collisions/shadowing, from-attributes, cfg gaps, unsupported Rust path literals, requirements scope boundaries/includes, stored facts/profiles, snapshot restore/comparison/Impact and sentinel build.rs/setup.py that must never execute. The 0.1–0.7 suites remain in the full run.

Reproduce mixed Git UI data and read-only profile:

```powershell
$env:DEPENDENCY_DUMP_DIR = "$env:TEMP\dependency-080-check"
$env:DEPENDENCY_MIXED_FIXTURE = Join-Path (Get-Location) 'src-tauri/tests/fixtures/native_mixed'
$env:DEPENDENCY_PROFILE_ROOTS = (Get-Location).Path
cargo test --manifest-path src-tauri/Cargo.toml export_git_ui_and_check_local_oracle -- --ignored --nocapture
node tests/ui-smoke.mjs "$env:DEPENDENCY_DUMP_DIR\git-ui.json" 'C:\Program Files\Google\Chrome\Application\chrome.exe'
```

The UI test consumes real exported graph, statuses, comparison and inventory; only Tauri transport/job timing is mocked. It also checks language-profile presentation. Rust tests exercise native jobs and persistence. Browser testing does not claim automated native folder-picker/MSI-install verification.

Read-only observations on the development machine: DEPENDENCY HEAD analysis about 7.4 s, working capture/analysis about 7.2 s, comparison 130 ms, 80 source files with 56 cached parses reused. A separate Python project completed in about 4.4 s with 88 sources and 162 confirmed graph edges. Unresolved environment/symbol requests remained visible (133 coverage observations), not guessed connections. These measurements describe the checked working state, not performance guarantees.

The installed Edge launcher exited before exposing a debugging endpoint. Browser scenarios ran successfully with installed Chrome; the harness now reports early browser exit instead of waiting for a misleading timeout.

## Deliberately incomplete language semantics

- Rust: complete workspace selection, tests/examples/benches discovery, target-specific dependency tables, cfg/feature evaluation, inherited path-dependency source mapping, macro/generated items, raw/complex path attributes and arbitrary reexport/glob/shadowing chains are not compiler-equivalent. Unsupported cases are retained as facts/coverage limitations. Edition-sensitive name resolution is not promised.
- Python: ordinary explicit module imports have the strongest coverage. From-attributes, namespace-package portions, executable packaging metadata, runtime import aliases/hooks and installed package versions remain limited. Framework/plugin entry points and sufficiency of test candidates are unknown.
- No inferred JS↔Rust Tauri command, Python↔Rust binding or FFI edge is created by matching names.
- New profiles use conservative whole-profile comparison; per-language partial comparison is deferred.
- Existing Git input limits and 64 MiB snapshot/inventory read bounds remain. Rust module contexts are capped per target; requirements includes are bounded and visited once. These bounds do not guarantee total process-memory usage.

Windows artifacts are produced with `npm run tauri build` in the standard NSIS/MSI output directories.
