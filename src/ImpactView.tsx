import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowRight, Network, RefreshCw, X } from "lucide-react";
import type { AnalysisResult, ImpactGraphScope, ImpactPage, ImpactPath, ImpactRow, ImpactSummary, SnapshotRecord } from "./types";

import { impactFindings, isCurrentImpact } from "./researchJourney";

const kinds = ["import", "reexport", "commonjs_import", "type_only_import", "dynamic_import", "python_import", "rust_import", "conditional_import", "module_declaration"];
const kindLabels: Record<string, string> = { import: "import", reexport: "re-export", commonjs_import: "require", type_only_import: "type-only", dynamic_import: "import()", python_import: "Python import", rust_import: "Rust use", conditional_import: "Условный импорт", module_declaration: "Rust mod (структура)" };
const reasons: Record<string, string> = { "depth-limit": "Ограничение глубины", "node-budget": "Лимит посещённых узлов", "edge-budget": "Лимит проверенных рёбер", "time-budget": "Лимит времени обхода" };

export function ImpactView({ analysis, snapshot, initialRoots, onGraph,onBackToFile,onSaveFindings,externalBusy }: { analysis: AnalysisResult; snapshot: SnapshotRecord; initialRoots: string[]; onGraph: (scope: ImpactGraphScope) => void;onBackToFile?:()=>void;onSaveFindings:(summary:ImpactSummary,note:string)=>void;externalBusy:boolean }) {
  const [roots, setRoots] = useState(initialRoots); const [picker, setPicker] = useState("");
  const [edgeKinds, setEdgeKinds] = useState(() => [...kinds.slice(0,5),...kinds.slice(5).filter(kind=>analysis.edges.some(edge=>edge.kind===kind))]); const [depth, setDepth] = useState(""); const [retry, setRetry] = useState(0);
  const [summary, setSummary] = useState<ImpactSummary | null>(null); const [page, setPage] = useState<ImpactPage | null>(null);
  const [search, setSearch] = useState(""); const [testsOnly, setTestsOnly] = useState(false);
  const [proof, setProof] = useState<{ row: ImpactRow; path: ImpactPath } | null>(null);
  const [busy, setBusy] = useState(false); const [pageBusy, setPageBusy] = useState(false); const [pathBusy, setPathBusy] = useState(false); const [error, setError] = useState<string | null>(null);
  const generation = useRef(0); const pageSequence = useRef(0); const pathSequence = useRef(0);
  const files = useMemo(() => analysis.nodes.filter(node => node.kind === "file"), [analysis]);
  const choices = picker.trim() ? files.filter(node => !roots.includes(node.id) && node.path.toLowerCase().includes(picker.trim().toLowerCase())).slice(0, 20) : [];
  useEffect(() => { setRoots(initialRoots); }, [initialRoots]);
  useEffect(() => {
    const token = ++generation.current; ++pageSequence.current; ++pathSequence.current;
    setSummary(null); setPage(null); setProof(null); setError(null); setPathBusy(false); setPageBusy(false);
    if (!roots.length || !edgeKinds.length) { setBusy(false); return; }
    setBusy(true);
    const timer = window.setTimeout(() => {
      void invoke<ImpactSummary>("get_impact", { request: { snapshotId: snapshot.id, roots, edgeKinds, maxDepth: depth ? Number(depth) : null } }).then(result => {
        if (generation.current !== token) return;
        if (result.snapshotId !== snapshot.id) throw new Error("Impact Analysis вернул другой снимок. Повторите запрос.");
        setSummary(result); setPage(result.firstPage);
      }).catch(reason => { if (generation.current === token) setError(String(reason)); }).finally(() => { if (generation.current === token) setBusy(false); });
    }, 200);
    return () => { window.clearTimeout(timer); if (generation.current === token) ++generation.current; };
  }, [snapshot.id, roots, edgeKinds, depth, retry]);
  useEffect(() => {
    const sequence = ++pageSequence.current; const token = generation.current;
    if (!summary) return;
    setPageBusy(true);
    const timer = window.setTimeout(() => {
      void invoke<ImpactPage>("get_impact_page", { queryId: summary.queryId, offset: 0, limit: 200, text: search, testsOnly }).then(result => {
        if (token === generation.current && sequence === pageSequence.current) setPage(result);
      }).catch(reason => { if (token === generation.current && sequence === pageSequence.current) setError(String(reason)); }).finally(() => { if (token === generation.current && sequence === pageSequence.current) setPageBusy(false); });
    }, 150);
    return () => { window.clearTimeout(timer); ++pageSequence.current; };
  }, [summary, search, testsOnly]);
  const loadPage = async (offset: number) => {
    if (!summary) return; const token = generation.current; const sequence = ++pageSequence.current; setPageBusy(true); setError(null);
    try { const result = await invoke<ImpactPage>("get_impact_page", { queryId: summary.queryId, offset, limit: 200, text: search, testsOnly }); if (token === generation.current && sequence === pageSequence.current) setPage(result); }
    catch (reason) { if (token === generation.current && sequence === pageSequence.current) setError(String(reason)); }
    finally { if (token === generation.current && sequence === pageSequence.current) setPageBusy(false); }
  };
  const loadPath = async (row: ImpactRow, offset = 0) => {
    if (!summary) return; const token = generation.current; const sequence = ++pathSequence.current; setPathBusy(true); setError(null);
    try { const path = await invoke<ImpactPath>("get_impact_path", { queryId: summary.queryId, nodeId: row.nodeId, offset, limit: 200 }); if (token === generation.current && sequence === pathSequence.current) setProof({ row, path }); }
    catch (reason) { if (token === generation.current && sequence === pathSequence.current) setError(String(reason)); }
    finally { if (token === generation.current && sequence === pathSequence.current) setPathBusy(false); }
  };
  const graph = (path = false) => {
    if (!summary || !page) return;
    const edges = path ? proof?.path.edges : undefined;
    const ids = edges ? edges.flatMap(edge => [edge.source, edge.target]) : page.items.map(row => row.nodeId);
    onGraph({ queryId: summary.queryId, title: path ? `Путь: ${proof?.row.path} · сегмент с ребра ${proof!.path.offset + 1}` : "Потребители — текущая страница", roots: summary.roots,
      nodeIds: [...new Set([...summary.roots, ...ids])], edges, edgeKinds: summary.effectiveEdgeKinds,
      knownAffectedCount: summary.knownAffectedCount, shownAffectedCount: new Set(ids.filter(id => !summary.roots.includes(id))).size, partial: summary.status === "partial" });
  };
  const prefix = summary?.status === "partial" ? "не менее " : "";
  return <section className="impact-view">
    <div className="impact-journey-tools">{onBackToFile&&<button onClick={onBackToFile}>Вернуться к файлу</button>}<button disabled={externalBusy||busy||pageBusy||pathBusy||!!error||!isCurrentImpact(summary,snapshot.id,roots,edgeKinds,depth)} onClick={()=>{if(summary&&isCurrentImpact(summary,snapshot.id,roots,edgeKinds,depth))onSaveFindings(summary,impactFindings(summary,{search,testsOnly},proof?.path??null));}}>Сохранить выводы</button></div>
    <div className="panel impact-controls"><b>Исходные файлы · {roots.length}/32</b><div className="impact-roots">{roots.map(id => <button key={id} onClick={() => setRoots(roots.filter(root => root !== id))} title="Убрать исходный файл">{id}<X size={12} /></button>)}</div>
      <label>Добавить файл<input aria-label="Найти исходный файл" value={picker} onChange={event => setPicker(event.target.value)} placeholder="Путь или имя файла…" /></label>
      {choices.length > 0 && <div className="impact-choices">{choices.map(node => <button key={node.id} disabled={roots.length >= 32} onClick={() => { setRoots([...roots, node.id]); setPicker(""); }}>{node.path}</button>)}<small>Показано до 20 совпадений. Уточните путь для поиска.</small></div>}
      <div className="impact-options"><button className="text-button" onClick={() => setEdgeKinds(kinds.filter((kind,index)=>index<5||analysis.edges.some(edge=>edge.kind===kind)))}>{analysis.edges.some(edge=>edge.kind==="module_declaration")?"Все связи":"Все импорты"}</button><button className="text-button" onClick={() => setEdgeKinds(["import","reexport","commonjs_import",...kinds.filter(kind=>["python_import","rust_import"].includes(kind)&&analysis.edges.some(edge=>edge.kind===kind))])}>Статические</button>{kinds.map(kind => <label key={kind} title={kind}><input type="checkbox" checked={edgeKinds.includes(kind)} onChange={() => setEdgeKinds(edgeKinds.includes(kind) ? edgeKinds.filter(item => item !== kind) : [...edgeKinds, kind])} />{kindLabels[kind]}</label>)}<label>Глубина<select aria-label="Глубина влияния" value={depth} onChange={event => setDepth(event.target.value)}><option value="">Без ограничения</option>{[1, 2, 3, 5, 10, 20, 64].map(value => <option key={value}>{value}</option>)}</select></label></div>
      <small>Снимок {snapshot.id} · {new Date(snapshot.createdAt).toLocaleString("ru-RU")} · {snapshot.analyzerVersion}</small>
    </div>
    {!roots.length && <div className="all-clear"><Network size={25} /><h2>Выберите файл для анализа влияния</h2><p>Ищем тех, кто прямо или транзитивно импортирует выбранные файлы.</p></div>}
    {!edgeKinds.length && <p>Выберите хотя бы один тип импорта.</p>}
    {busy && <p role="status">Исследуем потребителей в сохранённом графе…</p>}
    {pathBusy && <p role="status">Загружаем доказательный путь…</p>}
    {error && <div className="error-banner" role="alert">{error}<button onClick={() => setRetry(retry + 1)}><RefreshCw size={14} />Повторить запрос</button></div>}
    {summary && <>
      <div className="diagnostics-coverage impact-summary"><b>{summary.status === "partial" ? "Частичный обход" : "Обход выбранного графа завершён"}</b><p>{summary.partialReasons.map(reason => reasons[reason] ?? reason).join(" · ")}</p><div className="impact-counts"><span>Потребителей: <b>{prefix}{summary.knownAffectedCount}</b></span><span>Прямых: {prefix}{summary.knownDirectCount}</span><span>Транзитивных: {prefix}{summary.knownTransitiveCount}</span><span>Кандидатов тестов: {prefix}{summary.knownTestCandidateCount}</span><span>Макс. расстояние: {summary.maxKnownDistance}</span></div>
        <p>Это потенциальная затронутость по графу, а не прогноз поломки. Тестовые файлы определяются по имени, достаточность набора тестов не гарантируется.</p><b>{summary.coverage.state === "known-gaps" ? "В снимке есть пробелы покрытия" : summary.coverage.state === "legacy-unknown" ? "Историческое покрытие известно не полностью" : "В снимке не зарегистрированы ошибки покрытия"}</b>{summary.coverage.reasons.map(reason => <p key={reason}>{reason}</p>)}
        <details><summary>Параметры и стоимость сохранённого запроса</summary><p>{summary.impactVersion} · {summary.analyzerVersion} · {summary.rulesVersion}</p><p>Первоначальное чтение: {summary.readMs} мс · построение индекса: {summary.indexMs} мс · обход/сортировка: {summary.elapsedMs} мс · оценка индекса: {(summary.estimatedIndexBytes / 1048576).toFixed(2)} МиБ. Повторные запросы могут использовать кэш.</p><p>Посещено узлов: {summary.visitedNodeCount} · проверено рёбер: {summary.examinedEdgeCount} · бюджеты: {summary.budgets.maxNodes} узлов / {summary.budgets.maxEdges} рёбер / {summary.budgets.maxMillis} мс обхода</p></details>
      </div>
      <div className="impact-list-tools"><label>Поиск потребителя<input aria-label="Поиск потребителя" value={search} onChange={event => setSearch(event.target.value)} placeholder="Фильтр списка, не обхода…" /></label><label><input type="checkbox" checked={testsOnly} onChange={event => setTestsOnly(event.target.checked)} />Только кандидаты тестов</label><button className="text-button" disabled={!page?.items.length || pageBusy} onClick={() => graph()}><Network size={14} />Страница в графе</button></div>
      {page && <div className="panel table-panel"><div className="table-wrap"><table className="impact-table"><thead><tr><th>ПОТРЕБИТЕЛЬ</th><th>РАССТОЯНИЕ</th><th>КОРЕНЬ ПУТИ</th><th>КОНТЕКСТ</th><th /></tr></thead><tbody>{page.items.map(row => <tr key={row.nodeId}><td>{row.path}</td><td>{row.distance} · {row.distance === 1 ? "прямой" : "транзитивный"}</td><td>{row.viaRootId}</td><td>{row.isTestCandidate && <span>кандидат теста </span>}{row.hasRecordedCycleDiagnostic && <span>сохранённая диагностика цикла</span>}</td><td><button className="text-button" disabled={pathBusy} onClick={() => void loadPath(row)}>Почему затронут</button></td></tr>)}</tbody></table></div>{!page.items.length && <p className="impact-empty">{summary.knownAffectedCount ? "Нет совпадений по фильтрам списка." : "В выбранном графе не найдены потребители. Это не означает, что изменение безопасно."}</p>}<div className="impact-pagination"><button disabled={pageBusy || !page.offset} onClick={() => void loadPage(Math.max(0, page.offset - 200))}>Предыдущая</button><span>{pageBusy ? "Обновляем страницу…" : `${page.items.length ? page.offset + 1 : 0}–${page.offset + page.items.length} из ${prefix}${page.knownMatchingCount}`}</span><button disabled={pageBusy || page.nextOffset === null} onClick={() => page.nextOffset !== null && void loadPage(page.nextOffset)}>Следующая</button></div></div>}
      {proof && <div className="panel impact-proof"><h3>Почему затронут: {proof.row.path}</h3><p>Один кратчайший путь до {proof.row.viaRootId}. Стрелки направлены от импортирующего файла к зависимости.</p><p>Показаны рёбра {proof.path.offset + 1}–{proof.path.offset + proof.path.edges.length} из {proof.path.totalEdgeCount}. {proof.path.reachesRoot ? "Сегмент достигает исходного файла." : "Маршрут показан частично, продолжение доступно ниже."}</p><ol start={proof.path.offset + 1}>{proof.path.edges.map((edge, index) => <li key={`${edge.id}:${index}`}><code>{edge.source}:{edge.line}</code> <ArrowRight size={12} /> <code>{edge.target}</code><small> · {edge.kind}</small></li>)}</ol><div className="impact-pagination"><button disabled={pathBusy || !proof.path.offset} onClick={() => void loadPath(proof.row, Math.max(0, proof.path.offset - 200))}>Предыдущий сегмент</button><button disabled={pathBusy || proof.path.nextOffset === null} onClick={() => proof.path.nextOffset !== null && void loadPath(proof.row, proof.path.nextOffset)}>Следующий сегмент</button><button className="text-button" onClick={() => graph(true)}>Показать сегмент в графе</button></div></div>}
    </>}
  </section>;
}
