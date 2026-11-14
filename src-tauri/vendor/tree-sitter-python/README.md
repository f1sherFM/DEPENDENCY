# DEPENDENCY Python grammar patch

Vendored from tree-sitter-python v0.25.0, upstream commit
`293fdc02038ee2bf0e2e206711b69c90ac0d413f`.
The published crate checksum was
`6bf85fd39652e740bf60f46f4cda9492c3a9ad75880575bf14960f775cb74a1c`.
Upstream: https://github.com/tree-sitter/tree-sitter-python
License: MIT; the original LICENSE and scanner/bindings attribution are preserved.
Local crate version: `0.25.0-dependency.1`, not an upstream release.

## Reason and scope

Upstream issue: https://github.com/tree-sitter/tree-sitter-python/issues/346
PEP 696: https://peps.python.org/pep-0696/

The patch changes declaration type-parameter lists for classes, functions and
type aliases to allow defaults. Ordinary generic type applications retain the
upstream rule; `list[T = int]` is not enabled. Type alias left-hand sides retain
their `type` / `generic_type` AST shape. Scanner, ordinary Python expressions,
import rules, Rust and JS/TS grammars are unchanged.

The package.json only makes grammar.js CommonJS inside DEPENDENCY's ESM repo.
Cargo uses committed generated C; normal builds need no Node or Tree-sitter CLI
for grammar generation. Do not preprocess user source or clear ERROR nodes.

## Reproduce

Use Tree-sitter CLI **0.25.10** (ABI 15), then run in this directory:

```text
tree-sitter --version
tree-sitter generate
```

Generated files are src/parser.c, src/grammar.json, src/node-types.json and
src/tree_sitter headers. Repeat generation and compare hashes or git diff;
generation should be deterministic. An upstream unnecessary-conflict warning
for type_alias_statement/primary_expression is harmless.
On Windows, if a compiler/indexer holds parser.c mapped, generate from a copy of
grammar.js, tree-sitter.json and package.json in a fresh temporary directory,
then compare the six generated files byte-for-byte. Do not generate concurrently
with a Cargo build.

Application regressions, from the DEPENDENCY repository root:

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked python_313
```

For upstream corpus regression, check out the pinned upstream commit in a
separate temporary directory, copy this grammar.js and generated src files over
that checkout, and run `tree-sitter test --rebuild --overview-only` there.
The patched parser passed all 117 upstream corpus cases plus highlight/tag
tests. No upstream test expectations were updated. New syntax/negative cases,
import locations, bindings, cache and snapshot tests live in DEPENDENCY.

## Removal

When an upstream release supports PEP 696, test it against the same regressions
and corpus, replace the path dependency with the pinned registry dependency,
remove this directory/resource entry and bump only the Python parser version.
Do not silently change the parser behind an existing cache version.
