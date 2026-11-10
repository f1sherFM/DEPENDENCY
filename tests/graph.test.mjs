import { test } from 'node:test';
import assert from 'node:assert/strict';
import { projectGraph, layoutGraph } from '../src/graph.ts';

const node = (path, kind = 'file') => ({ id: path, path, label: path.split('/').at(-1), kind, language: 'TypeScript', lines: 10, incoming: 0, outgoing: 0, declared: null });
const edge = (source, target, kind = 'import', line = 1) => ({ id: `${source}:${target}:${line}`, source, target, kind, line });
const options = { mode: 'directories', directory: '', query: '', showPackages: true, edgeKind: '', focus: null, direction: 'both', limit: 200 };

test('aggregation preserves direction, type and import counts without mutating snapshot', () => {
  const nodes = [node('a/one.ts'), node('a/two.ts'), node('b/index.ts')];
  const edges = [edge('a/one.ts', 'a/two.ts'), edge('a/one.ts', 'b/index.ts'), edge('a/two.ts', 'b/index.ts'), edge('b/index.ts', 'a/one.ts', 'type_only_import')];
  const before = JSON.stringify({ nodes, edges });
  const result = projectGraph(nodes, edges, options);
  assert.equal(result.nodes.length, 2);
  assert.equal(result.internalEdges, 1);
  assert.equal(result.edges.find(e => e.source === 'directory:a').count, 2);
  assert.equal(result.edges.find(e => e.source === 'directory:b').kind, 'type_only_import');
  assert.equal(JSON.stringify({ nodes, edges }), before);
});

test('drilling down exposes children and counts boundary imports', () => {
  const result = projectGraph([node('src/a.ts'), node('src/deep/b.ts'), node('other/c.ts')], [edge('src/a.ts', 'src/deep/b.ts'), edge('src/a.ts', 'other/c.ts')], { ...options, directory: 'src' });
  assert.deepEqual(new Set(result.nodes.map(n => n.id)), new Set(['src/a.ts', 'directory:src/deep']));
  assert.equal(result.boundaryEdges, 1);
  assert.equal(result.edges.length, 1);
});

test('one-hop focus respects direction and excludes neighbors of neighbors', () => {
  const nodes = ['a', 'b', 'c', 'd'].map(p => node(p));
  const edges = [edge('a', 'b'), edge('c', 'a'), edge('b', 'd'), edge('b', 'c')];
  const result = projectGraph(nodes, edges, { ...options, mode: 'files', focus: 'a', direction: 'outgoing' });
  assert.deepEqual(new Set(result.nodes.map(n => n.id)), new Set(['a', 'b']));
  assert.equal(result.edges.length, 1);
  assert.equal(result.edges[0].target, 'b');
});

test('search and filters use actual file paths before aggregating', () => {
  const result = projectGraph([node('src/wanted.ts'), node('src/other.ts'), node('package:react', 'package')], [], { ...options, query: 'wanted', showPackages: false });
  assert.equal(result.nodes.length, 1);
  assert.deepEqual(result.nodes[0].members, ['src/wanted.ts']);
});

test('cycles, self imports and isolated files have finite, distinct positions', () => {
  const result = projectGraph(['a', 'b', 'c'].map(p => node(p)), [edge('a', 'b'), edge('b', 'a'), edge('a', 'a')], { ...options, mode: 'files' });
  const layout = layoutGraph(result.nodes, result.edges, 'a');
  assert.equal(layout.positions.size, 3);
  assert.equal(new Set([...layout.positions.values()].map(p => `${p.x},${p.y}`)).size, 3);
  assert.ok([...layout.positions.values()].every(p => Number.isFinite(p.x + p.y)));
  assert.equal(result.edges.length, 3);
});

test('edge kind filter preserves nodes but excludes other import kinds', () => {
  const result = projectGraph([node('a'), node('b')], [edge('a', 'b'), edge('a', 'b', 'type_only_import')], { ...options, mode: 'files', edgeKind: 'type_only_import' });
  assert.equal(result.nodes.length, 2);
  assert.equal(result.edges.length, 1);
  assert.equal(result.edges[0].kind, 'type_only_import');
});

test('empty view has a valid layout and no phantom links', () => {
  const result = projectGraph([node('a')], [], { ...options, query: 'absent' });
  const layout = layoutGraph(result.nodes, result.edges, null);
  assert.equal(result.nodes.length, 0);
  assert.equal(result.edges.length, 0);
  assert.equal(layout.positions.size, 0);
  assert.ok(layout.width > 0 && layout.height > 0);
});

test('large graph limits expose omitted nodes and imports, retaining focused node', () => {
  const nodes = Array.from({ length: 2500 }, (_, i) => node(`file-${i}`));
  const edges = nodes.slice(1).map(n => edge('file-2499', n.id));
  const result = projectGraph(nodes, edges, { ...options, mode: 'files', focus: 'file-2499' });
  assert.equal(result.nodes.length, 200);
  assert.ok(result.nodes.some(n => n.id === 'file-2499'));
  assert.ok(result.hiddenNodes > 2000);
  assert.equal(result.hiddenEdges + result.edges.reduce((sum, e) => sum + e.count, 0), edges.length);
});

test('dense graph also caps rendered arrows and reports omitted imports', () => {
  const nodes = Array.from({ length: 70 }, (_, i) => node(`f${i}`));
  const edges = nodes.flatMap(a => nodes.map(b => edge(a.id, b.id)));
  const result = projectGraph(nodes, edges, { ...options, mode: 'files' });
  assert.equal(result.edges.length, 1000);
  assert.equal(result.hiddenEdges, edges.length - 1000);
  assert.equal(result.hiddenNodes, 0);
});
