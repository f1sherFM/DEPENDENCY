// Read-only adapter over committed snapshot JSON and opt-in UI observations.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

export const dataDirectory = () => process.env.DEPENDENCY_APP_DATA || (process.env.APPDATA && join(process.env.APPDATA, 'com.f1sherfm.dependency'));
function readJson(path, maxBytes) {
  if (statSync(path).size > maxBytes) throw new Error('Local observation exceeds the supported size');
  return JSON.parse(readFileSync(path, 'utf8'));
}
export function requireAccess(directory) {
  if (!directory) throw new Error('Set DEPENDENCY_APP_DATA to the application data directory');
  let config;
  try { config = readJson(join(directory, 'agent-access.json'), 4096); }
  catch (error) { if (error.code === 'ENOENT') throw new Error('Agent access is disabled; run dependency-agent.mjs enable'); throw error; }
  if (config.enabled !== true) throw new Error('Agent access is disabled');
}
const text = { type: 'string', minLength: 1, maxLength: 2000 };
const offset = { type: 'integer', minimum: 0, maximum: 10000000 };
const limit = { type: 'integer', minimum: 1, maximum: 100 };
const snapshot = { snapshotId: text, sessionId: text };
const definitions = [
  ['dependency_get_context', 'Read the real open UI, viewport, graph filters, selected node, runtime errors and observation freshness. Multiple active sessions require sessionId.', { sessionId: text }],
  ['dependency_list_sessions', 'List observations; stale sessions are explicitly marked and are not treated as a live app.', {}],
  ['dependency_list_projects', 'Read saved projects without opening, analyzing or updating last-opened timestamps.', { offset, limit }],
  ['dependency_list_snapshots', 'Page immutable snapshot metadata for a saved project.', { projectId: text, offset, limit }, ['projectId']],
  ['dependency_get_snapshot', 'Read snapshot summary, analyzer profile and coverage. Omitted snapshotId means the observed UI snapshot, never the latest snapshot by assumption.', snapshot],
  ['dependency_get_diagnostics', 'Page saved diagnostics, separate architecture from coverage; evidence is available by diagnosticId.', { ...snapshot, category: { enum: ['architecture', 'coverage'] }, severity: { enum: ['info', 'warning', 'error'] }, ruleId: text, offset, limit }],
  ['dependency_get_diagnostic', 'Read one saved diagnostic; node IDs and each evidence array are paged independently with totals.', { ...snapshot, diagnosticId: text, offset, limit }, ['diagnosticId']],
  ['dependency_get_graph', 'Page actual saved nodes and edges with independent offsets. This is the full snapshot, not the filtered UI projection.', { ...snapshot, nodeOffset: offset, edgeOffset: offset, limit }],
  ['dependency_get_node', 'Read a node and page its saved incoming/outgoing edges. No impact or dependency inference is performed.', { ...snapshot, nodeId: text, offset, limit }],
];
export const toolDefinitions = definitions.map(([name, description, properties, required = []]) => ({ name, description, inputSchema: { type: 'object', properties, required, additionalProperties: false }, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false } }));
function validate(name, args) {
  const schema = toolDefinitions.find(tool => tool.name === name)?.inputSchema;
  if (!schema) throw new Error('Unknown tool');
  if (!args || typeof args !== 'object' || Array.isArray(args)) throw new Error('Arguments must be an object');
  for (const key of schema.required) if (!(key in args)) throw new Error(`Missing ${key}`);
  for (const [key, value] of Object.entries(args)) {
    const rule = schema.properties[key];
    if (!rule || (rule.enum && !rule.enum.includes(value)) || (rule.type === 'string' && (typeof value !== 'string' || value.length < 1 || value.length > rule.maxLength)) || (rule.type === 'integer' && (!Number.isSafeInteger(value) || value < rule.minimum || value > rule.maximum))) throw new Error(`Invalid argument: ${key}`);
  }
}
function page(items, offset = 0, limit = 30) {
  return { items: items.slice(offset, offset + limit), total: items.length, nextOffset: offset + limit < items.length ? offset + limit : null };
}
function sessions(directory) {
  const items = [], warnings = [];
  let files;
  try { files = readdirSync(join(directory, 'agent-sessions')); }
  catch (error) { if (error.code === 'ENOENT') return { items, warnings }; throw error; }
  for (const file of files.filter(file => /^[\da-f-]{36}\.json$/i.test(file))) {
    try {
      const item = readJson(join(directory, 'agent-sessions', file), 140000);
      if (item.schemaVersion !== 1 || item.sessionId !== file.slice(0, -5) || !Number.isSafeInteger(item.observedAt) || !Number.isSafeInteger(item.pid) || !item.state || typeof item.state !== 'object') throw new Error('Unsupported observation format');
      const ageMs = Date.now() - item.observedAt;
      let processAlive = true;
      try { process.kill(item.pid, 0); } catch (error) { processAlive = error.code === 'EPERM'; }
      items.push({ ...item, ageMs, stale: !processAlive || ageMs < 0 || ageMs > 15000 });
    } catch (error) { warnings.push(`${file}: ${error.message}`); }
  }
  return { items: items.sort((a, b) => b.observedAt - a.observedAt), warnings };
}
function context(directory, sessionId) {
  const found = sessions(directory);
  if (sessionId) {
    const item = found.items.find(item => item.sessionId === sessionId);
    if (!item) throw new Error('Session not found');
    return { ...item, warnings: found.warnings };
  }
  const active = found.items.filter(item => !item.stale);
  if (active.length !== 1) throw new Error(active.length ? 'Multiple active windows: use dependency_list_sessions and sessionId' : 'No live observation. Start DEPENDENCY with agent access enabled; saved snapshots can still be queried by explicit ID.');
  return { ...active[0], warnings: found.warnings };
}
function snapshotData(db, directory, args) {
  let id = args.snapshotId;
  if (!id) {
    const observed = context(directory, args.sessionId);
    if (observed.stale) throw new Error('Observation is stale: specify snapshotId explicitly to read saved data');
    id = observed.state.snapshotId;
  }
  if (!id) throw new Error('No snapshot is open');
  const row = db.prepare('SELECT id,project_id,created_at,analyzer_version,rules_version,parsed_file_count,reused_file_count,CASE WHEN length(CAST(result_json AS BLOB))<=67108864 THEN result_json ELSE NULL END AS result_json FROM snapshots WHERE id=?').get(id);
  if (!row) throw new Error('Snapshot not found');
  if (!row.result_json) throw new Error('Snapshot exceeds the supported 64 MiB limit');
  const result = JSON.parse(row.result_json); delete row.result_json;
  if (!Array.isArray(result.nodes) || !Array.isArray(result.edges)) throw new Error('Unsupported snapshot format');
  return { snapshot: row, result };
}
const diagnosticSummary = item => ({ id: item.id, ruleId: item.ruleId, severity: item.severity, category: item.category, confidence: item.confidence, title: item.title, explanation: item.explanation, suggestion: item.suggestion, nodeCount: item.nodeIds?.length ?? 0, evidenceType: item.evidence?.type });
export function callTool(name, args = {}, directory = dataDirectory()) {
  validate(name, args); requireAccess(directory);
  if (name === 'dependency_get_context') return context(directory, args.sessionId);
  if (name === 'dependency_list_sessions') {
    const found = sessions(directory);
    return { ...found, items: found.items.map(({ state, ...metadata }) => ({ ...metadata, projectId: state.projectId, snapshotId: state.snapshotId, view: state.view })) };
  }
  const db = new DatabaseSync(join(directory, 'dependency.sqlite3'), { readOnly: true });
  try {
    db.exec('PRAGMA query_only=ON; PRAGMA busy_timeout=2000');
    if (![2,3].includes(db.prepare('PRAGMA user_version').get().user_version)) throw new Error('Unsupported storage schema: expected version 2 or 3');
    if (name === 'dependency_list_projects') {
      const total = db.prepare('SELECT count(*) AS n FROM projects').get().n;
      const start = args.offset ?? 0, size = args.limit ?? 30;
      return { items: db.prepare('SELECT id,name,root_path,last_opened_at,latest_snapshot_id FROM projects ORDER BY last_opened_at DESC,id LIMIT ? OFFSET ?').all(size, start), total, nextOffset: start + size < total ? start + size : null };
    }
    if (name === 'dependency_list_snapshots') {
      const total = db.prepare('SELECT count(*) AS n FROM snapshots WHERE project_id=?').get(args.projectId).n;
      const start = args.offset ?? 0, size = args.limit ?? 30;
      return { items: db.prepare('SELECT id,project_id,created_at,analyzer_version,rules_version,parsed_file_count,reused_file_count FROM snapshots WHERE project_id=? ORDER BY created_at DESC,id DESC LIMIT ? OFFSET ?').all(args.projectId, size, start), total, nextOffset: start + size < total ? start + size : null };
    }
    const { snapshot, result } = snapshotData(db, directory, args);
    const diagnostics = result.diagnostics ?? [];
    if (name === 'dependency_get_snapshot') {
      const { cycles, resolutionIssues, packages, ...summary } = result.summary;
      return { snapshot, summary, profile: result.analysisProfile ?? null, basis: result.basis ?? null, rulesRun: result.rulesRun ?? null, diagnosticCounts: diagnostics.reduce((counts, item) => { const key = `${item.category}:${item.severity}`; counts[key] = (counts[key] ?? 0) + 1; return counts; }, {}), resolutionIssueCount: resolutionIssues?.length ?? 0, note: 'Saved results are returned unchanged; no re-analysis was performed.' };
    }
    if (name === 'dependency_get_graph') return { snapshotId: snapshot.id, nodes: page(result.nodes, args.nodeOffset, args.limit), edges: page(result.edges, args.edgeOffset, args.limit) };
    if (name === 'dependency_get_diagnostics') return { snapshotId: snapshot.id, ...page(diagnostics.filter(item => (!args.category || item.category === args.category) && (!args.severity || item.severity === args.severity) && (!args.ruleId || item.ruleId === args.ruleId)).map(diagnosticSummary), args.offset, args.limit), legacy: !result.rulesRun };
    if (name === 'dependency_get_diagnostic') {
      const item = diagnostics.find(item => item.id === args.diagnosticId);
      if (!item) throw new Error('Diagnostic not found');
      return { snapshotId: snapshot.id, ...diagnosticSummary(item), nodeIds: page(item.nodeIds ?? [], args.offset, args.limit), evidence: Object.fromEntries(Object.entries(item.evidence ?? {}).map(([key, value]) => [key, Array.isArray(value) ? page(value, args.offset, args.limit) : value])) };
    }
    let nodeId = args.nodeId;
    if (!nodeId) {
      const observed = context(directory, args.sessionId);
      if (observed.stale || observed.state.snapshotId !== snapshot.id) throw new Error('Selection belongs to a different or stale UI snapshot; specify nodeId');
      nodeId = observed.state.selectedNodeId;
    }
    const node = result.nodes.find(node => node.id === nodeId);
    if (!node) throw new Error('Node not found; specify nodeId or select a node in the app');
    return { snapshotId: snapshot.id, node, incoming: page(result.edges.filter(edge => edge.target === nodeId), args.offset, args.limit), outgoing: page(result.edges.filter(edge => edge.source === nodeId), args.offset, args.limit) };
  } finally { db.close(); }
}
