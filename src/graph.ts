import type { GraphEdge, GraphNode } from "./types";

export type DisplayNode = Omit<GraphNode, "kind"> & { kind: "file" | "package" | "directory"; members: string[] };
export type DisplayEdge = GraphEdge & { count: number };
export type GraphOptions = {
  mode: "files" | "directories"; directory: string; query: string; showPackages: boolean;
  edgeKind: string; focus: string | null; direction: "both" | "incoming" | "outgoing";
  limit: number;
};

// A view of a snapshot: aggregation and filtering never mutate analysis data.
export function projectGraph(nodes: GraphNode[], edges: GraphEdge[], options: GraphOptions) {
  const query = options.query.trim().toLowerCase();
  const eligible = nodes.filter(node => (options.showPackages || node.kind !== "package")
    && (!query || `${node.label} ${node.path}`.toLowerCase().includes(query)));
  const eligibleIds = new Set(eligible.map(node => node.id));
  const filteredEdges = edges.filter(edge => eligibleIds.has(edge.source) && eligibleIds.has(edge.target)
    && (!options.edgeKind || edge.kind === options.edgeKind));
  let focusIds: Set<string> | null = null;
  if (options.focus) {
    focusIds = new Set([options.focus]);
    for (const edge of filteredEdges) {
      if (edge.source === options.focus && options.direction !== "incoming") focusIds.add(edge.target);
      if (edge.target === options.focus && options.direction !== "outgoing") focusIds.add(edge.source);
    }
  }
  const displayed = new Map<string, DisplayNode>();
  const membership = new Map<string, string>();
  const prefix = options.directory ? `${options.directory}/` : "";
  for (const node of eligible) {
    if (focusIds && !focusIds.has(node.id)) continue;
    if (node.kind !== "package" && !node.path.startsWith(prefix)) continue;
    const remainder = node.path.slice(prefix.length);
    const folder = options.mode === "directories" && node.kind !== "package" && remainder.includes("/")
      ? `${prefix}${remainder.split("/")[0]}` : null;
    const id = folder ? `directory:${folder}` : node.id;
    membership.set(node.id, id);
    const existing = displayed.get(id);
    if (existing) { existing.members.push(node.id); existing.lines += node.lines; }
    else displayed.set(id, folder ? {
      id, path: folder, label: remainder.split("/")[0], kind: "directory", members: [node.id],
      language: null, lines: node.lines, incoming: 0, outgoing: 0, declared: null,
    } : { ...node, members: [node.id], incoming: 0, outgoing: 0 });
  }
  const aggregated = new Map<string, DisplayEdge>();
  let internalEdges = 0;
  let boundaryEdges = 0;
  for (const edge of filteredEdges) {
    if (options.focus && (edge.source !== options.focus && edge.target !== options.focus)) continue;
    if (options.focus && options.direction === "incoming" && edge.target !== options.focus) continue;
    if (options.focus && options.direction === "outgoing" && edge.source !== options.focus) continue;
    const source = membership.get(edge.source); const target = membership.get(edge.target);
    if (!source || !target) { if (source || target) boundaryEdges++; continue; }
    if (source === target && source.startsWith("directory:")) { internalEdges++; continue; }
    const key = JSON.stringify([source, target, edge.kind]);
    const existing = aggregated.get(key);
    if (existing) existing.count++;
    else aggregated.set(key, { ...edge, id: key, source, target, count: 1 });
    displayed.get(source)!.outgoing++;
    displayed.get(target)!.incoming++;
  }
  const allNodes = [...displayed.values()].sort((a, b) =>
    Number(b.members.includes(options.focus ?? "")) - Number(a.members.includes(options.focus ?? ""))
    || (b.incoming + b.outgoing) - (a.incoming + a.outgoing) || a.path.localeCompare(b.path));
  const visibleNodes = allNodes.slice(0, options.limit);
  const visibleIds = new Set(visibleNodes.map(node => node.id));
  const allEdges = [...aggregated.values()];
  const candidateEdges = allEdges.filter(edge => visibleIds.has(edge.source) && visibleIds.has(edge.target));
  const visibleEdges = candidateEdges.slice(0, 1000);
  return {
    nodes: visibleNodes, edges: visibleEdges, totalNodes: allNodes.length,
    hiddenNodes: allNodes.length - visibleNodes.length,
    hiddenEdges: allEdges.reduce((sum, edge) => sum + edge.count, 0) - visibleEdges.reduce((sum, edge) => sum + edge.count, 0),
    internalEdges, boundaryEdges,
  };
}

// Breadth-first columns keep connected nodes together; arrows carry direction.
export function layoutGraph(nodes: DisplayNode[], edges: DisplayEdge[], selected: string | null) {
  const neighbors = new Map(nodes.map(node => [node.id, new Set<string>()]));
  for (const edge of edges) { neighbors.get(edge.source)?.add(edge.target); neighbors.get(edge.target)?.add(edge.source); }
  const positions = new Map<string, { x: number; y: number }>();
  const rows = new Map<number, number>();
  const order = selected && neighbors.has(selected) ? [selected, ...nodes.map(node => node.id)] : nodes.map(node => node.id);
  const visited = new Set<string>();
  const isolated: string[] = [];
  let componentRow = 0;
  for (const root of order) {
    if (visited.has(root)) continue;
    if (!neighbors.get(root)?.size) { isolated.push(root); visited.add(root); continue; }
    const queue: [string, number][] = [[root, 0]]; visited.add(root);
    for (let index = 0; index < queue.length; index++) {
      const [id, depth] = queue[index]; const column = Math.min(depth, 6);
      const row = Math.max(componentRow, rows.get(column) ?? 0);
      positions.set(id, { x: 40 + column * 260, y: 40 + row * 100 }); rows.set(column, row + 1);
      for (const neighbor of neighbors.get(id) ?? []) if (!visited.has(neighbor)) {
        visited.add(neighbor); queue.push([neighbor, depth + 1]);
      }
    }
    componentRow = Math.max(...rows.values());
  }
  isolated.forEach((id, index) => positions.set(id, { x: 40 + (index % 4) * 260, y: 40 + (componentRow + Math.floor(index / 4)) * 100 }));
  return { positions, width: Math.max(600, ...[...positions.values()].map(p => p.x + 240)), height: Math.max(400, ...[...positions.values()].map(p => p.y + 100)) };
}
