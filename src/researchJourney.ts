import type { AnalysisResult, ImpactPath, ImpactSummary, SavedGraphState } from "./types";

const sameSet=(a:string[],b:string[])=>{const sorted=[...b].sort();return a.length===b.length&&[...a].sort().every((value,index)=>value===sorted[index]);};
export function isCurrentImpact(summary:ImpactSummary|null,snapshotId:string,roots:string[],edgeKinds:string[],depth:string){
  return !!summary&&summary.snapshotId===snapshotId&&sameSet(summary.roots,roots)&&sameSet(summary.effectiveEdgeKinds,edgeKinds)&&summary.maxDepth===(depth?Number(depth):null);
}
function clip(text:string,limit:number){const result=text.slice(0,limit);return /[\uD800-\uDBFF]$/.test(result)?result.slice(0,-1):result;}
export function impactFindings(summary:ImpactSummary,filters:{search:string;testsOnly:boolean},proof:ImpactPath|null){
  const prefix=summary.status==="partial"?"не менее ":"";
  const note=["Выводы анализа влияния",`Снимок: ${summary.snapshotId}`,`Версия обхода: ${summary.impactVersion}`,`Анализатор: ${summary.analyzerVersion}; правила: ${summary.rulesVersion}`,
    `Статус: ${summary.status==="partial"?"частичный обход":"обход выбранного графа завершён"}`,
    `Причины неполного обхода: ${summary.partialReasons.join(", ")||"не зарегистрированы"}`,
    "Исходные файлы:",...summary.roots.map(root=>`• ${root}`),
    `Типы связей: ${summary.effectiveEdgeKinds.join(", ")}`,`Глубина: ${summary.maxDepth??"без ограничения"}`,
    `Потребителей: ${prefix}${summary.knownAffectedCount}; прямых: ${prefix}${summary.knownDirectCount}; транзитивных: ${prefix}${summary.knownTransitiveCount}`,
    `Кандидатов тестов: ${prefix}${summary.knownTestCandidateCount}`,`Покрытие: ${summary.coverage.state}`,...summary.coverage.reasons,
    `Фильтр списка (не обхода): ${filters.search||"без поиска"}; только кандидаты тестов: ${filters.testsOnly?"да":"нет"}`,
    "Это потенциальная затронутость по сохранённому графу, не прогноз поломки и не гарантия достаточности тестов.",
    ...(proof?[`Открытый сегмент пути: рёбра ${proof.offset+1}–${proof.offset+proof.edges.length} из ${proof.totalEdgeCount}; ${proof.reachesRoot?"достигает исходного файла":"показан частично"}`,...proof.edges.map(edge=>`${edge.source}:${edge.line} → ${edge.target} · ${edge.kind}`)]:[])
  ].join("\n");
  return note.length<=14000?note:clip(note,13800)+"\n[Запись сокращена из-за размера. Остальные параметры/места не включены; изучите снимок перед выводами.]";
}
export function graphForImpactRoot(analysis:AnalysisResult,root:string):SavedGraphState|null{
  if(!analysis.nodes.some(node=>node.id===root&&node.kind==="file"))return null;
  // The focused root is placed first at (40,40) by the current layout contract.
  return {mode:"files",directory:"",query:"",showPackages:false,edgeKind:"",focus:root,direction:"incoming",selectedNodeId:root,detailsOpen:true,camera:{centerX:135,centerY:70,zoom:1}};
}
export const findingName=(label:string)=>clip(`Влияние: ${label}`,120);
