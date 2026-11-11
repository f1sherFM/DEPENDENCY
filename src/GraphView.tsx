import { useEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
import { ArrowLeft, FileCode2, Folder, Maximize2, Network, Package, PanelRightClose, Search, ZoomIn, ZoomOut } from "lucide-react";
import type { AnalysisResult, Diagnostic, ImpactGraphScope } from "./types";
import { layoutGraph, projectGraph } from "./graph";

export function GraphView({ analysis, selected, onSelect, searchInput, detailsOpen, setDetailsOpen, diagnostic, onClearDiagnostic, onOpenDiagnostic, impactScope, onOpenImpact }: {
  analysis: AnalysisResult; selected: string | null; onSelect: (id: string) => void;
  searchInput: RefObject<HTMLInputElement | null>; detailsOpen: boolean; setDetailsOpen: (value: boolean) => void;
  diagnostic: Diagnostic | null; onClearDiagnostic: () => void; onOpenDiagnostic: () => void;
  impactScope: ImpactGraphScope | null; onOpenImpact: () => void;
}) {
  const [mode, setMode] = useState<"files" | "directories">("directories");
  const [directory, setDirectory] = useState("");
  const [query, setQuery] = useState("");
  const [showPackages, setShowPackages] = useState(true);
  const [edgeKind, setEdgeKind] = useState("");
  const [focus, setFocus] = useState<string | null>(null);
  const [direction, setDirection] = useState<"both" | "incoming" | "outgoing">("both");
  const [camera, setCamera] = useState({ x: 20, y: 20, zoom: 1 });
  const [viewport, setViewport] = useState({ width: 900, height: 600 });
  const canvas = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; cameraX: number; cameraY: number } | null>(null);
  const view = useMemo(() => projectGraph(analysis.nodes, impactScope?.edges ?? (impactScope ? analysis.edges.filter(edge => impactScope.edgeKinds.includes(edge.kind)) : analysis.edges), {
    mode, directory, query, showPackages, edgeKind, focus, direction, limit: 200, nodeIds: impactScope?.nodeIds ?? diagnostic?.nodeIds, priorityIds: impactScope?.roots,
  }), [analysis, mode, directory, query, showPackages, edgeKind, focus, direction, diagnostic, impactScope]);
  const layout = useMemo(() => layoutGraph(view.nodes, view.edges, focus), [view, focus]);
  const kinds = useMemo(() => [...new Set(analysis.edges.map(edge => edge.kind))].sort(), [analysis]);
  const selectedNode = analysis.nodes.find(node => node.id === selected);
  const fit = () => {
    const zoom = Math.max(0.02, Math.min(1.4, (viewport.width - 40) / layout.width, (viewport.height - 40) / layout.height));
    setCamera({ zoom, x: (viewport.width - layout.width * zoom) / 2, y: (viewport.height - layout.height * zoom) / 2 });
  };
  useEffect(() => {
    if (!canvas.current) return;
    const observer = new ResizeObserver(([entry]) => setViewport({ width: entry.contentRect.width, height: entry.contentRect.height }));
    observer.observe(canvas.current); return () => observer.disconnect();
  }, []);
  useEffect(() => { fit(); }, [layout, viewport.width, viewport.height]);
  useEffect(() => {
    if (diagnostic || impactScope) { setMode("files"); setDirectory(""); setQuery(""); setEdgeKind(""); setShowPackages(true); setFocus(null); return; }
    if (!selected) return;
    // A jump from the context panel must reveal its target, even across filters.
    setMode("files"); setDirectory(""); setQuery(""); setShowPackages(true); setFocus(selected);
  }, [selected, diagnostic, impactScope]);
  const zoomAt = (factor: number, x = viewport.width / 2, y = viewport.height / 2) => setCamera(previous => {
    const zoom = Math.max(0.02, Math.min(3, previous.zoom * factor));
    return { zoom, x: x - (x - previous.x) * zoom / previous.zoom, y: y - (y - previous.y) * zoom / previous.zoom };
  });
  const reset = () => { onClearDiagnostic(); setMode("directories"); setDirectory(""); setQuery(""); setEdgeKind(""); setFocus(null); setShowPackages(true); onSelect(""); };
  return <div className="graph-workspace">
    {impactScope && <div className="graph-navigation impact-graph-context"><b>{impactScope.title}</b><span>Выборка: {impactScope.shownAffectedCount} потребителей из {impactScope.partial ? "не менее " : ""}{impactScope.knownAffectedCount}. {impactScope.partial && "Обход неполный."} Корни выделены зелёным.</span><button onClick={onOpenImpact}>К анализу влияния</button><button onClick={reset}>Весь проект</button></div>}
    {diagnostic && <div className="graph-navigation"><b>{diagnostic.title}</b><code>{diagnostic.ruleId}</code><button onClick={onOpenDiagnostic}>К диагностике</button><button onClick={reset}>Весь проект</button></div>}
    <div className="graph-toolbar"><div className="graph-tabs">
      <button className={`graph-tab ${mode === "files" ? "active" : ""}`} onClick={() => setMode("files")}><FileCode2 size={15} />Файлы</button>
      <button className={`graph-tab ${mode === "directories" ? "active" : ""}`} onClick={() => { setMode("directories"); setFocus(null); }}> <Folder size={15} />Каталоги</button>
    </div><div className="graph-tools"><label className="graph-search"><Search size={15} /><input ref={searchInput} value={query} onChange={event => setQuery(event.target.value)} placeholder="Путь или имя…" aria-label="Поиск узла" /><kbd>Ctrl K</kbd></label>
      <label className="filter-check"><input type="checkbox" checked={showPackages} onChange={event => setShowPackages(event.target.checked)} />Пакеты</label>
      <select aria-label="Тип связи" value={edgeKind} onChange={event => setEdgeKind(event.target.value)}><option value="">Все типы связей</option>{kinds.map(kind => <option key={kind}>{kind}</option>)}</select>
      {!detailsOpen && <button className="icon-button" onClick={() => setDetailsOpen(true)} title="Показать контекст"><PanelRightClose size={17} /></button>}
    </div></div>
    <div className="graph-navigation"><button onClick={reset}>Весь проект</button>{directory && <><span>/ {directory}</span><button onClick={() => setDirectory(directory.split("/").slice(0, -1).join("/"))}><ArrowLeft size={13} />Выше</button></>}
      <button disabled={!selectedNode} onClick={() => { setFocus(selected); setMode("files"); setDirectory(""); setQuery(""); }}>Окружение узла</button>
      {focus && <><select aria-label="Направление связей" value={direction} onChange={event => setDirection(event.target.value as typeof direction)}><option value="both">Входящие и исходящие</option><option value="incoming">Кто импортирует</option><option value="outgoing">Что импортирует</option></select><button onClick={() => setFocus(null)}>Убрать фокус</button></>}
    </div>
    <div className="graph-canvas interactive-canvas" ref={canvas} onWheel={event => {
      const bounds = event.currentTarget.getBoundingClientRect(); zoomAt(event.deltaY < 0 ? 1.12 : 1 / 1.12, event.clientX - bounds.left, event.clientY - bounds.top);
    }} onPointerDown={event => {
      if (event.button !== 0 || (event.target as Element).closest(".graph-node")) return;
      event.currentTarget.setPointerCapture(event.pointerId); drag.current = { x: event.clientX, y: event.clientY, cameraX: camera.x, cameraY: camera.y };
    }} onPointerMove={event => {
      if (drag.current) { const start = drag.current; setCamera(previous => ({ ...previous, x: start.cameraX + event.clientX - start.x, y: start.cameraY + event.clientY - start.y })); }
    }} onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }}>
      <svg width="100%" height="100%" role="group" aria-label="Интерактивный граф зависимостей">
        <defs><marker id="graph-arrow" markerWidth="8" markerHeight="8" refX="8" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" fill="#8b9abb" /></marker></defs>
        <g transform={`translate(${camera.x},${camera.y}) scale(${camera.zoom})`}>
          {view.edges.map(edge => {
            const a = layout.positions.get(edge.source)!; const b = layout.positions.get(edge.target)!;
            const forward = b.x > a.x; const sx = a.x + (forward ? 190 : 0); const tx = b.x + (forward ? 0 : 190);
            const path = edge.source === edge.target ? `M ${a.x + 140} ${a.y} C ${a.x + 260} ${a.y - 65}, ${a.x + 250} ${a.y + 100}, ${a.x + 190} ${a.y + 40}`
              : `M ${sx} ${a.y + 30} C ${sx + (forward ? 70 : -70)} ${a.y + 30}, ${tx + (forward ? -70 : 70)} ${b.y + 30}, ${tx} ${b.y + 30}`;
            return <g key={edge.id}><path d={path} className={`graph-edge ${edge.kind === "type_only_import" ? "type-edge" : ""} ${selected === edge.source || selected === edge.target ? "highlight" : ""}`} markerEnd="url(#graph-arrow)"><title>{edge.kind} · {edge.count} импортов</title></path>{edge.count > 1 && <text className="edge-count" x={(sx + tx) / 2} y={(a.y + b.y) / 2 + 23}>{edge.count}</text>}</g>;
          })}
          {view.nodes.map(node => { const p = layout.positions.get(node.id)!; const Icon = node.kind === "directory" ? Folder : node.kind === "package" ? Package : FileCode2;
            const activate = () => { if (node.kind === "directory") { setDirectory(node.path); setFocus(null); } else onSelect(node.id); };
            return <g key={node.id} role="button" aria-label={`${node.kind === "directory" ? "Открыть каталог" : "Выбрать"} ${node.path}`} className={`graph-node ${node.kind} ${diagnostic || impactScope ? "diagnostic-member" : ""} ${impactScope?.roots.some(root => node.members.includes(root)) ? "impact-root" : ""} ${selected === node.id ? "selected" : ""}`} transform={`translate(${p.x},${p.y})`} onClick={activate} tabIndex={0} onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); activate(); } }}>
              <title>{node.path}{node.kind === "directory" ? ` · ${node.members.length} файлов. Нажмите для раскрытия.` : ""}</title><rect width="190" height="60" rx="10" /><Icon x={12} y={20} size={18} /><text x="40" y="25" className="node-label">{node.label.length > 21 ? `${node.label.slice(0, 20)}…` : node.label}</text><text x="40" y="44" className="node-caption">{node.kind === "directory" ? `${node.members.length} файлов` : node.kind === "package" ? "Внешний пакет" : node.language ?? "Ресурс"} · ↓{node.incoming} ↑{node.outgoing}</text>
            </g>;
          })}
        </g>
      </svg>
      {!view.nodes.length && <div className="empty-graph"><Search size={20} /><b>Узлы не найдены</b><span>Измените поиск, каталог или фильтры.</span><button onClick={reset}>Сбросить фильтры</button></div>}
    </div>
    <div className="graph-coverage" role="status">{view.nodes.length} из {view.totalNodes} узлов выборки · {view.edges.reduce((sum, edge) => sum + edge.count, 0)} импортов · {view.edges.length} стрелок
      {(view.hiddenNodes > 0 || view.hiddenEdges > 0) && <b> · Скрыто: {view.hiddenNodes} узлов, {view.hiddenEdges} импортов. Уточните поиск или раскройте каталог.</b>}
      {view.internalEdges > 0 && <span> · {view.internalEdges} импортов внутри свёрнутых каталогов</span>}
      {view.boundaryEdges > 0 && <span> · {view.boundaryEdges} связей за границами представления</span>}
    </div>
    <div className="graph-footer"><span className="graph-count">Стрелка: импортирующий → зависимость · Пунктир: type-only</span><span className="graph-count">{focus ? `Фокус: ${selectedNode?.path ?? focus}` : "Перетащите фон · Колесо: масштаб"}</span><div className="graph-tools"><button className="icon-button" title="Отдалить" onClick={() => zoomAt(1 / 1.2)}><ZoomOut size={17} /></button><span>{Math.round(camera.zoom * 100)}%</span><button className="icon-button" title="Приблизить" onClick={() => zoomAt(1.2)}><ZoomIn size={17} /></button><button className="icon-button" title="Вписать граф" onClick={fit}><Maximize2 size={17} /></button><Network size={15} /></div></div>
  </div>;
}
