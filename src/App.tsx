import { useEffect, useRef, useState } from "react";
import { GraphView } from "./GraphView";
import { useAgentObservation } from "./agentObservation";
import { InvestigationDialog, InvestigationsView } from "./InvestigationsView";
import type { InvestigationDraft } from "./InvestigationsView";
import { ProblemsView } from "./ProblemsView";
import { ImpactView } from "./ImpactView";
import { GitView } from "./GitView";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle, ArrowDownUp, ArrowLeft, ArrowRight, Box, Braces, ChevronDown,
  CircleDot, Clock3, Code2, FileCode2, Folder, FolderOpen,
  Layers3, LayoutDashboard, LoaderCircle, Network, Package, PanelRightClose,
  Plus, RefreshCw, XCircle, Settings2, Share2, ShieldCheck, Sparkles, Unplug,

} from "lucide-react";
import type { AnalysisResult, Diagnostic, GraphNode, ImpactGraphScope, ProjectRecord, ProjectView, SnapshotRecord, GitJob, Investigation, SavedGraphState } from "./types";

type View = "overview" | "graph" | "modules" | "packages" | "problems" | "history" | "impact" | "git" | "investigations";
const navItems: { id: View; label: string; icon: typeof LayoutDashboard }[] = [
  { id: "overview", label: "Обзор", icon: LayoutDashboard },
  { id: "graph", label: "Зависимости", icon: Network },
  { id: "modules", label: "Модули", icon: Layers3 },
  { id: "packages", label: "Пакеты", icon: Package },
  { id: "problems", label: "Проблемы", icon: AlertTriangle },
  { id: "history", label: "Снимки", icon: Clock3 },
  { id: "impact", label: "Анализ влияния", icon: Share2 },
  { id: "git", label: "Git и сравнение", icon: Code2 },
  { id: "investigations", label: "Исследования", icon: Folder },
];

function App() {
  const [analysis, setAnalysis] = useState<AnalysisResult | null>(null);
  const [currentProject, setCurrentProject] = useState<ProjectView | null>(null);
  const [projects, setProjects] = useState<ProjectRecord[]>([]);
  const [snapshots, setSnapshots] = useState<SnapshotRecord[]>([]);
  const [view, setView] = useState<View>("overview");
  const [impactRoots, setImpactRoots] = useState<string[]>([]);
  const [impactScope, setImpactScope] = useState<ImpactGraphScope | null>(null);
  const [graphDiagnostic, setGraphDiagnostic] = useState<Diagnostic | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [detailsOpen, setDetailsOpen] = useState(() => window.innerWidth > 1080);
  const [gitVisited, setGitVisited] = useState(false);
  const [gitJobId, setGitJobId] = useState<string | null>(null);
  const [graphRestore,setGraphRestore]=useState<{key:string;graph:SavedGraphState}|null>(null);
  const [activeInvestigation,setActiveInvestigation]=useState<Investigation|null>(null);
  const [researchDraft,setResearchDraft]=useState<InvestigationDraft|null>(null);
  const [researchRevision,setResearchRevision]=useState(0);
  const agentStatus = useAgentObservation({ projectId: currentProject?.project.id ?? null, snapshotId: currentProject?.snapshot.id ?? null, view, selectedNodeId: selected, busy });
  const navigationGeneration = useRef(0);
  const graphSearch = useRef<HTMLInputElement>(null);
  const mainContent = useRef<HTMLElement>(null);
  useEffect(() => { mainContent.current?.scrollTo(0, 0); }, [view, currentProject?.snapshot.id]);
  useEffect(() => {
    const narrow = window.matchMedia("(max-width: 1080px)");
    const onResize = () => { if (narrow.matches) setDetailsOpen(false); };
    narrow.addEventListener("change", onResize);
    return () => narrow.removeEventListener("change", onResize);
  }, []);

  const refreshProjects = async () => setProjects(await invoke<ProjectRecord[]>("list_projects"));
  useEffect(() => {
    const token = ++navigationGeneration.current;
    void invoke<ProjectRecord[]>("list_projects").then(async (saved) => {
      if (token !== navigationGeneration.current) return;
      setProjects(saved);
      if (saved[0]) {
        try { const project = await invoke<ProjectView>("load_project", { projectId: saved[0].id }); if (token === navigationGeneration.current) showProject(project); }
        catch (reason) { if (token === navigationGeneration.current) setError(String(reason)); }
      }
    }).catch((reason) => { if (token === navigationGeneration.current) setError(String(reason)); });
  }, []);
  useEffect(() => {
    if (!currentProject) { setSnapshots([]); return; }
    let live = true;
    void invoke<SnapshotRecord[]>("list_snapshots", { projectId: currentProject.project.id }).then(result => { if (live) setSnapshots(result); }).catch((reason) => { if (live) setError(String(reason)); });
    return () => { live = false; };
  }, [currentProject?.project.id, currentProject?.snapshot.id]);

  const showProject = (project: ProjectView) => {
    setGraphRestore(null);setActiveInvestigation(null);setResearchDraft(null);
    if (currentProject?.project.id !== project.project.id) setGitVisited(false);
    setCurrentProject(project); setAnalysis(project.result); setGraphDiagnostic(null); setImpactScope(null); setImpactRoots([]); setSelected(null); setView("overview");
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
    if (busy) return;
    const token = ++navigationGeneration.current;
    let root = path;
    setBusy(true); setError(null);
    try {
      if (!root) {
        const choice = await open({ directory: true, multiple: false, title: "Выберите папку проекта" });
        if (!choice || Array.isArray(choice)) return;
        root = choice;
      }
      if (token !== navigationGeneration.current) return;
      if (!force) {
        const saved = await invoke<ProjectView | null>("open_project", { root });
        if (saved) { if (token === navigationGeneration.current) showProject(saved); return; }
      }
      let result: ProjectView;
      if (await invoke<boolean>("discover_git", { root })) {
        const id = await invoke<string>("start_git_analysis", { root, reference: null }); setGitJobId(id);
        while (true) {
          const job = await invoke<GitJob | null>("get_git_job", { jobId: id });
          if (!job) throw new Error("Git job недоступен");
          if (job.done) { if (job.error || !job.snapshotId) throw new Error(job.error ?? "Git analysis failed"); result = await invoke<ProjectView>("load_snapshot", { snapshotId: job.snapshotId }); break; }
          await new Promise(resolve => window.setTimeout(resolve, 400));
        }
      } else { result = await invoke<ProjectView>("analyze_project", { root }); }
      if (token === navigationGeneration.current) showProject(result);
    } catch (reason) {
      if (token === navigationGeneration.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally { if (token === navigationGeneration.current) { setBusy(false); setGitJobId(null); } }
  };

  const openSavedProject = async (id: string) => {
    if (busy) return;
    const token = ++navigationGeneration.current;
    setBusy(true); setError(null);
    try { const project = await invoke<ProjectView>("load_project", { projectId: id }); if (token === navigationGeneration.current) showProject(project); }
    catch (reason) { if (token === navigationGeneration.current) setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { if (token === navigationGeneration.current) { setBusy(false); setGitJobId(null); } }
  };

  const openSnapshot = async (id: string) => {
    if (busy) return;
    const token = ++navigationGeneration.current;
    setBusy(true); setError(null);
    try { const project = await invoke<ProjectView>("load_snapshot", { snapshotId: id }); if (token === navigationGeneration.current) { showProject(project); setView("history"); } }
    catch (reason) { if (token === navigationGeneration.current) setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { if (token === navigationGeneration.current) { setBusy(false); setGitJobId(null); } }
  };

  const selectNode = (id: string) => { setSelected(id); if (!graphDiagnostic?.nodeIds.includes(id)) setGraphDiagnostic(null); if (!impactScope?.nodeIds.includes(id)) setImpactScope(null); };
  const openImpact = (id: string) => { setImpactRoots([id]); setImpactScope(null); setGraphDiagnostic(null); setView("impact"); };
  const openGitSnapshot = async (snapshotId: string, nodeId?: string, impact = false) => {
    if (busy) return;
    const token = ++navigationGeneration.current;
    const project = await invoke<ProjectView>("load_snapshot", { snapshotId });
    if (token !== navigationGeneration.current) return;
    showProject(project); setGitVisited(true);
    if (nodeId && project.result.nodes.some(node => node.id === nodeId)) { setSelected(nodeId); if (impact) setImpactRoots([nodeId]); }
    setView(impact && nodeId ? "impact" : "graph");
  };
  const selectedNode = analysis?.nodes.find((node) => node.id === selected) ?? null;
  const detachResearch=()=>{setActiveInvestigation(null);setGraphRestore(null);};

  const openResearch=async(id:string,edit=false)=>{
    if(busy||!currentProject)return;
    const token=++navigationGeneration.current;setBusy(true);setError(null);
    try{
      const saved=await invoke<Investigation>("load_investigation",{id});
      if(token!==navigationGeneration.current)return;
      if(saved.projectId!==currentProject.project.id)throw new Error("Исследование относится к другому проекту");
      if(edit){setResearchDraft({snapshotId:saved.snapshotId,graph:saved.graph,existing:saved});return;}
      const project=await invoke<ProjectView>("load_snapshot",{snapshotId:saved.snapshotId});
      if(token!==navigationGeneration.current)return;
      showProject(project);setSelected(saved.graph.selectedNodeId);setDetailsOpen(saved.graph.detailsOpen);
      setGraphRestore({key:crypto.randomUUID(),graph:saved.graph});setActiveInvestigation(saved);setView("graph");
    }catch(reason){if(token===navigationGeneration.current)setError(String(reason));}
    finally{if(token===navigationGeneration.current){setBusy(false);setGitJobId(null);}}
  };
  const saveResearchGraph=(graph:SavedGraphState)=>{
    if(!currentProject||busy)return;
    setResearchDraft({snapshotId:currentProject.snapshot.id,graph,existing:activeInvestigation?.snapshotId===currentProject.snapshot.id?activeInvestigation:undefined});
  };
  const researchSaved=(saved:Investigation)=>{
    setResearchDraft(null);setResearchRevision(revision=>revision+1);
    if(view==="graph"&&saved.snapshotId===currentProject?.snapshot.id){
      setActiveInvestigation(saved);setGraphRestore({key:crypto.randomUUID(),graph:saved.graph});
    }else if(activeInvestigation?.id===saved.id)setActiveInvestigation(saved);
  };

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
          {agentStatus !== "off" && <span className="agent-badge" title="Локальное наблюдение для CLI/MCP. Изменение проектов через этот канал недоступно.">{agentStatus === "active" ? "Агент · чтение" : "Агент · нет связи"}</span>}
          {analysis && <span className="local-badge"><ShieldCheck size={13} /> Локальный анализ</span>}
          <button className="icon-button" disabled title="Настройки пока недоступны" aria-label="Настройки пока недоступны"><Settings2 size={18} /></button>
          <div className="avatar">D</div>
        </div>
      </header>

      <aside className="sidebar">
        <button type="button" className="sidebar-project" disabled={busy} onClick={() => void scan()} aria-label="Открыть другой проект" title="Открыть другой проект">
          <span className="sidebar-project-icon"><FolderOpen size={17} /></span>
          <span className="sidebar-project-copy"><b>{analysis?.summary.name ?? "Ваш проект"}</b><span>{analysis?.summary.rootPath ?? "Выберите локальный репозиторий"}</span></span>
          <ChevronDown size={15} className="muted" />
        </button>
        <div className="sidebar-caption">ПРОЕКТ</div>
        <nav className="main-nav" aria-label="Разделы проекта">
          {navItems.map(({ id, label, icon: Icon }) => <button key={id} className={`nav-item ${view === id ? "active" : ""}`} onClick={() => { if (id === "git") setGitVisited(true); setView(id); }}>
            <Icon size={18} strokeWidth={1.8} /><span>{label}</span>
            {analysis && <span className="nav-count" title={id === "problems" ? "Архитектурные предупреждения. Ограничения анализа показаны отдельно в разделе." : undefined}>{id === "graph" ? analysis.summary.dependencyCount : id === "modules" ? analysis.summary.moduleCount : id === "packages" ? analysis.summary.externalPackageCount : id === "problems" ? (analysis.rulesRun ? analysis.diagnostics.filter(item => item.category === "architecture" && item.severity !== "info").length : analysis.summary.cycleCount) : ""}</span>}
          </button>)}
        </nav>
        <div className="sidebar-caption recent-caption">СОХРАНЁННЫЕ ПРОЕКТЫ</div>
        <div className="recent-projects">{projects.slice(0, 6).map((project) => <button key={project.id} className={`recent-project ${currentProject?.project.id === project.id ? "selected" : ""}`} onClick={() => void openSavedProject(project.id)} title={project.rootPath}><Folder size={14} /><span>{project.name}</span></button>)}</div>
        <div className="sidebar-divider" />
        <div className="sidebar-caption">БЫСТРЫЕ ДЕЙСТВИЯ</div>
        <button className="quick-action" onClick={() => void scan()}><Plus size={17} /> Открыть проект</button>
        <button className="quick-action" disabled={!analysis || busy} onClick={() => analysis && void scan(analysis.summary.rootPath, true)}><RefreshCw size={16} className={busy ? "spin" : ""} /> Пересканировать</button>
        <div className="sidebar-bottom"><span className="engine-status"><i /> Engine готов</span><span className="version">v0.9.0</span></div>
      </aside>

      <main ref={mainContent} className={`main-content ${view === "graph" ? "graph-page" : ""} ${analysis && view === "graph" && detailsOpen ? "with-details" : "without-details"}`}>
        {gitJobId && <button className="text-button" onClick={() => void invoke("cancel_git_job", { jobId: gitJobId }).catch(reason => setError(String(reason)))}>Отменить Git-анализ</button>}
        {!analysis ? <Welcome busy={busy} error={error} onOpen={() => void scan()} /> : <>
          <div className="page-heading">
            <div><div className="eyebrow"><span className="crumb">{analysis.summary.name}</span><ArrowRight size={12} /><span>{navItems.find((item) => item.id === view)?.label}</span></div>
              <h1>{view === "overview" ? "Обзор проекта" : view === "graph" ? "Граф зависимостей" : view === "modules" ? "Модули проекта" : view === "packages" ? "Внешние пакеты" : view === "history" ? "История снимков" : view === "impact" ? "Анализ влияния" : view === "git" ? "Git и сравнение архитектуры" : view === "investigations" ? "Исследования проекта" : "Проблемы анализа"}</h1>
              <p className="subheading" title={analysis.summary.rootPath}>{view === "overview" ? "Структура и связи, обнаруженные в выбранном репозитории." : `${analysis.summary.rootPath}`}</p>
            </div>
            <button className="primary-button" disabled={busy} onClick={() => void scan(analysis.summary.rootPath, true)}>{busy ? <LoaderCircle className="spin" size={16} /> : <RefreshCw size={16} />}{busy ? "Анализируем…" : "Запустить анализ"}</button>
          </div>
          {analysis.analysisProfile && <details className="panel analysis-profile"><summary>Профиль анализа · {Object.keys(analysis.analysisProfile.analyzers).join(", ")}</summary>{analysis.analysisProfile.limitations.map(text=><p key={text}>{text}</p>)}{analysis.analysisProfile.analyzers.Python && <p>Python source roots: {analysis.analysisProfile.pythonRoots.map(root=>root||"корень").join(", ")}</p>}<p>Rust mod обозначает структуру, use — зависимость. Условные импорты не доказывают выполнение.</p></details>}
          {analysis.basis && <details className="snapshot-basis"><summary>Источник снимка · {analysis.basis.kind === "git-worktree" ? "Рабочая копия Git" : analysis.basis.kind === "git-commit" ? "Коммит Git" : analysis.basis.kind}</summary><p><b>{analysis.basis.kind}</b> · {analysis.basis.commitOid ?? analysis.basis.observedHeadOid ?? "unborn HEAD"} · {analysis.basis.resolutionProfile} · {analysis.basis.captureConsistency}</p></details>}
          {error && <div className="error-banner"><AlertTriangle size={16} />{error}<button onClick={() => setError(null)}><XCircle size={15} /></button></div>}
          {view === "overview" && <Overview analysis={analysis} onOpenGraph={() => {detachResearch();setView("graph");}} onSelect={id => { detachResearch();setGraphDiagnostic(null); setImpactScope(null); setSelected(id); setView("graph"); }} />}
          {view === "graph" && activeInvestigation && <div className="graph-navigation research-context"><b>Исследование: {activeInvestigation.name}</b><span>Сохранённый снимок {activeInvestigation.snapshotId.slice(0,8)}</span><button onClick={()=>setView("investigations")}>К исследованиям</button><button onClick={()=>{detachResearch();setSelected(null);setGraphDiagnostic(null);setImpactScope(null);}}>Закрыть исследование</button></div>}
          {view === "graph" && <GraphView key={`${currentProject?.snapshot.id}:${graphRestore?.key??"default"}`} analysis={analysis} selected={selected} onSelect={selectNode} searchInput={graphSearch} detailsOpen={detailsOpen} setDetailsOpen={setDetailsOpen} diagnostic={graphDiagnostic} onClearDiagnostic={() => { setGraphDiagnostic(null); setImpactScope(null); }} impactScope={impactScope} onOpenImpact={() => setView("impact")} onOpenDiagnostic={() => setView("problems")} savedGraph={graphRestore?.graph} onSaveResearch={saveResearchGraph} busy={busy} />}
          {view === "investigations" && currentProject && <InvestigationsView key={currentProject.project.id} projectId={currentProject.project.id} currentSnapshotId={currentProject.snapshot.id} revision={researchRevision} busy={busy} onOpen={id=>void openResearch(id)} onEdit={id=>void openResearch(id,true)} onDeleted={id=>{if(activeInvestigation?.id===id){detachResearch();setSelected(null);}}} />}
          {view === "modules" && <ModulesView analysis={analysis} onSelect={(id) => { detachResearch();selectNode(id); setView("graph"); }} />}
          {view === "packages" && <PackagesView analysis={analysis} />}
          {view === "problems" && <ProblemsView key={currentProject?.snapshot.id} activeDiagnosticId={graphDiagnostic?.id} analysis={analysis} onShowGraph={(diagnostic) => { detachResearch();setImpactScope(null); setGraphDiagnostic(diagnostic); setSelected(null); setView("graph"); }} onSelect={(id) => { detachResearch();selectNode(id); setView("graph"); }} />}
          {view === "history" && <HistoryView snapshots={snapshots} current={currentProject?.snapshot.id ?? ""} onOpen={openSnapshot} />}
          {currentProject && (gitVisited || view === "git") && <div hidden={view !== "git"}><GitView key={currentProject.project.id} root={analysis.summary.rootPath} projectId={currentProject.project.id} snapshot={currentProject.snapshot} analysis={analysis} externalBusy={busy} onOpen={openGitSnapshot} /></div>}
          {currentProject && <div hidden={view !== "impact"}><ImpactView key={currentProject.snapshot.id} analysis={analysis} snapshot={currentProject.snapshot} initialRoots={impactRoots} onGraph={(scope) => { detachResearch();setImpactScope(scope); setGraphDiagnostic(null); setSelected(null); setView("graph"); }} /></div>}
          <div className="status-line"><span><ShieldCheck size={13} /> Исходный код не запускается</span><span><Clock3 size={13} /> Снимок {formatDate(currentProject?.snapshot.createdAt ?? 0)} · анализатор {currentProject?.snapshot.analyzerVersion ?? "—"}</span></div>
        </>}
      </main>

      {analysis && view === "graph" && detailsOpen && <aside className="details-panel"><div className="details-head"><div><span className="sidebar-caption">КОНТЕКСТ</span><h3>{selectedNode ? "Выбранный узел" : "Сводка проекта"}</h3></div><button className="icon-button" onClick={() => setDetailsOpen(false)} aria-label="Скрыть панель"><PanelRightClose size={17} /></button></div>
        {selectedNode ? <NodeDetails key={selectedNode.id} node={selectedNode} analysis={analysis} onSelect={selectNode} onImpact={openImpact} /> : <>
          <div className="detail-empty"><div className="detail-empty-icon"><CircleDot size={20} /></div><b>Выберите узел графа</b><span>Здесь появятся сведения о связях, языке и размере модуля.</span></div>
          <div className="detail-section"><div className="detail-section-title">СОСТАВ ГРАФА</div><DetailMetric label="Исходные файлы" value={analysis.summary.sourceFileCount} /><DetailMetric label="Внутренние связи" value={analysis.summary.dependencyCount} /><DetailMetric label="Внешние пакеты" value={analysis.summary.packages.length} /></div>
          <div className="detail-section"><div className="detail-section-title">ЯЗЫКИ</div>{analysis.summary.languages.map((language) => <div className="language-row" key={language.name}><span><i className="language-dot" />{language.name}</span><b>{language.count}</b></div>)}</div>
        </>}
      </aside>}
      {researchDraft&&<InvestigationDialog draft={researchDraft} onClose={()=>setResearchDraft(null)} onSaved={researchSaved}/>}
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
  const width = 780; const height = 310; const cols = 6; const pos = new Map(files.map((n, i) => [n.id, { x: 45 + (i % cols) * 124, y: 36 + Math.floor(i / cols) * 44 }]));
  const shown = new Set(files.map((n) => n.id));
  return <div className="mini-graph"><svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label="Предпросмотр графа файлов">
    {analysis.edges.filter((e) => e.kind !== "package" && shown.has(e.source) && shown.has(e.target)).slice(0, 90).map((e) => { const a = pos.get(e.source)!; const b = pos.get(e.target)!; return <path key={e.id} d={`M ${a.x + 70} ${a.y + 14} C ${(a.x + b.x) / 2} ${a.y + 14}, ${(a.x + b.x) / 2} ${b.y + 14}, ${b.x} ${b.y + 14}`} className="mini-edge" />; })}
    {files.map((node) => { const p = pos.get(node.id)!; return <g key={node.id} className="mini-node" onClick={() => onSelect(node.id)}><rect x={p.x} y={p.y} width="78" height="27" rx="7" /><circle cx={p.x + 11} cy={p.y + 13} r="3" /><text x={p.x + 19} y={p.y + 17}>{node.label.length > 10 ? `${node.label.slice(0, 9)}…` : node.label}</text></g>; })}
  </svg>{analysis.nodes.filter((n) => n.kind === "file").length > 36 && <div className="graph-overflow">Показаны первые 36 узлов предпросмотра. Откройте полный граф для всей выборки.</div>}</div>;
}

function NodeDetails({ node, analysis, onSelect, onImpact }: { node: GraphNode; analysis: AnalysisResult; onSelect: (id: string) => void; onImpact: (id: string) => void }) {
  const [relationLimit, setRelationLimit] = useState(50);
  const inbound = analysis.edges.filter((e) => e.target === node.id); const outbound = analysis.edges.filter((e) => e.source === node.id);
  const resolve = (id: string) => analysis.nodes.find((n) => n.id === id)?.path ?? id;
  const NodeIcon = node.kind === "package" ? Package : FileCode2;
  return <div className="node-details"><div className={`node-detail-icon ${node.kind}`}><NodeIcon size={20} /></div><h2>{node.label}</h2><div className="node-detail-path">{node.path}</div><div className="node-tags"><span>{node.kind === "package" ? "ВНЕШНИЙ ПАКЕТ" : node.language?.toUpperCase() ?? "РЕСУРС"}</span>{node.kind === "package" && <span>{node.declared ? "ОБЪЯВЛЕН" : "НЕ ОБЪЯВЛЕН"}</span>}</div>
    {node.kind === "file" && <button className="text-button impact-action" onClick={() => onImpact(node.id)}><Share2 size={14} />Анализ влияния</button>}
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
  return <section className="panel table-panel"><div className="table-toolbar"><div><b>{analysis.summary.externalPackageCount}</b><span>пакетов и деклараций обнаружено</span></div><div className="table-tools"><span className="table-chip"><Package size={13} /> npm · Cargo · PyPI</span></div></div><div className="table-wrap"><table><thead><tr><th>ПАКЕТ</th><th>ЗАДАННЫЙ ДИАПАЗОН</th><th>СОСТОЯНИЕ</th><th>ИМПОРТОВ</th></tr></thead><tbody>{analysis.summary.packages.map((pkg) => <tr key={pkg.name}><td><span className="file-cell package-cell"><Package size={16} />{pkg.name}</span></td><td><code>{pkg.version}</code></td><td><span className={`status-chip ${pkg.dependencyType === "объявлен" || pkg.name.startsWith("Cargo ·") || pkg.name.startsWith("PyPI ·") ? "ok" : "warning"}`}>{pkg.dependencyType}</span></td><td>{pkg.importedBy}</td></tr>)}</tbody></table>{!analysis.summary.packages.length && <div className="table-empty"><Package size={21} /><b>Импортируемые пакеты не найдены</b><span>Граф отражает пакеты, которые удалось извлечь из импортов.</span></div>}</div><p className="table-note">Читаются package.json, Cargo.toml, pyproject.toml и requirements.txt. Декларация пакета не подтверждает установленную версию. Python import names не сопоставляются с PyPI автоматически.</p></section>;
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
