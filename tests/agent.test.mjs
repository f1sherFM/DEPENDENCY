import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';
import { callTool, toolDefinitions } from '../scripts/agent-reader.mjs';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'dependency-agent-test-'));
  const db = new DatabaseSync(join(root, 'dependency.sqlite3'));
  db.exec('PRAGMA user_version=2; CREATE TABLE projects(id TEXT,name TEXT,root_path TEXT,last_opened_at INTEGER,latest_snapshot_id TEXT); CREATE TABLE snapshots(id TEXT,project_id TEXT,created_at INTEGER,analyzer_version TEXT,rules_version TEXT,parsed_file_count INTEGER,reused_file_count INTEGER,result_json TEXT);');
  db.prepare('INSERT INTO projects VALUES(?,?,?,?,?)').run('project','sample','C:/data/sample',1,'new');
  const result = { summary: { name:'sample', packages:[],cycles:[],resolutionIssues:[] }, nodes:[{id:'a',kind:'file'},{id:'b',kind:'file'}],edges:[{id:'ab',source:'a',target:'b',kind:'import'}], diagnostics:[{id:'cycle',ruleId:'dependency-cycle',category:'architecture',severity:'warning',nodeIds:['a','b'],evidence:{type:'cycle',component:['a','b'],route:[{source:'a',target:'b'}]}}],rulesRun:{version:'saved-rule-version'} };
  for(const id of ['old','new']) db.prepare('INSERT INTO snapshots VALUES(?,?,?,?,?,?,?,?)').run(id,'project',1,'saved-parser','saved-rules',1,0,JSON.stringify({...result,summary:{...result.summary,name:id}}));
  db.close();
  writeFileSync(join(root,'agent-access.json'),JSON.stringify({enabled:true}));
  mkdirSync(join(root,'agent-sessions'));
  const sessionId = randomUUID();
  const observation = { schemaVersion:1,sessionId,pid:process.pid,observedAt:Date.now(),state:{snapshotId:'old',projectId:'project',view:'graph',selectedNodeId:'a'} };
  const save = value => writeFileSync(join(root,'agent-sessions',`${value.sessionId}.json`),JSON.stringify(value));
  save(observation);
  return { root, observation, save, close:()=>rmSync(root,{recursive:true,force:true}) };
}
test('adapter reads the observed historic snapshot, pages proof and leaves SQLite bytes unchanged',()=>{
  const f=fixture();
  try {
    const before=readFileSync(join(f.root,'dependency.sqlite3'));
    assert.equal(callTool('dependency_get_snapshot',{},f.root).snapshot.id,'old');
    assert.equal(callTool('dependency_get_snapshot',{snapshotId:'new'},f.root).summary.name,'new');
    assert.equal(callTool('dependency_list_snapshots',{projectId:'project',limit:1},f.root).nextOffset,1);
    assert.equal(callTool('dependency_get_graph',{limit:1},f.root).nodes.nextOffset,1);
    assert.equal(callTool('dependency_get_node',{},f.root).outgoing.items[0].target,'b');
    assert.throws(()=>callTool('dependency_get_node',{snapshotId:'new'},f.root),/different or stale/);
    const diagnostic=callTool('dependency_get_diagnostic',{diagnosticId:'cycle',limit:1},f.root);
    assert.equal(diagnostic.nodeIds.total,2); assert.equal(diagnostic.evidence.component.nextOffset,1);
    assert.deepEqual(readFileSync(join(f.root,'dependency.sqlite3')),before);
    assert.throws(()=>callTool('dependency_get_graph',{limit:1000},f.root),/Invalid/);
    assert.throws(()=>callTool('dependency_get_snapshot',{sql:'DELETE FROM projects'},f.root),/Invalid/);
  } finally { f.close(); }
});
test('stale, ambiguous, disabled and unsupported observations do not masquerade as a live UI',()=>{
  const f=fixture();
  try {
    f.save({...f.observation,observedAt:Date.now()-20000});
    assert.throws(()=>callTool('dependency_get_context',{},f.root),/No live/);
    assert.equal(callTool('dependency_get_context',{sessionId:f.observation.sessionId},f.root).stale,true);
    assert.equal(callTool('dependency_get_snapshot',{snapshotId:'old'},f.root).snapshot.id,'old');
    f.save({...f.observation,observedAt:Date.now()});
    f.save({...f.observation,sessionId:randomUUID(),observedAt:Date.now()});
    assert.throws(()=>callTool('dependency_get_context',{},f.root),/Multiple/);
    assert.equal(callTool('dependency_get_context',{sessionId:f.observation.sessionId},f.root).state.snapshotId,'old');
    writeFileSync(join(f.root,'agent-access.json'),'{"enabled":false}');
    assert.throws(()=>callTool('dependency_get_snapshot',{snapshotId:'old'},f.root),/disabled/);
    writeFileSync(join(f.root,'agent-access.json'),'{"enabled":true}');
    const db=new DatabaseSync(join(f.root,'dependency.sqlite3')); db.exec('PRAGMA user_version=3');
    assert.equal(callTool('dependency_get_snapshot',{snapshotId:'old'},f.root).snapshot.id,'old');
    db.exec('PRAGMA user_version=4'); db.close();
    assert.throws(()=>callTool('dependency_get_snapshot',{snapshotId:'old'},f.root),/Unsupported storage/);
  } finally { f.close(); }
});
test('real MCP stdio handshake, tools and calls work through the official SDK',async()=>{
  const f=fixture();
  const client=new Client({name:'dependency-test',version:'1'},{capabilities:{}});
  const transport=new StdioClientTransport({command:process.execPath,args:[resolve('scripts/dependency-agent.mjs'),'--stdio'],env:{...process.env,DEPENDENCY_APP_DATA:f.root},stderr:'pipe'});
  try {
    await client.connect(transport);
    const tools=await client.listTools();
    assert.equal(tools.tools.length,toolDefinitions.length);
    assert.ok(tools.tools.every(tool=>tool.annotations.readOnlyHint));
    const context=await client.callTool({name:'dependency_get_context',arguments:{}});
    assert.equal(context.structuredContent.state.view,'graph');
    const snapshot=await client.callTool({name:'dependency_get_snapshot',arguments:{}});
    assert.equal(snapshot.structuredContent.snapshot.id,'old');
    const invalid=await client.callTool({name:'dependency_get_graph',arguments:{limit:1000}});
    assert.equal(invalid.isError,true);
    writeFileSync(join(f.root,'agent-access.json'),'{"enabled":false}');
    assert.equal((await client.callTool({name:'dependency_get_context',arguments:{}})).isError,true);
  } finally { await client.close(); f.close(); }
});
