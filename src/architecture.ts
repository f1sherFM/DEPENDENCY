import type { GraphEdge, GraphNode } from "./types";

// Aggregate only observed file relations. Directory names are not inferred layers.
export function architectureMap(nodes: GraphNode[], edges: GraphEdge[], directory = "", filesOnly = false, limit = 12) {
  const prefix = directory ? `${directory}/` : "";
  const groups = new Map<string, { id: string; path: string; label: string; kind: "directory" | "files" | "file"; members: string[] }>();
  const membership = new Map<string, string>();
  const files = new Map(nodes.filter(node=>node.kind==="file").map(node=>[node.id,node]));
  for (const node of nodes) {
    if (node.kind !== "file" || !node.path.startsWith(prefix)) continue;
    const rest = node.path.slice(prefix.length);
    if (filesOnly && rest.includes("/")) continue;
    const folder = !filesOnly && rest.includes("/") ? prefix + rest.split("/")[0] : null;
    const id = filesOnly ? node.id : folder ? `directory:${folder}` : `files:${directory}`;
    const group = groups.get(id) ?? { id, path: folder ?? (filesOnly ? node.path : directory), label: folder ? rest.split("/")[0] : filesOnly ? node.label : "Файлы каталога", kind: folder ? "directory" : filesOnly ? "file" : "files", members: [] };
    group.members.push(node.id); groups.set(id, group); membership.set(node.id, id);
  }
  const relations = new Map<string, { id: string; source: string; target: string; count: number; kinds: string[]; evidence: GraphEdge[] }>();
  let internal = 0, boundary = 0;
  const boundaries = new Map<string,{id:string;folder:string;direction:"incoming"|"outgoing";count:number;kinds:string[];evidence:GraphEdge[]}>();
  for (const edge of edges) {
    // External packages are a separate view; no invented boundary nodes.
    if (edge.kind === "package") continue;
    const source = membership.get(edge.source), target = membership.get(edge.target);
    if (!source || !target) {
      const outside=files.get(source?edge.target:edge.source);
      if ((source || target)&&outside) {
        boundary++;
        const direction=source?"outgoing":"incoming";
        const folder=outside.path.split('/').slice(0,-1).join('/');
        const id=JSON.stringify(["boundary",direction,folder]);
        const relation=boundaries.get(id)??{id,folder,direction,count:0,kinds:[],evidence:[]};
        relation.count++;relation.evidence.push(edge);
        if(!relation.kinds.includes(edge.kind))relation.kinds.push(edge.kind);
        boundaries.set(id,relation);
      }
      continue;
    }
    if (source === target && groups.get(source)!.kind !== "file") { internal++; continue; }
    const id = JSON.stringify([source,target]);
    const relation = relations.get(id) ?? { id, source, target, count: 0, kinds: [], evidence: [] };
    relation.count++; relation.evidence.push(edge);
    if (!relation.kinds.includes(edge.kind)) relation.kinds.push(edge.kind);
    relations.set(id, relation);
  }
  const all = [...groups.values()].sort((a,b)=>a.path.localeCompare(b.path)||a.id.localeCompare(b.id));
  const visible = all.slice(0,limit), ids = new Set(visible.map(node=>node.id));
  const allRelations = [...relations.values()].sort((a,b)=>a.id.localeCompare(b.id));
  const shown = allRelations.filter(edge=>ids.has(edge.source)&&ids.has(edge.target));
  return { nodes: visible, edges: shown, boundaries:[...boundaries.values()].sort((a,b)=>a.id.localeCompare(b.id)), total: all.length, hidden: all.length-visible.length, internal, boundary,
    hiddenRelations: allRelations.filter(edge=>!ids.has(edge.source)||!ids.has(edge.target)).reduce((sum,edge)=>sum+edge.count,0) };
}
