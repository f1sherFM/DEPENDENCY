// Optional browser smoke test. Uses a real engine JSON dump and mocks only Tauri IPC.
// node tests/ui-smoke.mjs <analysis.json> <chromium.exe> [output-directory]
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, writeFile, mkdtemp } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { tmpdir } from 'node:os';
import { resolve, extname, join } from 'node:path';

const [fixturePath, browserPath, outputPath] = process.argv.slice(2);
assert.ok(fixturePath && browserPath, 'Pass an engine JSON dump and a Chromium executable');
const fixture = JSON.parse(await readFile(fixturePath, 'utf8'));
const analysis = fixture.analysis ?? fixture;
const output = outputPath || await mkdtemp(join(tmpdir(), 'dependency-ui-'));
const profile = await mkdtemp(join(tmpdir(), 'dependency-browser-'));
const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url, 'http://localhost');
    const path = resolve('dist', `.${url.pathname === '/' ? '/index.html' : url.pathname}`);
    assert.ok(path.startsWith(resolve('dist') + '\\') || path.startsWith(resolve('dist') + '/'));
    response.setHeader('Content-Type', ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' })[extname(path)] || 'application/octet-stream');
    response.end(await readFile(path));
  } catch { response.statusCode = 404; response.end(); }
});
await new Promise(done => server.listen(0, '127.0.0.1', done));
const origin = `http://127.0.0.1:${server.address().port}`;
const child = spawn(browserPath, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--window-size=1520,1000', 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'], windowsHide: true });
let socket;
try {
  let debugging = '';
  const endpoint = await new Promise((done, reject) => {
    const timeout = setTimeout(() => reject(new Error('Chromium debugging endpoint timeout: '+debugging.slice(-3000))), 15000);
    child.on('error', reject);
    child.once('exit', (code,signal)=>{clearTimeout(timeout);reject(new Error('Chromium exited before debugging: '+code+' '+signal+' '+debugging.slice(-3000)));});
    child.stderr.on('data', chunk => {
      debugging += chunk;
      const match = debugging.match(/DevTools listening on (ws:\/\/[^\s]+)/);
      if (match) { clearTimeout(timeout); done(match[1]); }
    });
  });
  const port = new URL(endpoint).port;
  const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket = new WebSocket(pages.find(page => page.type === 'page').webSocketDebuggerUrl);
  await new Promise(done => socket.addEventListener('open', done, { once: true }));
  let serial = 0; const waiting = new Map(); const exceptions = [];
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    if (message.method === 'Runtime.exceptionThrown') exceptions.push(message.params.exceptionDetails);
    if (message.id && waiting.has(message.id)) {
      const { done, reject, timeout } = waiting.get(message.id); clearTimeout(timeout); waiting.delete(message.id);
      if (message.error) reject(new Error(JSON.stringify(message.error))); else done(message.result);
    }
  });
  const send = (method, params = {}) => new Promise((done, reject) => {
    const id = ++serial;
    const timeout = setTimeout(() => { waiting.delete(id); reject(new Error(`${method} timeout`)); }, 15000);
    waiting.set(id, { done, reject, timeout }); socket.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async expression => {
    const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  const wait = async expression => {
    for (let i = 0; i < 100; i++) { if (await evaluate(`Boolean(${expression})`)) return; await new Promise(done => setTimeout(done, 100)); }
    const state = await evaluate(`({text:document.body.innerText.slice(0,3000),unhandled:window.__unhandled})`);
    throw new Error(`UI condition failed: ${expression}\n${JSON.stringify({state,exceptions})}`);
  };
  const bridge = result => `(() => {
    const result = ${JSON.stringify(result)};
    const gitFixture=${JSON.stringify(fixture.git ?? null)};
    const project = gitFixture?.views.find(view=>view.snapshot.id===${JSON.stringify(fixture.snapshot?.id??"")})?.project ?? { id: ${JSON.stringify(fixture.snapshot?.projectId??'test-project')}, name: result.summary.name, rootPath: result.summary.rootPath, lastOpenedAt: 0, latestSnapshotId: 'snapshot', latestSnapshotAt: 0 };
    const snapshot = ${JSON.stringify(fixture.snapshot ?? null)} ?? { id: 'snapshot', projectId: project.id, createdAt: 0, analyzerVersion: 'test-dump', rulesVersion: result.rulesRun?.version ?? 'legacy', parsedFileCount: 0, reusedFileCount: 0 };
    const impactCases = ${JSON.stringify(fixture.cases ?? [])};
    const key = request => JSON.stringify([request.roots.slice().sort(),request.edgeKinds.slice().sort(),request.maxDepth]);
    window.__impactCalls = [];
    window.__unhandled = []; window.addEventListener('unhandledrejection',event=>window.__unhandled.push(String(event.reason)));
    window.__TAURI_INTERNALS__ = { invoke: async (cmd, args) => {
      if (cmd === 'plugin:dialog|open') {window.__dialogCalls=(window.__dialogCalls||0)+1;if(window.__dialogFailure)throw new Error('Folder picker fixture failure');return window.__dialogResult??null;}
      if (cmd === 'agent_access_enabled') return true;
      if (cmd === 'publish_agent_state') { window.__agentState=args.state; return; }
      const researchRecords=()=>JSON.parse(localStorage.getItem('dependency-research-fixture')||'[]');
      if(cmd==='list_investigations'){const records=researchRecords().filter(item=>item.projectId===args.projectId);return {items:records.slice(args.offset,args.offset+args.limit),total:records.length,nextOffset:args.offset+args.limit<records.length?args.offset+args.limit:null};}
      if(cmd==='load_investigation'){const record=researchRecords().find(item=>item.id===args.id);if(!record)throw new Error('Research not found');return record;}
      if(cmd==='save_investigation'){
        const records=researchRecords();const request=args.request;const old=records.find(item=>item.id===request.id);
        if(old&&old.revision!==request.expectedRevision)throw new Error('Исследование уже изменилось в другом окне. Откройте его заново');
        if(old&&old.snapshotId!==request.snapshotId)throw new Error('Snapshot cannot change');
        const record={id:old?.id??crypto.randomUUID(),projectId:project.id,snapshotId:request.snapshotId,name:request.name.trim(),note:request.note,notePreview:request.note.slice(0,160),graph:request.graph,stateVersion:1,revision:(old?.revision??0)+1,createdAt:old?.createdAt??Date.now(),updatedAt:Date.now()};
        localStorage.setItem('dependency-research-fixture',JSON.stringify([...records.filter(item=>item.id!==record.id),record]));window.__lastResearchSave=record;return record;
      }
      if(cmd==='delete_investigation'){const records=researchRecords();const record=records.find(item=>item.id===args.id);if(!record||record.revision!==args.expectedRevision)throw new Error('Research changed');localStorage.setItem('dependency-research-fixture',JSON.stringify(records.filter(item=>item.id!==args.id)));return;}
      if(cmd==='start_git_analysis'||cmd==='analyze_project')window.__analysisCalls=(window.__analysisCalls||0)+1;
      if (cmd === 'open_project') {window.__openedRoot=args.root;return {project,snapshot,result};}
      if (cmd === 'list_projects') return [project];
      if (cmd === 'list_snapshots') return gitFixture?.snapshots.items ?? [snapshot];
      if (cmd === 'load_snapshot' && gitFixture) return gitFixture.views.find(view=>view.snapshot.id===args.snapshotId);
      if (cmd === 'load_project' || cmd === 'load_snapshot') return { project, snapshot, result };
      if(cmd==='discover_git') return !!gitFixture;
      if(gitFixture){
        if(cmd==='git_status') return gitFixture.status;
        if(cmd==='git_refs') return gitFixture.refs;
        if(cmd==='git_commits') return gitFixture.commits;
        if(cmd==='git_snapshot_page') return gitFixture.snapshots;
        if(cmd==='snapshot_inventory') return gitFixture.inventories[args.snapshotId];
        if(cmd==='start_git_analysis') {window.__jobPolls=0;window.__gitJob={id:'fixture-job',root:project.rootPath,phase:'capturing',fileCount:0,done:false,error:null,snapshotId:null};return window.__gitJob.id;}
        if(cmd==='get_git_job') {if(window.__gitJob&&!window.__gitJob.done&&args.jobId){if(++window.__jobPolls>=2)window.__gitJob={...window.__gitJob,phase:'complete',done:true,fileCount:10,snapshotId:gitFixture.commitView.snapshot.id};}return window.__gitJob??null;}
        if(cmd==='cancel_git_job') {window.__gitJob={...window.__gitJob,phase:'cancelled',done:true,error:'Операция отменена'};return;}
        if(cmd==='compare_snapshots') {if(args.baseId!==gitFixture.baseSnapshotId||args.targetId!==gitFixture.targetSnapshotId)throw new Error('No exported comparison for these IDs');return gitFixture.comparison;}
        if(cmd==='comparison_page') {const rows=gitFixture.changes.items.filter(item=>(!args.category||item.category===args.category)&&(item.label+' '+item.detail).toLowerCase().includes(args.text.toLowerCase()));return {...gitFixture.changes,items:rows.slice(args.offset,args.offset+args.limit),offset:args.offset,matchingCount:rows.length,nextOffset:args.offset+args.limit<rows.length?args.offset+args.limit:null};}
      }
      if (cmd === 'get_impact') {
        window.__impactCalls.push(args.request);
        const item = impactCases.find(item => key(item.request) === key(args.request));
        if (!item) throw new Error('No engine-exported impact case: ' + key(args.request));
        const delay=window.__delayNextImpact || 0; window.__delayNextImpact=0;
        if (delay) await new Promise(done=>setTimeout(done,delay));
        return item.summary;
      }
      if (cmd === 'get_impact_page') {
        const item = impactCases.find(item => item.summary.queryId === args.queryId);
        if (!item || window.__expireImpact) throw new Error('Результат Impact Analysis истёк. Повторите запрос');
        const rows = item.page.items.filter(row => (!args.testsOnly || row.isTestCandidate) && row.path.toLowerCase().includes(args.text.trim().toLowerCase()));
        return { ...item.page, items: rows.slice(args.offset,args.offset+args.limit), offset: args.offset, knownMatchingCount: rows.length, nextOffset: args.offset+args.limit<rows.length ? args.offset+args.limit : null };
      }
      if (cmd === 'get_impact_path') {
        const item = impactCases.find(item => item.summary.queryId === args.queryId);
        if (!item || window.__expireImpact) throw new Error('Результат Impact Analysis истёк. Повторите запрос');
        return item.paths[args.nodeId];
      }
      throw new Error('Unexpected IPC: ' + cmd);
    }};
  })();`;
  await send('Runtime.enable'); await send('Page.enable');
  const reload = async () => {
    // Waiting for a selector alone can match the old document while reload is
    // still pending, making the next action race navigation.
    let listener, timer;
    const loaded = new Promise((done, reject) => {
      listener = event => { if (JSON.parse(event.data).method === 'Page.loadEventFired') done(); };
      socket.addEventListener('message', listener);
      timer = setTimeout(() => reject(new Error('Page reload timeout')), 15000);
    });
    try { await Promise.all([send('Page.reload'), loaded]); }
    finally { clearTimeout(timer); socket.removeEventListener('message', listener); }
  };
  let bridgeScriptId;
  const installBridge = async result => {
    // Keep one IPC fixture per document; old megabyte-sized scripts must not
    // accumulate or register duplicate listeners on every reload.
    if (bridgeScriptId) await send('Page.removeScriptToEvaluateOnNewDocument', { identifier: bridgeScriptId });
    const installed = await send('Page.addScriptToEvaluateOnNewDocument', { source: bridge(result) });
    bridgeScriptId = installed.identifier;
  };
  await installBridge(analysis);
  await send('Page.navigate', { url: origin });
  await wait(`document.querySelector('.main-nav') && document.querySelector('.stat-card')`);
  await evaluate(`window.__dialogFailure=true;document.querySelector('button.sidebar-project').click()`);
  await wait(`document.querySelector('.error-banner')?.textContent.includes('Folder picker fixture failure')`);
  await wait(`!document.querySelector('.page-heading .primary-button').disabled`);
  await evaluate(`window.__dialogFailure=false;Array.from(document.querySelectorAll('.quick-action')).find(b=>b.textContent.includes('Открыть проект')).click()`);
  await wait(`window.__dialogCalls===2 && !document.querySelector('.page-heading .primary-button').disabled && !document.querySelector('.error-banner')`);
  assert.equal(await evaluate(`window.__unhandled.length`),0,'folder picker failures must be handled');
  await evaluate(`window.__dialogResult='C:/fixture/selected-project';document.querySelector('button.sidebar-project').click()`);
  await wait(`window.__dialogCalls===3 && window.__openedRoot==='C:/fixture/selected-project' && !document.querySelector('button.sidebar-project').disabled`);
  await evaluate(`window.__dialogResult=null`);
  if(analysis.analysisProfile){await wait(`document.querySelector('.analysis-profile')`);for(const language of Object.keys(analysis.analysisProfile.analyzers)){assert.ok(await evaluate(`document.querySelector('.analysis-profile').textContent.includes(${JSON.stringify(language)})`));}}
  await evaluate(`document.querySelectorAll('.main-nav button')[4].click()`);
  await wait(`document.querySelector('.diagnostics-coverage')`);
  assert.ok(await evaluate(`document.querySelector('.diagnostics-coverage').textContent.includes('Архитектурных предупреждений')`));
  if(analysis.diagnostics.some(item=>item.ruleId==='dependency-cycle')) {
  await evaluate(`(() => { const select=document.querySelector('.diagnostics-filters select'); select.value='dependency-cycle'; select.dispatchEvent(new Event('change',{bubbles:true})); })()`);
  await wait(`document.querySelector('.diagnostic-card')?.textContent.includes('Подтверждённый замкнутый маршрут')`);
  const screenshot = await send('Page.captureScreenshot', { format: 'png' });
  await writeFile(join(output, 'diagnostics.png'), Buffer.from(screenshot.data, 'base64'));
  await evaluate(`document.querySelector('.diagnostic-card .text-button:last-child').click()`);
  await wait(`document.querySelector('.graph-workspace .graph-navigation code')?.textContent === 'dependency-cycle'`);
  assert.ok(await evaluate(`document.querySelectorAll('.graph-node').length > 0`));
  assert.ok(await evaluate(`document.querySelectorAll('.graph-edge').length > 0`));
  await wait(`window.__agentState?.view==='graph' && window.__agentState.graph?.renderedNodeIds.length>0`);
  assert.equal(await evaluate(`window.__agentState.snapshotId`),fixture.snapshot?.id??'snapshot');
  assert.equal(await evaluate(`window.__agentState.graph.renderedNodeIds.length`),await evaluate(`document.querySelectorAll('.graph-node').length`));
  assert.equal(await evaluate(`window.__agentState.layout.rootScrollTop`),0);
  await evaluate(`window.dispatchEvent(new ErrorEvent('error',{message:'agent-observation-test',filename:'fixture.ts',lineno:7,colno:3}))`);
  await wait(`window.__agentState.runtimeErrors.some(error=>error.message.includes('agent-observation-test')&&error.message.includes('fixture.ts:7:3'))`);
  await evaluate(`Array.from(document.querySelectorAll('.graph-navigation button')).find(button=>button.textContent==='К диагностике').click()`);
  await wait(`document.querySelector('.diagnostics-filters')`);
  } else {
    assert.ok(await evaluate(`Boolean(document.querySelector('.diagnostics-filters'))`));
    if(!analysis.diagnostics.some(item=>item.category==='architecture')){
      assert.equal(await evaluate(`document.querySelectorAll('.diagnostic-card').length`),0);
    }
    const coverageCount=analysis.summary.resolutionIssues.filter(issue=>issue.status==='unsupported').length;
    if(coverageCount){
      assert.ok(await evaluate(`document.querySelector('.coverage-limitations').textContent.includes(${JSON.stringify('Отмеченных случаев: '+coverageCount)})`));
      await evaluate(`Array.from(document.querySelectorAll('.diagnostics-coverage button')).find(button=>button.textContent.startsWith('Покрытие анализа')).click()`);
      await wait(`document.querySelector('.coverage-group')`);
      await writeFile(join(output,'real-coverage.png'),Buffer.from((await send('Page.captureScreenshot',{format:'png'})).data,'base64'));
    }
  }
  await evaluate(`document.querySelectorAll('.main-nav button')[1].click()`);
  await wait(`document.querySelector('.graph-workspace')`);
  await evaluate(`Array.from(document.querySelectorAll('.graph-navigation button')).find(button=>button.textContent==='Весь проект').click()`);
  await wait(`document.querySelector('.graph-node.directory')`);
  assert.equal(await evaluate(`document.querySelector('.filter-check input').checked`), false);
  assert.equal(await evaluate(`document.querySelectorAll('.graph-node.package').length`), 0);
  await evaluate(`document.querySelector('.graph-node.directory').dispatchEvent(new MouseEvent('click',{bubbles:true}))`);
  await wait(`Array.from(document.querySelectorAll('.graph-navigation button')).some(button=>button.textContent==='Выше')`);
  const graphShot = await send('Page.captureScreenshot', { format: 'png' });
  await writeFile(join(output, 'graph.png'), Buffer.from(graphShot.data, 'base64'));
  if (fixture.cases) {
    await evaluate(`document.querySelectorAll('.main-nav button')[6].click()`);
    await wait(`document.querySelector('.impact-controls')`);
    await evaluate(`(() => { const input=document.querySelector('[aria-label="Найти исходный файл"]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,'target.ts'); input.dispatchEvent(new Event('input',{bubbles:true})); })()`);
    await wait(`document.querySelector('.impact-choices button')?.textContent === 'src/target.ts'`);
    await evaluate(`document.querySelector('.impact-choices button').click()`);
    await wait(`document.querySelectorAll('.impact-table tbody tr').length === 7`);
    await evaluate(`document.querySelector('.impact-list-tools input[type=checkbox]').click()`);
    await wait(`document.querySelectorAll('.impact-table tbody tr').length === 1`);
    assert.ok(await evaluate(`document.querySelector('.impact-table').textContent.includes('unit.spec.ts')`));
    await evaluate(`window.__expireImpact=true; document.querySelector('.impact-table .text-button').click()`);
    await wait(`document.querySelector('.impact-view [role=alert]')?.textContent.includes('истёк')`);
    await evaluate(`window.__expireImpact=false; document.querySelector('.impact-view [role=alert] button').click()`);
    await wait(`document.querySelectorAll('.impact-table tbody tr').length === 1 && !document.querySelector('.impact-view [role=alert]')`);
    await evaluate(`document.querySelector('.impact-table .text-button').click()`);
    await wait(`document.querySelectorAll('.impact-proof li').length === 3`);
    assert.ok(await evaluate(`document.querySelector('.impact-proof li:last-child').textContent.includes('src/target.ts')`));
    const impactShot=await send('Page.captureScreenshot',{format:'png'}); await writeFile(join(output,'impact.png'),Buffer.from(impactShot.data,'base64'));
    await evaluate(`Array.from(document.querySelectorAll('.impact-proof button')).find(button=>button.textContent==='Показать сегмент в графе').click()`);
    await wait(`document.querySelectorAll('.graph-node.impact-root').length === 1`);
    assert.equal(await evaluate(`document.querySelectorAll('.graph-edge').length`),3);
    await evaluate(`Array.from(document.querySelectorAll('.graph-navigation button')).find(button=>button.textContent==='К анализу влияния').click()`);
    await evaluate(`window.__delayNextImpact=1500; Array.from(document.querySelectorAll('.impact-options button')).find(button=>button.textContent==='Статические').click()`);
    await wait(`window.__impactCalls.at(-1).edgeKinds.length === 3`);
    await evaluate(`Array.from(document.querySelectorAll('.impact-options button')).find(button=>button.textContent==='Все импорты').click()`);
    await wait(`document.querySelector('.impact-summary')?.textContent.includes('Потребителей: 7')`);
    await evaluate(`new Promise(done=>setTimeout(done,1600))`);
    assert.ok(await evaluate(`document.querySelector('.impact-summary').textContent.includes('Потребителей: 7')`));
    await evaluate(`document.querySelector('.impact-list-tools input[type=checkbox]').click(); Array.from(document.querySelectorAll('.impact-options button')).find(button=>button.textContent==='Статические').click()`);
    await wait(`document.querySelectorAll('.impact-table tbody tr').length === 5`);
    await evaluate(`(() => { Array.from(document.querySelectorAll('.impact-options button')).find(button=>button.textContent==='Все импорты').click(); const select=document.querySelector('[aria-label="Глубина влияния"]'); select.value='1'; select.dispatchEvent(new Event('change',{bubbles:true})); })()`);
    await wait(`document.querySelector('.impact-summary')?.textContent.includes('Частичный обход')`);
    assert.ok(await evaluate(`document.querySelector('.impact-summary').textContent.includes('не менее')`));
    await evaluate(`(() => { const select=document.querySelector('[aria-label="Глубина влияния"]'); select.value=''; select.dispatchEvent(new Event('change',{bubbles:true})); })()`);
    await wait(`document.querySelectorAll('.impact-table tbody tr').length === 7`);
    await evaluate(`(() => { const input=document.querySelector('[aria-label="Найти исходный файл"]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,'app.ts'); input.dispatchEvent(new Event('input',{bubbles:true})); })()`);
    await wait(`document.querySelector('.impact-choices button')?.textContent === 'src/app.ts'`);
    await evaluate(`document.querySelector('.impact-choices button').click()`);
    await wait(`document.querySelectorAll('.impact-table tbody tr').length === 6`);
    await evaluate(`document.querySelectorAll('.main-nav button')[1].click()`);
    await wait(`document.querySelector('[aria-label="Выбрать src/unit.spec.ts"]')`);
    await evaluate(`document.querySelector('[aria-label="Выбрать src/unit.spec.ts"]').dispatchEvent(new MouseEvent('click',{bubbles:true}))`);
    await wait(`document.querySelector('.impact-action')`);
    await evaluate(`document.querySelector('.impact-action').click()`);
    await wait(`document.querySelector('.impact-empty')?.textContent.includes('не найдены потребители')`);
    assert.ok(await evaluate(`document.querySelector('.impact-empty').textContent.includes('не означает')`));
    console.log('PASS: real Rust impact responses, type filters, partial counts, test path, graph roots, multi-root union, file-context action and empty result');
  }
  if(fixture.git){
    await evaluate(`document.querySelectorAll('.main-nav button')[7].click()`);
    await wait(`document.querySelector('.git-status-table')?.textContent.includes('src/main.ts')`);
    assert.ok(await evaluate(`document.querySelector('.git-status-table').textContent.includes('untracked.txt')`));
    await evaluate(`document.querySelector('.git-worktree-action').click()`);
    await wait(`document.querySelector('.git-job')?.textContent.includes('capturing')`);
    await evaluate(`document.querySelector('.git-job button').click()`);
    await wait(`document.querySelector('.git-job')?.textContent.includes('cancelled')`);
    await evaluate(`document.querySelector('.git-commit-action').click()`);
    await wait(`document.querySelector('.git-job')?.textContent.includes('complete')`);
    await evaluate(`(() => {const setter=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;for(const [label,value] of [['Base snapshot',${JSON.stringify(fixture.git.baseSnapshotId)}],['Target snapshot',${JSON.stringify(fixture.git.targetSnapshotId)}]]){const input=document.querySelector('[aria-label="'+label+'"]');setter.call(input,value);input.dispatchEvent(new Event('input',{bubbles:true}));}})()`);
    await evaluate(`document.querySelector('.git-compare-action').click()`);
    await wait(`document.querySelector('.git-comparison-summary')?.textContent.includes('Профили анализаторов сопоставимы')`);
    await wait(`document.querySelectorAll('.git-changes article').length>0`);
    await evaluate(`(() => {const select=document.querySelector('[aria-label="Категория изменений"]');select.value='edge';select.dispatchEvent(new Event('change',{bubbles:true}));})()`);
    await wait(`document.querySelector('.git-changes')?.textContent.includes('Импорт отсутствует в новом AST')`);
    const gitShot=await send('Page.captureScreenshot',{format:'png'});await writeFile(join(output,'git.png'),Buffer.from(gitShot.data,'base64'));
    await evaluate(`document.querySelector('.git-changes article .git-change-actions button').click()`);
    await wait(`document.querySelector('.snapshot-basis')?.textContent.includes(${JSON.stringify(fixture.git.views[0].result.basis.commitOid)})`);
    assert.ok(await evaluate(`Boolean(document.querySelector('.graph-workspace'))`));
    await evaluate(`document.querySelectorAll('.main-nav button')[7].click()`);
    await wait(`document.querySelector('.git-comparison-summary')`);
    assert.equal(await evaluate(`document.querySelector('[aria-label="Base snapshot"]').value`),fixture.git.baseSnapshotId);
    console.log('PASS: real Git status/commit/comparison data, job cancel/complete transport, inventory and historical graph navigation');
  }
  // Saved research: restore a historical snapshot and world-space camera after a reload/resize.
  await reload();await wait(`document.querySelector('.stat-card')`);
  const researchAnalysis=fixture.git?fixture.git.views.find(view=>view.snapshot.id===fixture.git.baseSnapshotId).result:analysis;
  if(fixture.git){
    await evaluate(`document.querySelectorAll('.main-nav button')[5].click()`);await wait(`document.querySelector('.snapshot-row')`);
    const index=fixture.git.snapshots.items.findIndex(item=>item.id===fixture.git.baseSnapshotId);assert.ok(index>=0);
    await evaluate(`document.querySelectorAll('.snapshot-row')[${index}].click()`);await wait(`document.querySelector('.snapshot-row.active')`);
  }
  await evaluate(`document.querySelectorAll('.main-nav button')[1].click()`);await wait(`document.querySelector('.graph-workspace')`);
  await evaluate(`document.querySelector('.graph-tabs button').click()`);
  const researchNode=researchAnalysis.nodes.find(node=>node.kind==='file'&&node.language);
  await wait(`Array.from(document.querySelectorAll('.graph-node')).some(node=>node.dataset.nodeId===${JSON.stringify(researchNode.id)})`);
  await evaluate(`Array.from(document.querySelectorAll('.graph-node')).find(node=>node.dataset.nodeId===${JSON.stringify(researchNode.id)}).dispatchEvent(new MouseEvent('click',{bubbles:true}))`);
  await wait(`document.querySelector('.graph-node.selected')`);
  await evaluate(`(() => {const input=document.querySelector('[aria-label="Поиск узла"]');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,${JSON.stringify(researchNode.label)});input.dispatchEvent(new Event('input',{bubbles:true}));})()`);
  await wait(`document.querySelector('[aria-label="Поиск узла"]').value===${JSON.stringify(researchNode.label)}`);
  await evaluate(`document.querySelector('[title="Отдалить"]').click()`);
  await evaluate(`document.querySelector('[title="Отдалить"]').click()`);
  await evaluate(`document.querySelector('[aria-label="Скрыть панель"]')?.click()`);
  await evaluate(`new Promise(resolve=>setTimeout(resolve,200))`);
  const worldCamera=()=>evaluate(`(() => {const box=document.querySelector('.graph-canvas').getBoundingClientRect();const m=document.querySelector('.graph-canvas > svg > g').transform.baseVal.consolidate().matrix;return {zoom:m.a,centerX:(box.width/2-m.e)/m.a,centerY:(box.height/2-m.f)/m.a};})()`);
  const savedPose=await worldCamera();
  await evaluate(`Array.from(document.querySelectorAll('.graph-navigation button')).find(button=>button.textContent.includes('Сохранить исследование')).click()`);
  await wait(`document.querySelector('.investigation-dialog')?.open`);
  await evaluate(`(() => {const input=document.querySelector('.investigation-dialog input');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,'Мой обход');input.dispatchEvent(new Event('input',{bubbles:true}));const note=document.querySelector('.investigation-dialog textarea');Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(note,'Заметка <script>только данные</script>');note.dispatchEvent(new Event('input',{bubbles:true}));})()`);
  await evaluate(`document.querySelector('.investigation-dialog button[type="submit"]').click()`);
  await wait(`!document.querySelector('.investigation-dialog')&&document.querySelector('.research-context')`);
  const savedResearch=await evaluate(`window.__lastResearchSave`);
  assert.equal(savedResearch.graph.query,researchNode.label);assert.equal(savedResearch.graph.selectedNodeId,researchNode.id);assert.equal(savedResearch.graph.detailsOpen,false);
  assert.ok(Math.abs(savedResearch.graph.camera.centerX-savedPose.centerX)<0.1&&Math.abs(savedResearch.graph.camera.centerY-savedPose.centerY)<0.1);
  await send('Emulation.setDeviceMetricsOverride',{width:1280,height:720,deviceScaleFactor:1.25,mobile:false});
  await reload();await wait(`document.querySelector('.stat-card')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[8].click()`);await wait(`document.querySelector('.investigation-item')`);
  assert.ok(await evaluate(`document.querySelector('.investigation-item').textContent.includes('<script>только данные</script>')`));
  await evaluate(`Array.from(document.querySelectorAll('.investigation-item button')).find(button=>button.textContent==='Открыть исследование').click()`);
  await wait(`document.querySelector('.research-context')?.textContent.includes('Мой обход')&&document.querySelector('.graph-node.selected')`);
  await evaluate(`new Promise(resolve=>setTimeout(resolve,250))`);
  const restoredPose=await worldCamera();assert.ok(Math.abs(restoredPose.zoom-savedPose.zoom)<0.0001&&Math.abs(restoredPose.centerX-savedPose.centerX)<0.1&&Math.abs(restoredPose.centerY-savedPose.centerY)<0.1,'camera center/zoom must survive restart with a different viewport');
  assert.equal(await evaluate(`document.querySelector('[aria-label="Поиск узла"]').value`),researchNode.label);
  assert.equal(await evaluate(`document.querySelector('.graph-node.selected').dataset.nodeId`),researchNode.id);
  await wait(`window.__agentState?.snapshotId===${JSON.stringify(savedResearch.snapshotId)}`);
  assert.equal(await evaluate(`window.__analysisCalls||0`),0,'restoration must not analyze sources');
  await evaluate(`document.querySelectorAll('.main-nav button')[8].click()`);await wait(`document.querySelector('.investigation-item')`);
  await evaluate(`Array.from(document.querySelectorAll('.investigation-item button')).find(button=>button.textContent==='Изменить заметку').click()`);await wait(`document.querySelector('.investigation-dialog')?.open`);
  await evaluate(`(() => {const note=document.querySelector('.investigation-dialog textarea');Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(note,'Изменённая заметка');note.dispatchEvent(new Event('input',{bubbles:true}));})()`);
  await evaluate(`document.querySelector('.investigation-dialog button[type="submit"]').click()`);await wait(`!document.querySelector('.investigation-dialog')&&document.querySelector('.investigation-item').textContent.includes('Изменённая заметка')`);
  assert.equal(await evaluate(`JSON.parse(localStorage.getItem('dependency-research-fixture'))[0].revision`),2);
  await evaluate(`Array.from(document.querySelectorAll('.investigation-item button')).find(button=>button.textContent==='Удалить').click()`);
  assert.equal(await evaluate(`JSON.parse(localStorage.getItem('dependency-research-fixture')).length`),1);
  await evaluate(`Array.from(document.querySelectorAll('.investigation-item button')).find(button=>button.textContent==='Да, удалить').click()`);await wait(`!document.querySelector('.investigation-item')`);
  await send('Emulation.clearDeviceMetricsOverride');
  console.log('PASS: saved research create, historic snapshot restore after reload/resize, selected node/filter/camera, edit, confirmed deletion, no re-analysis');
  const crowded = structuredClone(analysis);
  crowded.nodes = Array.from({length:36},(_,index)=>({id:`preview-${index}.ts`,label:`preview-${index}.ts`,path:`preview-${index}.ts`,kind:'file',language:'TypeScript',lines:1,incoming:0,outgoing:0,declared:null}));
  crowded.edges = [];
  await installBridge(crowded);
  await reload(); await wait(`document.querySelectorAll('.mini-node').length===36`);
  assert.ok(await evaluate(`(() => {const svg=document.querySelector('.mini-graph svg');const box=svg.viewBox.baseVal;return Array.from(svg.querySelectorAll('.mini-node rect')).every(rect=>rect.x.baseVal.value>=box.x&&rect.y.baseVal.value>=box.y&&rect.x.baseVal.value+rect.width.baseVal.value<=box.x+box.width&&rect.y.baseVal.value+rect.height.baseVal.value<=box.y+box.height);})()`),'all 36 preview nodes must fit inside the SVG');
  assert.ok(await evaluate(`document.querySelector('[aria-label="Настройки пока недоступны"]').disabled`));
  await evaluate(`document.querySelector('.mini-node').dispatchEvent(new MouseEvent('click',{bubbles:true}))`);
  await wait(`document.querySelector('.graph-workspace') && document.querySelector('.graph-node.selected')`);
  // A small, tall graph crosses the coverage hint's visibility boundary while zooming.
  const zoomFixture = structuredClone(analysis);
  zoomFixture.nodes = Array.from({length:9},(_,index)=>({id:`src/zoom-${index}.ts`,path:`src/zoom-${index}.ts`,label:`zoom-${index}.ts`,kind:'file',language:'TypeScript',lines:1,incoming:0,outgoing:0,declared:null}));
  zoomFixture.edges = zoomFixture.nodes.slice(1).map((node,index)=>({id:`zoom-edge-${index}`,source:zoomFixture.nodes[0].id,target:node.id,kind:'import',line:1}));
  await installBridge(zoomFixture);
  await reload(); await wait(`document.querySelector('.stat-card')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[1].click()`);
  await wait(`document.querySelector('.graph-node.directory')`);
  await evaluate(`document.querySelector('.graph-node.directory').dispatchEvent(new MouseEvent('click',{bubbles:true}))`);
  await wait(`document.querySelectorAll('.graph-node').length===9`);
  const camera = () => evaluate(`(() => {const m=document.querySelector('.interactive-canvas > svg > g').transform.baseVal.consolidate().matrix;return {zoom:m.a,x:m.e,y:m.f};})()`);
  const settle = () => evaluate(`new Promise(resolve=>setTimeout(resolve,180))`);
  await settle();
  const zooms=[];
  for(let index=0;index<12;index++){
    await evaluate(`document.querySelector('[title="Отдалить"]').click()`);
    await settle(); zooms.push((await camera()).zoom);
  }
  assert.ok(zooms.every((zoom,index)=>!index||zoom<zooms[index-1]),`manual zoom must not bounce on resize: ${zooms}`);
  assert.ok(zooms.at(-1)<0.2,`small graph should zoom below 64%: ${zooms}`);
  const manualCamera=await camera();
  // Canvas dimensions can change when the coverage hint wraps or the window resizes.
  await send('Emulation.setDeviceMetricsOverride',{width:1280,height:720,deviceScaleFactor:1.25,mobile:false});
  await settle();
  assert.equal((await camera()).zoom,manualCamera.zoom,'a canvas resize must not replace manual zoom with the automatic camera');
  await evaluate(`document.querySelector('[title="Вписать граф"]').click()`);
  await settle(); const fitted=await camera();
  const fittedBounds=await evaluate(`(() => {const box=document.querySelector('.graph-canvas').getBoundingClientRect();return Array.from(document.querySelectorAll('.graph-node > rect')).every(node=>{const r=node.getBoundingClientRect();return r.left>=box.left&&r.right<=box.right&&r.top>=box.top&&r.bottom<=box.bottom});})()`);
  assert.ok(fittedBounds,'fit must keep every node inside the canvas');
  await settle(); assert.deepEqual(await camera(),fitted,'fit must remain stable after ResizeObserver callbacks');
  await send('Emulation.setDeviceMetricsOverride',{width:1536,height:864,deviceScaleFactor:1.25,mobile:false});
  await settle(); const refitted=await camera();
  assert.ok(refitted.zoom<1,'resize after explicit fit must keep fit mode, not reset to the readable initial scale');
  await settle(); assert.deepEqual(await camera(),refitted);
  const panOrigin=await evaluate(`(() => {const r=document.querySelector('.graph-canvas').getBoundingClientRect();return {x:r.left+8,y:r.top+8};})()`);
  await send('Input.dispatchMouseEvent',{type:'mousePressed',...panOrigin,button:'left',buttons:1,clickCount:1});
  await send('Input.dispatchMouseEvent',{type:'mouseMoved',x:panOrigin.x+35,y:panOrigin.y+25,buttons:1});
  await send('Input.dispatchMouseEvent',{type:'mouseReleased',x:panOrigin.x+35,y:panOrigin.y+25,button:'left',buttons:0,clickCount:1});
  await settle(); const panned=await camera();
  assert.ok(Math.abs(panned.x-refitted.x-35)<0.001 && Math.abs(panned.y-refitted.y-25)<0.001,'background drag must move the camera');
  await send('Emulation.setDeviceMetricsOverride',{width:1280,height:720,deviceScaleFactor:1.25,mobile:false});
  await settle(); assert.deepEqual(await camera(),panned,'manual pan must persist through canvas reflow');
  await evaluate(`document.querySelector('.graph-canvas').dispatchEvent(new WheelEvent('wheel',{deltaY:100,clientX:500,clientY:450,bubbles:true}))`);
  await settle(); assert.ok((await camera()).zoom<panned.zoom,'wheel zoom must persist after fit');
  await evaluate(`for(let index=0;index<40;index++)document.querySelector('[title="Отдалить"]').click()`);
  await settle(); assert.ok(Math.abs((await camera()).zoom-0.02)<0.00001,'manual zoom should reach the actual 2% minimum');
  await send('Emulation.clearDeviceMetricsOverride');
  const noisy = structuredClone(analysis);
  const architectural = noisy.diagnostics.filter(item => item.category === 'architecture');
  const coverage = Array.from({length: 644}, (_,index) => ({id:`coverage-${index}`, ruleId:'resolution-unsupported', severity:'warning',confidence:'confirmed',category:'coverage',title:'Разрешение импорта',explanation:'Rust macros are not expanded',suggestion:'Inspect the macro',nodeIds:[noisy.nodes[0].id],evidence:{type:'resolution',issue:{source:noisy.nodes[0].id,line:index+1,specifier:`assert!(${index})`,status:'unsupported',reason:'Rust macros are not expanded',hint:'Inspect the macro',attemptedCandidates:[]}}}));
  noisy.diagnostics = [...architectural,...coverage];
  noisy.summary.resolutionIssues=coverage.map(item=>item.evidence.issue);
  noisy.summary.parseErrorCount=0;
  noisy.rulesRun.coverageIssueCount=644;
  for(let index=0;index<50;index++){
    const id=`package:noise-${index}`;
    noisy.nodes.push({id,path:id,label:id,kind:'package',language:null,lines:0,incoming:1,outgoing:0,declared:true});
    noisy.edges.push({id:`noise-edge-${index}`,source:noisy.nodes[0].id,target:id,kind:'package',line:1});
  }
  await installBridge(noisy);
  await reload(); await wait(`document.querySelector('.stat-card')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[4].click()`);
  await wait(`document.querySelector('.diagnostics-filters')`);
  assert.ok(await evaluate(`document.querySelector('.coverage-limitations').textContent.includes('Отмеченных случаев: 644')`));
  assert.ok(await evaluate(`document.querySelector('.coverage-resolution-counts').textContent.includes('Неразрешённых импортов: 0')`));
  assert.ok(await evaluate(`!document.querySelector('.diagnostics-coverage').textContent.includes('Зафиксировано проблем покрытия')`));
  assert.equal(await evaluate(`document.querySelectorAll('.coverage-group').length`),0);
  assert.equal(await evaluate(`document.querySelectorAll('.main-nav button')[4].querySelector('.nav-count').textContent`),String(architectural.filter(item=>item.severity!=='info').length));
  await evaluate(`Array.from(document.querySelectorAll('.diagnostics-coverage button')).find(button=>button.textContent.startsWith('Покрытие анализа')).click()`);
  await wait(`document.querySelectorAll('.coverage-group').length===1`);
  assert.equal(await evaluate(`document.querySelectorAll('.coverage-group .diagnostic-card').length`),0);
  assert.ok(await evaluate(`document.querySelector('.coverage-group').textContent.includes('644 сообщений')`));
  await evaluate(`document.querySelector('.coverage-group summary').click()`);
  await wait(`document.querySelectorAll('.coverage-group .diagnostic-card').length===30`);
  await evaluate(`document.querySelector('.coverage-group details > .text-button').click()`);
  await wait(`document.querySelectorAll('.coverage-group .diagnostic-card').length===60`);
  await writeFile(join(output,'coverage-grouped.png'),Buffer.from((await send('Page.captureScreenshot',{format:'png'})).data,'base64'));
  await evaluate(`document.querySelectorAll('.main-nav button')[1].click()`);
  await wait(`document.querySelector('.graph-workspace')`);
  assert.equal(await evaluate(`document.querySelectorAll('.graph-node.package').length`),0);
  await evaluate(`document.querySelector('.filter-check input').click()`);
  await wait(`document.querySelectorAll('.graph-node.package').length>=50`);
  assert.ok(await evaluate(`document.querySelector('.interactive-canvas > svg > g').transform.baseVal.consolidate().matrix.a>=1`),'automatic graph camera must preserve readable scale');
  assert.ok(await evaluate(`document.querySelector('.graph-coverage').textContent.includes('за пределами экрана')`));
  await evaluate(`document.querySelector('[title="Вписать граф"]').click()`);
  await wait(`document.querySelector('.interactive-canvas > svg > g').transform.baseVal.consolidate().matrix.a<1`);
  const large = structuredClone(analysis);
  const architectureTemplate=analysis.diagnostics.find(item=>item.category==='architecture') ?? {ruleId:'high-fan-out',severity:'warning',confidence:'heuristic',category:'architecture',title:'Много зависимостей',explanation:'Fixture',suggestion:'Review',nodeIds:[],evidence:{type:'coupling',observed:21,threshold:20,neighbors:[],imports:[]}};
  large.diagnostics = Array.from({ length: 125 }, (_, index) => ({ ...architectureTemplate, id: `large-${index}` }));
  await installBridge(large);
  await reload(); await wait(`document.querySelector('.stat-card')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[4].click()`);
  await wait(`document.querySelectorAll('.diagnostic-card').length === 50`);
  await evaluate(`document.querySelector('.problems-list > button').click()`);
  await wait(`document.querySelectorAll('.diagnostic-card').length === 100`);
  // Test effective CSS viewports, including laptop sizes at Windows scaling.
  for (const [width,height] of [[1920,1080],[1536,864],[1280,720],[1024,640],[800,560]]) {
    await send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1.25,mobile:false});
    await evaluate(`document.querySelectorAll('.main-nav button')[4].click()`);
    await evaluate(`document.querySelector('.main-content').scrollTop=100000`);
    const frame = await evaluate(`(() => {const top=document.querySelector('.topbar').getBoundingClientRect();const main=document.querySelector('.main-content');const box=main.getBoundingClientRect();return {rootScroll:document.scrollingElement.scrollTop,rootWidth:document.scrollingElement.scrollWidth,width:innerWidth,top:top.top,headerBottom:top.bottom,mainTop:box.top,mainBottom:box.bottom,height:innerHeight,scrollTop:main.scrollTop,horizontal:main.scrollWidth-main.clientWidth};})()`);
    assert.ok(frame.scrollTop>0,`long diagnostics must scroll inside the workspace: ${width}x${height} ${JSON.stringify(frame)}`);
    assert.equal(frame.rootScroll,0);
    assert.ok(frame.rootWidth<=frame.width+1 && frame.horizontal<=1,JSON.stringify(frame));
    assert.equal(frame.top,0); assert.ok(frame.mainTop>=frame.headerBottom-1 && frame.mainBottom<=frame.height+1,JSON.stringify(frame));
    await evaluate(`document.querySelectorAll('.main-nav button')[1].click()`);
    await wait(`document.querySelector('.graph-workspace') && document.querySelector('.main-content').scrollTop===0`);
    const graphFrame = await evaluate(`(() => {const main=document.querySelector('.main-content');const footer=document.querySelector('.graph-footer').getBoundingClientRect();const canvas=document.querySelector('.graph-canvas').getBoundingClientRect();return {footer:footer.bottom,main:main.getBoundingClientRect().bottom,canvas:canvas.height,scroll:main.scrollHeight-main.clientHeight};})()`);
    assert.ok(graphFrame.canvas>=100,`graph needs usable drawing space: ${width}x${height} ${JSON.stringify(graphFrame)}`);
    assert.ok(graphFrame.footer<=graphFrame.main+1,`graph controls must fit: ${width}x${height} ${JSON.stringify(graphFrame)}`);
    if(width===1024){
      await evaluate(`document.querySelector('[title="Показать контекст"]').click()`);
      await wait(`document.querySelector('.details-panel')`);
      assert.equal(await evaluate(`getComputedStyle(document.querySelector('.details-panel')).display`),'block');
      await evaluate(`document.querySelector('[aria-label="Скрыть панель"]').click()`);
    }
    await writeFile(join(output,`layout-${width}x${height}.png`),Buffer.from((await send('Page.captureScreenshot',{format:'png'})).data,'base64'));
  }
  await send('Emulation.clearDeviceMetricsOverride');
  const empty = structuredClone(analysis); empty.diagnostics = [];
  await installBridge(empty);
  await reload(); await wait(`document.querySelector('.stat-card')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[4].click()`);
  await wait(`document.querySelector('.all-clear')?.textContent.includes('Правила не выдали диагностик')`);
  const legacy = structuredClone(analysis); legacy.rulesRun = null; legacy.diagnostics = [];
  await installBridge(legacy);
  await reload();
  await wait(`document.querySelector('.stat-card')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[4].click()`);
  await wait(`document.querySelector('.diagnostics-coverage')?.textContent.includes('расширенная диагностика не выполнялась')`);
  assert.equal(await evaluate(`document.querySelectorAll('.cycle-path').length`), 0);
  assert.equal(exceptions.length, 0, JSON.stringify(exceptions));
  console.log(`PASS: real engine diagnostics, rule filtering, graph scope, directory drill-down, large/empty lists, legacy snapshot; screenshots: ${output}`);
} finally {
  socket?.close(); child.kill(); await new Promise(done => server.close(done));
}
