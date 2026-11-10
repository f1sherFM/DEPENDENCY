import { useEffect, useRef, useState } from "react";
import { GraphView } from "./GraphView";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle, ArrowDownUp, ArrowLeft, ArrowRight, Box, Braces, ChevronDown,
  CircleDot, Clock3, Code2, FileCode2, Folder, FolderOpen,
  Layers3, LayoutDashboard, LoaderCircle, Network, Package, PanelRightClose,
  Plus, RefreshCw, XCircle, Settings2, Share2, ShieldCheck, Sparkles, Unplug,

} from "lucide-react";
import type { AnalysisResult, GraphNode, ProjectRecord, ProjectView, SnapshotRecord } from "./types";

type View = "overview" | "graph" | "modules" | "packages" | "problems" | "history";
const navItems: { id: View; label: string; icon: typeof LayoutDashboard }[] = [
  { id: "overview", label: "Обзор", icon: LayoutDashboard },
  { id: "graph", label: "Зависимости", icon: Network },
  { id: "modules", label: "Модули", icon: Layers3 },
  { id: "packages", label: "Пакеты", icon: Package },
  { id: "problems", label: "Проблемы", icon: AlertTriangle },
  { id: "history", label: "Снимки", icon: Clock3 },
];

function App() {
  const [analysis, setAnalysis] = useState<AnalysisResult | null>(null);
  const [currentProject, setCurrentProject] = useState<ProjectView | null>(null);
  const [projects, setProjects] = useState<ProjectRecord[]>([]);
  const [snapshots, setSnapshots] = useState<SnapshotRecord[]>([]);
  const [view, setView] = useState<View>("overview");
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(true);
  const graphSearch = useRef<HTMLInputElement>(null);

  const refreshProjects = async () => setProjects(await invoke<ProjectRecord[]>("list_projects"));
  useEffect(() => {
    void invoke<ProjectRecord[]>("list_projects").then(async (saved) => {
      setProjects(saved);
      if (saved[0]) {
        try { showProject(await invoke<ProjectView>("load_project", { projectId: saved[0].id })); }
        catch (reason) { setError(String(reason)); }
      }
    }).catch((reason) => setError(String(reason)));
  }, []);
  useEffect(() => {
    if (!currentProject) { setSnapshots([]); return; }
    void invoke<SnapshotRecord[]>("list_snapshots", { projectId: currentProject.project.id }).then(setSnapshots).catch((reason) => setError(String(reason)));
  }, [currentProject?.project.id, currentProject?.snapshot.id]);

  const showProject = (project: ProjectView) => {
    setCurrentProject(project); setAnalysis(project.result); setSelected(null); setView("overview");
    void refreshProjects().catch(() => {});
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault(); graphSearch.current?.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  const scan = async (path?: string, force = false) => {
    let root = path;
    if (!root) {
      const choice = await open({ directory: true, multiple: false, title: "Выберите папку проекта" });
      if (!choice || Array.isArray(choice)) return;
      root = choice;
    }
    setBusy(true); setError(null);
    try {
      if (!force) {
        const saved = await invoke<ProjectView | null>("open_project", { root });
        if (saved) { showProject(saved); return; }
      }
      const result = await invoke<ProjectView>("analyze_project", { root });
      showProject(result);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally { setBusy(false); }
  };

  const openSavedProject = async (id: string) => {
    setBusy(true); setError(null);
    try { showProject(await invoke<ProjectView>("load_project", { projectId: id })); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  };

  const openSnapshot = async (id: string) => {
    setBusy(true); setError(null);
    try { showProject(await invoke<ProjectView>("load_snapshot", { snapshotId: id })); setView("history"); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  };

  const selectedNode = analysis?.nodes.find((node) => node.id === selected) ?? null;

  return (
    <div className="app-shell">
      <header className="topbar">
        <button className="brand" onClick={() => setView("overview")} aria-label="DEPENDENCY — обзор">
          <span className="brand-mark"><Box size={23} strokeWidth={2.3} /></span><span>DEPENDENCY</span>
        </button>
        <div className="top-project">
          <span className="top-project-label">РАБОЧЕЕ ПРОСТРАНСТВО</span>
          <span className="top-project-name">{analysis?.summary.name ?? "Проект не выбран"}</span>
        </div>
        <div className="top-actions">
          {analysis && <span className="local-badge"><ShieldCheck size={13} /> Локальный анализ</span>}
          <button className="icon-button" title="Настройки" aria-label="Настройки"><Settings2 size={18} /></button>
          <div className="avatar">D</div>
        </div>
      </header>

      <aside className="sidebar">
        <div className="sidebar-project">
          <div className="sidebar-project-icon"><FolderOpen size={17} /></div>
          <div className="sidebar-project-copy"><b>{analysis?.summary.name ?? "Ваш проект"}</b><span>{analysis?.summary.rootPath ?? "Выберите локальный репозиторий"}</span></div>
          <ChevronDown size={15} className="muted" />
        </div>
        <div className="sidebar-caption">ПРОЕКТ</div>
        <nav className="main-nav" aria-label="Разделы проекта">
          {navItems.map(({ id, label, icon: Icon }) => <button key={id} className={`nav-item ${view === id ? "active" : ""}`} onClick={() => setView(id)}>
            <Icon size={18} strokeWidth={1.8} /><span>{label}</span>
            {analysis && <span className="nav-count">{id === "graph" ? analysis.summary.dependencyCount : id === "modules" ? analysis.summary.moduleCount : id === "packages" ? analysis.summary.externalPackageCount : id === "problems" ? analysis.summary.cycleCount + analysis.summary.unresolvedCount + analysis.summary.ambiguousCount + analysis.summary.parseErrorCount : ""}</span>}
          </button>)}
        </nav>
        <div className="sidebar-caption recent-caption">СОХРАНЁННЫЕ ПРОЕКТЫ</div>
        <div className="recent-projects">{projects.slice(0, 6).map((project) => <button key={project.id} className={`recent-project ${currentProject?.project.id === project.id ? "selected" : ""}`} onClick={() => void openSavedProject(project.id)} title={project.rootPath}><Folder size={14} /><span>{project.name}</span></button>)}</div>
        <div className="sidebar-divider" />
        <div className="sidebar-caption">БЫСТРЫЕ ДЕЙСТВИЯ</div>
        <button className="quick-action" onClick={() => void scan()}><Plus size={17} /> Открыть проект</button>
        <button className="quick-action" disabled={!analysis || busy} onClick={() => analysis && void scan(analysis.summary.rootPath, true)}><RefreshCw size={16} className={busy ? "spin" : ""} /> Пересканировать</button>
        <div className="sidebar-bottom"><span className="engine-status"><i /> Engine готов</span><span className="version">v0.4.0</span></div>
      </aside>

      <main className={`main-content ${analysis && view === "graph" && detailsOpen ? "with-details" : "without-details"}`}>
        {!analysis ? <Welcome busy={busy} error={error} onOpen={() => void scan()} /> : <>
          <div className="page-heading">
            <div><div className="eyebrow"><span className="crumb">{analysis.summary.name}</span><ArrowRight size={12} /><span>{navItems.find((item) => item.id === view)?.label}</span></div>
              <h1>{view === "overview" ? "Обзор проекта" : view === "graph" ? "Граф зависимостей" : view === "modules" ? "Модули проекта" : view === "packages" ? "Внешние пакеты" : view === "history" ? "История снимков" : "Проблемы анализа"}</h1>
              <p className="subheading">{view === "overview" ? "Структура и связи, обнаруженные в выбранном репозитории." : `${analysis.summary.rootPath}`}</p>
            </div>
            <button className="primary-button" disabled={busy} onClick={() => void scan(analysis.summary.rootPath, true)}>{busy ? <LoaderCircle className="spin" size={16} /> : <RefreshCw size={16} />}{busy ? "Анализируем…" : "Запустить анализ"}</button>
          </div>
          {error && <div className="error-banner"><AlertTriangle size={16} />{error}<button onClick={() => setError(null)}><XCircle size={15} /></button></div>}
          {view === "overview" && <Overview analysis={analysis} onOpenGraph={() => setView("graph")} onSelect={setSelected} />}
          {view === "graph" && <GraphView key={currentProject?.snapshot.id} analysis={analysis} selected={selected} onSelect={setSelected} searchInput={graphSearch} detailsOpen={detailsOpen} setDetailsOpen={setDetailsOpen} />}
          {view === "modules" && <ModulesView analysis={analysis} onSelect={(id) => { setSelected(id); setView("graph"); }} />}
          {view === "packages" && <PackagesView analysis={analysis} />}
          {view === "problems" && <ProblemsView analysis={analysis} onSelect={(id) => { setSelected(id); setView("graph"); }} />}
          {view === "history" && <HistoryView snapshots={snapshots} current={currentProject?.snapshot.id ?? ""} onOpen={openSnapshot} />}
          <div className="status-line"><span><ShieldCheck size={13} /> Исходный код не запускается</span><span><Clock3 size={13} /> Снимок {formatDate(currentProject?.snapshot.createdAt ?? 0)} · анализатор {currentProject?.snapshot.analyzerVersion ?? "—"}</span></div>
        </>}
      </main>

      {analysis && view === "graph" && detailsOpen && <aside className="details-panel"><div className="details-head"><div><span className="sidebar-caption">КОНТЕКСТ</span><h3>{selectedNode ? "Выбранный узел" : "Сводка проекта"}</h3></div><button className="icon-button" onClick={() => setDetailsOpen(false)} aria-label="Скрыть панель"><PanelRightClose size={17} /></button></div>
        {selectedNode ? <NodeDetails key={selectedNode.id} node={selectedNode} analysis={analysis} onSelect={setSelected} /> : <>
          <div className="detail-empty"><div className="detail-empty-icon"><CircleDot size={20} /></div><b>Выберите узел графа</b><span>Здесь появятся сведения о связях, языке и размере модуля.</span></div>
          <div className="detail-section"><div className="detail-section-title">СОСТАВ ГРАФА</div><DetailMetric label="Исходные файлы" value={analysis.summary.sourceFileCount} /><DetailMetric label="Внутренние связи" value={analysis.summary.dependencyCount} /><DetailMetric label="Внешние пакеты" value={analysis.summary.packages.length} /></div>
          <div className="detail-section"><div className="detail-section-title">ЯЗЫКИ</div>{analysis.summary.languages.map((language) => <div className="language-row" key={language.name}><span><i className="language-dot" />{language.name}</span><b>{language.count}</b></div>)}</div>
        </>}
      </aside>}
    </div>
  );
}

function Welcome({ busy, error, onOpen }: { busy: boolean; error: string | null; onOpen: () => void }) {
  return <div className="welcome-wrap"><div className="welcome-card"><div className="welcome-art"><div className="art-orbit orbit-a" /><div className="art-orbit orbit-b" /><div className="art-node node-a"><FileCode2 size={19} /></div><div className="art-node node-b"><Braces size={19} /></div><div className="art-node node-c"><Package size={19} /></div><div className="art-core"><Network size={30} /></div><div className="art-link link-a" /><div className="art-link link-b" /><div className="art-link link-c" /></div>
    <span className="welcome-kicker"><Sparkles size={14} /> ПОНИМАЙТЕ СВОЙ КОД</span><h1>Архитектура проекта.<br /><span>Наглядно и по делу.</span></h1><p>Откройте локальный репозиторий, чтобы построить реальную карту исходных файлов и их зависимостей. Код проекта остаётся на вашем компьютере.</p><button className="primary-button welcome-button" disabled={busy} onClick={onOpen}>{busy ? <LoaderCircle className="spin" size={17} /> : <FolderOpen size={17} />}{busy ? "Индексируем проект…" : "Открыть проект"}<ArrowRight size={16} /></button>{error && <div className="welcome-error"><AlertTriangle size={15} />{error}</div>}<div className="welcome-foot"><span><ShieldCheck size={14} /> Без запуска кода</span><span><Code2 size={14} /> JS · TS · TSX</span><span><Share2 size={14} /> Локально</span></div></div></div>;
}

function Overview({ analysis, onOpenGraph, onSelect }: { analysis: AnalysisResult; onOpenGraph: () => void; onSelect: (id: string) => void }) {
  const { summary } = analysis;
  return <div className="overview-grid"><section className="stats-grid">
    <StatCard icon={FileCode2} label="Исходные файлы" value={summary.sourceFileCount} tone="blue" meta={`${summary.sourceLines.toLocaleString("ru-RU")} строк исходника`} />
    <StatCard icon={Layers3} label="Модули" value={summary.moduleCount} tone="violet" meta="каталоги с исходниками" />
    <StatCard icon={Network} label="Внутренние связи" value={summary.dependencyCount} tone="cyan" meta="разрешённые импорты" />
    <StatCard icon={Package} label="Пакеты" value={summary.externalPackageCount} tone="gold" meta={`${summary.packages.length} импортируются в коде`} />
    <StatCard icon={AlertTriangle} label="Циклы" value={summary.cycleCount} tone={summary.cycleCount ? "red" : "green"} meta="сильно связанные компоненты" />
    <StatCard icon={Unplug} label="Не разрешено" value={summary.unresolvedCount + summary.ambiguousCount} tone={summary.unresolvedCount || summary.ambiguousCount ? "amber" : "green"} meta="импортов и неоднозначных ссылок" />
  </section>
  <section className="panel graph-preview"><div className="panel-heading"><div><span className="eyebrow">КАРТА ПРОЕКТА</span><h2>Зависимости</h2></div><button className="text-button" onClick={onOpenGraph}>Исследовать граф <ArrowRight size={14} /></button></div>
    <MiniGraph analysis={analysis} onSelect={onSelect} /></section>
  <div className="overview-lower"><section className="panel lower-panel"><div className="panel-heading"><div><span className="eyebrow">СОСТАВ КОДА</span><h2>Языки</h2></div><Code2 size={17} className="panel-icon" /></div>{summary.languages.length ? summary.languages.map((lang) => <div className="language-line" key={lang.name}><span className="language-name"><i className="language-dot" />{lang.name}</span><div className="language-track"><i style={{ width: `${Math.max(3, (lang.count / Math.max(summary.sourceFileCount, 1)) * 100)}%` }} /></div><span className="language-count">{lang.count}</span></div>) : <EmptyRow text="Поддерживаемые исходные файлы не найдены." />}</section>
    <section className="panel lower-panel"><div className="panel-heading"><div><span className="eyebrow">ПРОВЕРКА ПАРСЕРА</span><h2>Покрытие анализа</h2></div><ShieldCheck size={17} className="panel-icon" /></div><DetailMetric label="Ошибки разбора" value={summary.parseErrorCount} /><DetailMetric label="Всего файлов в проекте" value={summary.fileCount} /><DetailMetric label="Строк в исходных файлах" value={summary.sourceLines} /><div className="coverage-note">Учитываются все строки поддерживаемых исходных файлов, включая пустые и комментарии.</div></section></div>
  </div>;
}

function StatCard({ icon: Icon, label, value, tone, meta }: { icon: typeof FileCode2; label: string; value: number; tone: string; meta: string }) {
  return <div className={`stat-card tone-${tone}`}><div className="stat-top"><div className="stat-icon"><Icon size={18} /></div><span className="stat-label">{label}</span></div><div className="stat-value">{value.toLocaleString("ru-RU")}</div><div className="stat-meta">{meta}</div></div>;
}

function MiniGraph({ analysis, onSelect }: { analysis: AnalysisResult; onSelect: (id: string) => void }) {
  const files = analysis.nodes.filter((n) => n.kind === "file").slice(0, 36);
  const width = 720; const height = 260; const cols = 6; const pos = new Map(files.map((n, i) => [n.id, { x: 45 + (i % cols) * 124, y: 36 + Math.floor(i / cols) * 44 }]));
  const shown = new Set(files.map((n) => n.id));
  return <div className="mini-graph"><svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label="Предпросмотр графа файлов">
    {analysis.edges.filter((e) => e.kind !== "package" && shown.has(e.source) && shown.has(e.target)).slice(0, 90).map((e) => { const a = pos.get(e.source)!; const b = pos.get(e.target)!; return <path key={e.id} d={`M ${a.x + 70} ${a.y + 14} C ${(a.x + b.x) / 2} ${a.y + 14}, ${(a.x + b.x) / 2} ${b.y + 14}, ${b.x} ${b.y + 14}`} className="mini-edge" />; })}
    {files.map((node) => { const p = pos.get(node.id)!; return <g key={node.id} className="mini-node" onClick={() => onSelect(node.id)}><rect x={p.x} y={p.y} width="78" height="27" rx="7" /><circle cx={p.x + 11} cy={p.y + 13} r="3" /><text x={p.x + 19} y={p.y + 17}>{node.label.length > 10 ? `${node.label.slice(0, 9)}…` : node.label}</text></g>; })}
  </svg>{analysis.nodes.filter((n) => n.kind === "file").length > 36 && <div className="graph-overflow">Показаны первые 36 узлов предпросмотра. Откройте полный граф для всей выборки.</div>}</div>;
}

function NodeDetails({ node, analysis, onSelect }: { node: GraphNode; analysis: AnalysisResult; onSelect: (id: string) => void }) {
  const [relationLimit, setRelationLimit] = useState(50);
  const inbound = analysis.edges.filter((e) => e.target === node.id); const outbound = analysis.edges.filter((e) => e.source === node.id);
  const resolve = (id: string) => analysis.nodes.find((n) => n.id === id)?.path ?? id;
  const NodeIcon = node.kind === "package" ? Package : FileCode2;
  return <div className="node-details"><div className={`node-detail-icon ${node.kind}`}><NodeIcon size={20} /></div><h2>{node.label}</h2><div className="node-detail-path">{node.path}</div><div className="node-tags"><span>{node.kind === "package" ? "ВНЕШНИЙ ПАКЕТ" : node.language?.toUpperCase() ?? "РЕСУРС"}</span>{node.kind === "package" && <span>{node.declared ? "ОБЪЯВЛЕН" : "НЕ ОБЪЯВЛЕН"}</span>}</div>
    <div className="detail-section"><div className="detail-section-title">МЕТРИКИ</div><DetailMetric label="Строки кода" value={node.lines || "—"} /><DetailMetric label="Входящие" value={node.incoming} /><DetailMetric label="Исходящие" value={node.outgoing} /></div>
    <div className="detail-section"><div className="detail-section-title">ИМПОРТИРУЕТ · {outbound.length}</div>{outbound.length ? outbound.slice(0, relationLimit).map((edge) => <button className="relation-row relation-link" key={edge.id} onClick={() => onSelect(edge.target)}><ArrowRight size={13} /><span>{resolve(edge.target)}</span><small>{edge.kind === "type_only_import" ? "type" : edge.kind}</small></button>) : <EmptyRow text="Нет обнаруженных исходящих связей." />}</div>
    <div className="detail-section"><div className="detail-section-title">ИСПОЛЬЗУЕТСЯ В · {inbound.length}</div>{inbound.length ? inbound.slice(0, relationLimit).map((edge) => <button className="relation-row relation-link" key={edge.id} onClick={() => onSelect(edge.source)}><ArrowLeft size={13} /><span>{resolve(edge.source)}</span><small>:{edge.line}</small></button>) : <EmptyRow text="Входящие импорты не найдены." />}</div>
    {(inbound.length > relationLimit || outbound.length > relationLimit) && <button className="text-button" onClick={() => setRelationLimit(relationLimit + 50)}>Показать ещё связи (сейчас до {relationLimit})</button>}
  </div>;
}

function ModulesView({ analysis, onSelect }: { analysis: AnalysisResult; onSelect: (id: string) => void }) {
  const files = analysis.nodes.filter((node) => node.kind === "file" && node.language);
  return <section className="panel table-panel"><div className="table-toolbar"><div><b>{files.length.toLocaleString("ru-RU")}</b><span>исходных файлов в индексе</span></div><div className="table-tools"><span className="table-chip"><ArrowDownUp size={13} /> Путь</span><span className="table-chip">AST-анализ</span></div></div><div className="table-wrap"><table><thead><tr><th>ФАЙЛ</th><th>ЯЗЫК</th><th>СТРОКИ</th><th>ВХОДЯЩИЕ</th><th>ИСХОДЯЩИЕ</th></tr></thead><tbody>{files.map((node) => <tr key={node.id} onClick={() => onSelect(node.id)}><td><span className="file-cell"><FileCode2 size={16} />{node.path}</span></td><td><span className="small-tag">{node.language}</span></td><td>{node.lines.toLocaleString("ru-RU")}</td><td>{node.incoming}</td><td>{node.outgoing}</td></tr>)}</tbody></table>{!files.length && <EmptyRow text="Исходные файлы не найдены." />}</div></section>;
}

function PackagesView({ analysis }: { analysis: AnalysisResult }) {
  return <section className="panel table-panel"><div className="table-toolbar"><div><b>{analysis.summary.externalPackageCount}</b><span>пакетов объявлено в package.json</span></div><div className="table-tools"><span className="table-chip"><Package size={13} /> npm</span></div></div><div className="table-wrap"><table><thead><tr><th>ПАКЕТ</th><th>ЗАДАННЫЙ ДИАПАЗОН</th><th>СОСТОЯНИЕ</th><th>ИМПОРТОВ</th></tr></thead><tbody>{analysis.summary.packages.map((pkg) => <tr key={pkg.name}><td><span className="file-cell package-cell"><Package size={16} />{pkg.name}</span></td><td><code>{pkg.version}</code></td><td><span className={`status-chip ${pkg.dependencyType === "объявлен" ? "ok" : "warning"}`}>{pkg.dependencyType}</span></td><td>{pkg.importedBy}</td></tr>)}</tbody></table>{!analysis.summary.packages.length && <div className="table-empty"><Package size={21} /><b>Импортируемые пакеты не найдены</b><span>Граф отражает пакеты, которые удалось извлечь из импортов.</span></div>}</div><p className="table-note">Для первой версии считывается package.json. Обновления и уязвимости не запрашиваются.</p></section>;
}

function ProblemsView({ analysis, onSelect }: { analysis: AnalysisResult; onSelect: (id: string) => void }) {
  const { summary } = analysis;
  return <section className="problems-list">{summary.cycles.map((cycle, index) => <article className="problem-card" key={cycle.join("|")}><div className="problem-icon danger"><RefreshCw size={18} /></div><div className="problem-copy"><span className="problem-severity red">ЦИКЛ · {index + 1}</span><b>Циклическая зависимость</b><div className="cycle-path">{cycle.map((id, i) => <span key={id}><button onClick={() => onSelect(id)}>{id}</button>{i < cycle.length - 1 && <ArrowRight size={12} />}</span>)}<ArrowRight size={12} /><button onClick={() => onSelect(cycle[0])}>{cycle[0]}</button></div></div></article>)}
    {summary.resolutionIssues.map((issue) => <article className="problem-card" key={`${issue.source}:${issue.line}:${issue.specifier}`}><div className={`problem-icon ${issue.status === "external" ? "info" : "warn"}`}><Unplug size={18} /></div><div className="problem-copy"><span className={`problem-severity ${issue.status === "external" ? "blue" : "amber"}`}>{issue.status === "external" ? "ВНЕШНЯЯ ССЫЛКА" : issue.status === "ambiguous" ? "НЕОДНОЗНАЧНО" : "НЕ РАЗРЕШЕНО"}</span><b><code>{issue.specifier}</code></b><button className="issue-source" onClick={() => onSelect(issue.source)}>{issue.source}:{issue.line}</button><p>{issue.reason}</p>{issue.attemptedCandidates.length > 0 && <div className="issue-candidates"><small>Проверено / сообщено resolver’ом:</small>{issue.attemptedCandidates.map((candidate) => <code key={candidate}>{candidate}</code>)}</div>}<p className="issue-hint">{issue.hint}</p></div></article>)}
    {summary.parseErrorCount > 0 && <article className="problem-card"><div className="problem-icon warn"><AlertTriangle size={18} /></div><div className="problem-copy"><span className="problem-severity amber">ПОКРЫТИЕ</span><b>Файлы с синтаксическими ошибками</b><p>{summary.parseErrorCount} файлов не прошли полный разбор; дерево AST может содержать частичные результаты.</p></div></article>}
    {!summary.cycleCount && !summary.resolutionIssues.length && !summary.parseErrorCount && <div className="all-clear"><div><ShieldCheck size={26} /></div><h2>Критичных проблем анализа нет</h2><p>В текущем снимке не обнаружены циклы, неразрешённые импорты или ошибки разбора.</p></div>}
  </section>;
}

function EmptyRow({ text }: { text: string }) { return <div className="empty-row">{text}</div>; }
function DetailMetric({ label, value }: { label: string; value: string | number }) { return <div className="detail-metric"><span>{label}</span><b>{typeof value === "number" ? value.toLocaleString("ru-RU") : value}</b></div>; }

function formatDate(value: number) { return value ? new Date(value).toLocaleString("ru-RU") : "—"; }
function HistoryView({ snapshots, current, onOpen }: { snapshots: SnapshotRecord[]; current: string; onOpen: (id: string) => void }) {
  return <section className="panel history-panel"><div className="panel-heading"><div><span className="eyebrow">СОХРАНЁННЫЕ РЕЗУЛЬТАТЫ</span><h2>{snapshots.length} {snapshots.length === 1 ? "снимок" : "снимков"}</h2></div><Clock3 size={18} className="panel-icon" /></div>
    {!snapshots.length ? <EmptyRow text="Снимков пока нет. Запустите анализ проекта." /> : <div className="snapshot-list">{snapshots.map((snapshot) => <button key={snapshot.id} className={`snapshot-row ${snapshot.id === current ? "active" : ""}`} onClick={() => onOpen(snapshot.id)}><span className="snapshot-icon"><Clock3 size={16} /></span><span className="snapshot-copy"><b>{formatDate(snapshot.createdAt)}{snapshot.id === current && <i>ОТКРЫТ</i>}</b><small>Анализатор {snapshot.analyzerVersion} · правила {snapshot.rulesVersion}</small><small>Разобрано заново: {snapshot.parsedFileCount} · из кэша: {snapshot.reusedFileCount}</small></span><ArrowRight size={16} /></button>)}</div>}
  </section>;
}

export default App;
