import { useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { AnalysisResult } from "./types";
import { architectureMap } from "./architecture";
import { layoutGraph } from "./graph";

export type ArchitectureLocation={directory:string;filesOnly:boolean;limit:number;relation:string|null;evidenceLimit:number;scrollLeft:number;scrollTop:number};
export const initialArchitectureLocation:ArchitectureLocation={directory:"",filesOnly:false,limit:12,relation:null,evidenceLimit:20,scrollLeft:0,scrollTop:0};
export function ArchitectureMap({ analysis, onSelect, location, onChange }: { analysis: AnalysisResult; onSelect: (id: string,edgeId?:string) => void; location:ArchitectureLocation;onChange:Dispatch<SetStateAction<ArchitectureLocation>> }) {
  const {directory,filesOnly,limit,relation,evidenceLimit}=location;
  const setRelation=(relation:string|null)=>onChange(previous=>({...previous,relation}));
  const setEvidenceLimit=(update:number|((value:number)=>number))=>onChange(previous=>({...previous,evidenceLimit:typeof update==="function"?update(previous.evidenceLimit):update}));
  const setLimit=(update:(value:number)=>number)=>onChange(previous=>({...previous,limit:update(previous.limit)}));
  const [highlight,setHighlight]=useState<string|null>(null);
  const scroll=useRef<HTMLDivElement>(null);
  useLayoutEffect(()=>{scroll.current?.scrollTo(location.scrollLeft,location.scrollTop);},[directory,filesOnly]);
  const marker=useId().replaceAll(":","");
  const view=useMemo(()=>architectureMap(analysis.nodes,analysis.edges,directory,filesOnly,limit),[analysis,directory,filesOnly,limit]);
  const layout=useMemo(()=>layoutGraph(view.nodes.map(node=>({ ...node,kind:node.kind==="file"?"file":"directory",language:null,lines:0,incoming:0,outgoing:0,declared:null })),view.edges.map(edge=>({...edge,kind:"aggregate",line:0})),null),[view]);
  const selectedRelation=view.edges.find(edge=>edge.id===relation)??view.boundaries.find(edge=>edge.id===relation);
  const mapHeight=Math.max(170,...[...layout.positions.values()].map(position=>position.y+100));
  const move=(path:string,files=false)=>{onChange({...initialArchitectureLocation,directory:path,filesOnly:files});setHighlight(null);};
  const open=(node:typeof view.nodes[number])=>node.kind==="file"?onSelect(node.id):move(node.path,node.kind==="files");
  const inspectRelation=(id:string)=>{setRelation(id);setEvidenceLimit(20);};
  return <div className="architecture-map">
    <p className="architecture-explanation">Каталоги и файлы сохранённого снимка. Стрелка: использует → зависимость. Число на стрелке — количество связей; типы и места доступны по нажатию.</p>
    <div className="architecture-navigation"><button onClick={()=>move("")}>Весь проект</button><span>/{directory}{filesOnly?" · файлы каталога":""}</span>{(directory||filesOnly)&&<button onClick={()=>move(filesOnly?directory:directory.split("/").slice(0,-1).join("/"))}>Выше</button>}</div>
    {!view.total?<p>В этой области нет индексированных файлов.</p>:<div className="architecture-scroll" ref={scroll} onScroll={event=>{const {scrollLeft,scrollTop}=event.currentTarget;onChange(previous=>previous.scrollLeft===scrollLeft&&previous.scrollTop===scrollTop?previous:{...previous,scrollLeft,scrollTop});}} onMouseLeave={()=>setHighlight(null)}>
      <div className="architecture-world" style={{width:layout.width+60,height:mapHeight}}>
        <svg width={layout.width+60} height={mapHeight} aria-label="Связи частей проекта"><defs><marker id={marker} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor" /></marker></defs>
          {view.edges.map(edge=>{const a=layout.positions.get(edge.source)!,b=layout.positions.get(edge.target)!;const forward=b.x>a.x;const x1=a.x+(forward?190:0),x2=b.x+(forward?0:190);const lane=forward?22:38;const y1=a.y+lane,y2=b.y+lane;const bend=edge.source===edge.target?80:Math.min(a.x-8,Math.max(25,Math.abs(x2-x1)/2));const d=edge.source===edge.target?`M ${a.x+190} ${a.y+20} C ${a.x+270} ${a.y-25}, ${a.x+270} ${a.y+90}, ${a.x+190} ${a.y+45}`:`M ${x1} ${y1} C ${x1+(forward?bend:-bend)} ${y1}, ${x2+(forward?-bend:bend)} ${y2}, ${x2} ${y2}`;return <g key={edge.id} className={highlight&&edge.source!==highlight&&edge.target!==highlight?"architecture-relation dimmed":"architecture-relation"}>
            <path className="architecture-hit" d={d} role="button" tabIndex={0} aria-label={`${edge.source} → ${edge.target}: ${edge.count} связей`} onClick={()=>inspectRelation(edge.id)} onKeyDown={event=>{if(event.key==="Enter"||event.key===" "){event.preventDefault();inspectRelation(edge.id);}}} />
            <path d={d} markerEnd={`url(#${marker})`} /><text style={{pointerEvents:"all",cursor:"pointer"}} onClick={()=>inspectRelation(edge.id)} x={edge.source===edge.target?a.x+245:(x1+x2)/2} y={edge.source===edge.target?a.y+32:(y1+y2)/2-6}>{edge.count}</text>
          </g>;})}
        </svg>
        {view.nodes.map(node=>{const p=layout.positions.get(node.id)!;return <button className="architecture-node" key={node.id} style={{left:p.x,top:p.y}} title={node.path||"Корень проекта"} onMouseEnter={()=>setHighlight(node.id)} onFocus={()=>setHighlight(node.id)} onBlur={()=>setHighlight(null)} onClick={()=>open(node)}><b>{node.kind==="directory"?"▸ ":""}{node.label}</b><small>{node.kind==="file"?node.path:`${node.members.length} файлов · раскрыть`}</small></button>;})}
      </div>
    </div>}
    <p className="architecture-summary">Показано {view.nodes.length} из {view.total} частей · {view.internal} связей внутри свёрнутых групп · {view.boundary} связей за границей области · {view.hiddenRelations} связей скрыто лимитом отображения.</p>
    {view.hidden>0&&<button onClick={()=>setLimit(value=>value+12)}>Показать ещё части ({view.hidden})</button>}
    <div className="architecture-relations" aria-label="Связи между частями">{view.edges.map(edge=><button key={edge.id} aria-pressed={edge.id===relation} onClick={()=>{setRelation(edge.id);setEvidenceLimit(20);}}>{view.nodes.find(node=>node.id===edge.source)!.label} → {view.nodes.find(node=>node.id===edge.target)!.label} · {edge.count}</button>)}</div>
    {!!view.boundaries.length&&<section className="architecture-boundaries"><h3>Связи за пределами области</h3>{view.boundaries.slice(0,limit).map(boundary=><div key={boundary.id}><button aria-pressed={boundary.id===relation} onClick={()=>inspectRelation(boundary.id)}>{boundary.direction==="incoming"?"Используется из":"Использует"} /{boundary.folder||" (корень)"} · {boundary.count}</button><button onClick={()=>move(boundary.folder)}>Открыть каталог</button></div>)}{view.boundaries.length>limit&&<button onClick={()=>setLimit(value=>value+12)}>Ещё внешние группы ({view.boundaries.length-limit})</button>}</section>}
    {selectedRelation&&<section className="architecture-evidence"><h3>Основание связи · {selectedRelation.count}</h3><p>Типы: {selectedRelation.kinds.join(", ")}. Показаны сохранённые связи, без повторного анализа.</p><ol>{selectedRelation.evidence.slice(0,evidenceLimit).map(edge=><li key={edge.id}><button onClick={()=>onSelect(edge.source,edge.id)}>{edge.source}:{edge.line}</button> → <button onClick={()=>onSelect(edge.target,edge.id)}>{edge.target}</button> · {edge.kind}</li>)}</ol>{selectedRelation.count>evidenceLimit&&<button onClick={()=>setEvidenceLimit(value=>value+20)}>Показать ещё места</button>}<button onClick={()=>setRelation(null)}>Закрыть сведения</button></section>}
  </div>;
}
