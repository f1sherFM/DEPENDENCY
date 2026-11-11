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
    const timeout = setTimeout(() => reject(new Error('Chromium debugging endpoint timeout')), 15000);
    child.on('error', reject);
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
    throw new Error(`UI condition failed: ${expression}`);
  };
  const bridge = result => `(() => {
    const result = ${JSON.stringify(result)};
    const gitFixture=${JSON.stringify(fixture.git ?? null)};
    const project = gitFixture?.views.find(view=>view.snapshot.id===${JSON.stringify(fixture.snapshot?.id??"")})?.project ?? { id: ${JSON.stringify(fixture.snapshot?.projectId??'test-project')}, name: result.summary.name, rootPath: result.summary.rootPath, lastOpenedAt: 0, latestSnapshotId: 'snapshot', latestSnapshotAt: 0 };
    const snapshot = ${JSON.stringify(fixture.snapshot ?? null)} ?? { id: 'snapshot', projectId: project.id, createdAt: 0, analyzerVersion: 'test-dump', rulesVersion: result.rulesRun?.version ?? 'legacy', parsedFileCount: 0, reusedFileCount: 0 };
    const impactCases = ${JSON.stringify(fixture.cases ?? [])};
    const key = request => JSON.stringify([request.roots.slice().sort(),request.edgeKinds.slice().sort(),request.maxDepth]);
    window.__impactCalls = [];
    window.__TAURI_INTERNALS__ = { invoke: async (cmd, args) => {
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
  await send('Page.addScriptToEvaluateOnNewDocument', { source: bridge(analysis) });
  await send('Page.navigate', { url: origin });
  await wait(`document.querySelector('.main-nav') && document.querySelector('.stat-card')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[4].click()`);
  await wait(`document.querySelector('.diagnostics-coverage') && document.querySelector('.diagnostic-card')`);
  assert.ok(await evaluate(`document.querySelector('.diagnostics-coverage').textContent.includes('Архитектурных предупреждений')`));
  await evaluate(`(() => { const select=document.querySelector('.diagnostics-filters select'); select.value='dependency-cycle'; select.dispatchEvent(new Event('change',{bubbles:true})); })()`);
  await wait(`document.querySelector('.diagnostic-card')?.textContent.includes('Подтверждённый замкнутый маршрут')`);
  const screenshot = await send('Page.captureScreenshot', { format: 'png' });
  await writeFile(join(output, 'diagnostics.png'), Buffer.from(screenshot.data, 'base64'));
  await evaluate(`document.querySelector('.diagnostic-card .text-button:last-child').click()`);
  await wait(`document.querySelector('.graph-workspace .graph-navigation code')?.textContent === 'dependency-cycle'`);
  assert.ok(await evaluate(`document.querySelectorAll('.graph-node').length > 0`));
  assert.ok(await evaluate(`document.querySelectorAll('.graph-edge').length > 0`));
  await evaluate(`Array.from(document.querySelectorAll('.graph-navigation button')).find(button=>button.textContent==='К диагностике').click()`);
  await wait(`document.querySelector('.diagnostics-filters')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[1].click()`);
  await wait(`document.querySelector('.graph-workspace')`);
  await evaluate(`Array.from(document.querySelectorAll('.graph-navigation button')).find(button=>button.textContent==='Весь проект').click()`);
  await wait(`document.querySelector('.graph-node.directory')`);
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
  const large = structuredClone(analysis);
  large.diagnostics = Array.from({ length: 125 }, (_, index) => ({ ...analysis.diagnostics[0], id: `large-${index}` }));
  await send('Page.addScriptToEvaluateOnNewDocument', { source: bridge(large) });
  await send('Page.reload'); await wait(`document.querySelector('.stat-card')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[4].click()`);
  await wait(`document.querySelectorAll('.diagnostic-card').length === 50`);
  await evaluate(`document.querySelector('.problems-list > button').click()`);
  await wait(`document.querySelectorAll('.diagnostic-card').length === 100`);
  const empty = structuredClone(analysis); empty.diagnostics = [];
  await send('Page.addScriptToEvaluateOnNewDocument', { source: bridge(empty) });
  await send('Page.reload'); await wait(`document.querySelector('.stat-card')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[4].click()`);
  await wait(`document.querySelector('.all-clear')?.textContent.includes('Правила не выдали диагностик')`);
  const legacy = structuredClone(analysis); legacy.rulesRun = null; legacy.diagnostics = [];
  await send('Page.addScriptToEvaluateOnNewDocument', { source: bridge(legacy) });
  await send('Page.reload');
  await wait(`document.querySelector('.stat-card')`);
  await evaluate(`document.querySelectorAll('.main-nav button')[4].click()`);
  await wait(`document.querySelector('.diagnostics-coverage')?.textContent.includes('расширенная диагностика не выполнялась')`);
  assert.equal(await evaluate(`document.querySelectorAll('.cycle-path').length`), 0);
  assert.equal(exceptions.length, 0, JSON.stringify(exceptions));
  console.log(`PASS: real engine diagnostics, rule filtering, graph scope, directory drill-down, large/empty lists, legacy snapshot; screenshots: ${output}`);
} finally {
  socket?.close(); child.kill(); await new Promise(done => server.close(done));
}
