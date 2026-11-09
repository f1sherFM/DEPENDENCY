export type LanguageCount = { name: string; count: number };
export type PackageInfo = { name: string; version: string; dependencyType: string; importedBy: number };
export type ProjectSummary = {
  name: string; rootPath: string; fileCount: number; sourceFileCount: number; moduleCount: number;
  dependencyCount: number; externalPackageCount: number; cycleCount: number; parseErrorCount: number;
  unresolvedCount: number; sourceLines: number; languages: LanguageCount[]; packages: PackageInfo[]; cycles: string[][];
};
export type GraphNode = {
  id: string; label: string; path: string; kind: "file" | "package"; language: string | null;
  lines: number; incoming: number; outgoing: number; declared: boolean | null;
};
export type GraphEdge = { id: string; source: string; target: string; kind: string; line: number };
export type AnalysisResult = { summary: ProjectSummary; nodes: GraphNode[]; edges: GraphEdge[] };
export type ProjectRecord = { id: string; name: string; rootPath: string; lastOpenedAt: number; latestSnapshotId: string | null; latestSnapshotAt: number | null };
export type SnapshotRecord = { id: string; projectId: string; createdAt: number; analyzerVersion: string; rulesVersion: string; parsedFileCount: number; reusedFileCount: number };
export type ProjectView = { project: ProjectRecord; snapshot: SnapshotRecord; result: AnalysisResult };
