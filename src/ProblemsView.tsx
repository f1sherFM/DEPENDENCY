import { useMemo, useState } from "react";
import { AlertTriangle, Network, ShieldCheck } from "lucide-react";
import type { AnalysisResult, Diagnostic, GraphEdge } from "./types";

const severityNames = { info: "ИНФОРМАЦИЯ", warning: "ПРЕДУПРЕЖДЕНИЕ", error: "ОШИБКА" };
export function ProblemsView({ analysis, onSelect, onShowGraph, activeDiagnosticId }: { analysis: AnalysisResult; onSelect: (id: string) => void; onShowGraph: (diagnostic: Diagnostic) => void; activeDiagnosticId?: string | null }) {
  const [rule, setRule] = useState(""); const [severity, setSeverity] = useState(""); const [category, setCategory] = useState(""); const [limit, setLimit] = useState(50);
  const diagnostics = analysis.diagnostics ?? [];
  const rules = useMemo(() => [...new Set(diagnostics.map(item => item.ruleId))].sort(), [diagnostics]);
  const filtered = diagnostics.filter(item => (!rule || item.ruleId === rule) && (!severity || item.severity === severity) && (!category || item.category === category));
  const activeDiagnostic = filtered.find(item => item.id === activeDiagnosticId);
  const ordered = activeDiagnostic ? [activeDiagnostic, ...filtered.filter(item => item !== activeDiagnostic)] : filtered;
  const architectureWarnings = diagnostics.filter(item => item.category === "architecture" && item.severity !== "info").length;
  const coverageWarnings = diagnostics.filter(item => item.category === "coverage" && item.severity !== "info").length;
  if (!analysis.rulesRun) return <section className="problems-list">
    <div className="diagnostics-coverage"><b>Исторический снимок · расширенная диагностика не выполнялась</b><p>Показаны сохранённые сведения прежнего анализатора. Для запуска новых правил создайте новый снимок пересканированием проекта.</p></div>
    {analysis.summary.cycles.slice(0, limit).map(component => <article className="problem-card" key={component.join("|")}><AlertTriangle size={18} /><div className="problem-copy"><b>Сильно связанная компонента · {component.length} файлов</b><p>Порядок файлов не является маршрутом цикла.</p><details><summary>Состав компоненты · показано {Math.min(limit, component.length)} из {component.length}</summary>{component.slice(0, limit).map(id => <button className="issue-source" key={id} onClick={() => onSelect(id)}>{id}</button>)}</details></div></article>)}
    {analysis.summary.resolutionIssues.slice(0, limit).map((issue, index) => <article className="problem-card" key={index}><div className="problem-copy"><b>{issue.specifier}</b><button className="issue-source" onClick={() => onSelect(issue.source)}>{issue.source}:{issue.line}</button><p>{issue.reason}</p><p>{issue.hint}</p></div></article>)}
    {(analysis.summary.cycles.length > limit || analysis.summary.cycles.some(component => component.length > limit) || analysis.summary.resolutionIssues.length > limit) && <button className="text-button" onClick={() => setLimit(limit + 50)}>Показать ещё сохранённые сведения</button>}
    {analysis.summary.parseErrorCount > 0 && <p>{analysis.summary.parseErrorCount} файлов с ошибками разбора.</p>}
  </section>;
  const run = analysis.rulesRun;
  return <section className="problems-list">
    <div className="diagnostics-coverage"><b>Архитектурных предупреждений: {architectureWarnings} · Предупреждений покрытия: {coverageWarnings}</b><p>Исходников: {run.sourceFileCount} · Выполнено правил: {run.completedRules.length} · Пропущено: {run.skippedRules.length} · {run.durationMs} мс</p><p>{run.limitation}</p>
      {run.coverageIssueCount > 0 && <p className="coverage-warning">Зафиксировано проблем покрытия: {run.coverageIssueCount}. Некоторые связи могут отсутствовать.</p>}
      {run.skippedRules.map(item => <p key={item.ruleId}><code>{item.ruleId}</code>: {item.reason}</p>)}
      <details><summary>Правила и настройки этого снимка</summary><code>{run.version} · {run.configHash.slice(0, 12)}</code><pre>{JSON.stringify(run.effectiveConfig, null, 2)}</pre></details>
    </div>
    <div className="diagnostics-filters"><label>Правило<select value={rule} onChange={event => { setRule(event.target.value); setLimit(50); }}><option value="">Все</option>{rules.map(name => <option key={name}>{name}</option>)}</select></label><label>Важность<select value={severity} onChange={event => { setSeverity(event.target.value); setLimit(50); }}><option value="">Все</option>{Object.entries(severityNames).map(([value, name]) => <option key={value} value={value}>{name}</option>)}</select></label><label>Категория<select value={category} onChange={event => { setCategory(event.target.value); setLimit(50); }}><option value="">Все</option><option value="architecture">Архитектура</option><option value="coverage">Покрытие анализа</option></select></label><span>{filtered.length} из {diagnostics.length}</span></div>
    {ordered.slice(0, limit).map(item => <DiagnosticCard key={item.id} diagnostic={item} onSelect={onSelect} onShowGraph={onShowGraph} />)}
    {filtered.length > limit && <button className="text-button" onClick={() => setLimit(limit + 50)}>Показать ещё ({filtered.length - limit})</button>}
    {!filtered.length && <div className="all-clear"><ShieldCheck size={24} /><h2>{diagnostics.length ? "Нет результатов по выбранным фильтрам" : "Правила не выдали диагностик"}</h2><p>Результат относится к покрытию и настройкам этого снимка.</p></div>}
  </section>;
}

function DiagnosticCard({ diagnostic: item, onSelect, onShowGraph }: { diagnostic: Diagnostic; onSelect: (id: string) => void; onShowGraph: (item: Diagnostic) => void }) {
  const [limit, setLimit] = useState(40);
  const evidence = item.evidence;
  const links = evidence.type === "cycle" ? evidence.route : evidence.type === "coupling" ? evidence.imports : [];
  const nodes = evidence.type === "cycle" ? evidence.component : evidence.type === "coupling" ? evidence.neighbors : item.nodeIds;
  const renderEdge = (edge: GraphEdge, index: number) => <li key={`${edge.id}:${index}`}><button className="issue-source" onClick={() => onSelect(edge.source)}>{edge.source}:{edge.line}</button><span> → </span><button className="issue-source" onClick={() => onSelect(edge.target)}>{edge.target}</button><small> · {edge.kind}</small></li>;
  return <article className="problem-card diagnostic-card"><div className={`problem-icon ${item.severity === "info" ? "info" : "warn"}`}><AlertTriangle size={18} /></div><div className="problem-copy">
    <span className={`problem-severity ${item.severity === "info" ? "blue" : "amber"}`}>{severityNames[item.severity]} · {item.category === "coverage" ? "ПОКРЫТИЕ" : "АРХИТЕКТУРА"}</span><b>{item.title}</b><small>{item.ruleId} · {item.confidence === "confirmed" ? "подтверждено данными снимка" : "эвристика — требует проверки"}</small><p>{item.explanation}</p>
    {evidence.type === "cycle" && <p>Граф: {evidence.graph} · Типы рёбер: {evidence.edgeKinds.join(", ")}</p>}
    {evidence.type === "coupling" && <p>Уникальных соседей: {evidence.observed} · Порог: {evidence.threshold}</p>}
    {links.length > 0 && <details open><summary>{evidence.type === "cycle" ? "Подтверждённый замкнутый маршрут" : "Импорты — основание метрики"} · {links.length}</summary><ol className="diagnostic-edges">{links.slice(0, limit).map(renderEdge)}</ol>{links.length > limit && <p>Показаны первые {limit} рёбер. Маршрут показан частично.</p>}</details>}
    {evidence.type === "resolution" && <><button className="issue-source" onClick={() => onSelect(evidence.issue.source)}>{evidence.issue.source}:{evidence.issue.line} · {evidence.issue.specifier}</button><div className="issue-candidates">{evidence.issue.attemptedCandidates.slice(0, limit).map((path, index) => <code key={index}>{path}</code>)}</div></>}
    {evidence.type === "parse" && <p><code>{evidence.source}</code>: {evidence.reason}</p>}
    {evidence.type === "consumers" && <><p>Проверенные исключения: {evidence.checkedExclusions.join("; ")}</p><p>{evidence.limitation}</p></>}
    {nodes.length > 0 && <details><summary>{evidence.type === "cycle" ? "Полный состав компоненты" : "Затронутые узлы"} · {nodes.length}</summary><div className="diagnostic-members">{nodes.slice(0, limit).map(id => <button className="issue-source" key={id} onClick={() => onSelect(id)}>{id}</button>)}</div></details>}
    {(nodes.length > limit || links.length > limit) && <button className="text-button" onClick={() => setLimit(limit + 40)}>Показать ещё доказательства</button>}
    <p className="issue-hint">{item.suggestion}</p>{item.nodeIds.length > 0 && <button className="text-button" onClick={() => onShowGraph(item)}><Network size={14} />Показать в графе · {item.nodeIds.length} узлов</button>}
  </div></article>;
}
