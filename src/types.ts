export type LanguageCount = { name: string; count: number };
export type PackageInfo = { name: string; version: string; dependencyType: string; importedBy: number };
export type ResolutionIssue = { source: string; specifier: string; line: number; status: string; reason: string; attemptedCandidates: string[]; hint: string };
export type ProjectSummary = {
  name: string; rootPath: string; fileCount: number; sourceFileCount: number; moduleCount: number;
  dependencyCount: number; externalPackageCount: number; cycleCount: number; parseErrorCount: number;
  unresolvedCount: number; ambiguousCount: number; sourceLines: number; languages: LanguageCount[]; packages: PackageInfo[]; cycles: string[][]; resolutionIssues: ResolutionIssue[];
};
export type GraphNode = {
  id: string; label: string; path: string; kind: "file" | "package"; language: string | null;
  lines: number; incoming: number; outgoing: number; declared: boolean | null;
};
export type GraphEdge = { id: string; source: string; target: string; kind: string; line: number };
export type SavedGraphState = {
  mode: "files" | "directories"; directory: string; query: string; showPackages: boolean; edgeKind: string;
  focus: string | null; direction: "both" | "incoming" | "outgoing"; selectedNodeId: string | null;
  camera: { centerX: number; centerY: number; zoom: number }; detailsOpen: boolean;
};
export type InvestigationMetadata = { id: string; projectId: string; snapshotId: string; name: string; notePreview: string; stateVersion: number; revision: number; createdAt: number; updatedAt: number };
export type Investigation = InvestigationMetadata & { note: string; graph: SavedGraphState };
export type InvestigationPage = { items: InvestigationMetadata[]; total: number; nextOffset: number | null };
export type RuleConfig = { enabled: boolean; exclude: string[]; threshold: number | null };
export type RulesRun = {
  version: string; configHash: string; effectiveConfig: { rules: Record<string, RuleConfig> };
  completedRules: string[]; skippedRules: { ruleId: string; reason: string }[];
  limitation: string; sourceFileCount: number; coverageIssueCount: number; durationMs: number;
};
export type DiagnosticEvidence =
  | { type: "cycle"; graph: string; component: string[]; route: GraphEdge[]; edgeKinds: string[] }
  | { type: "coupling"; observed: number; threshold: number; neighbors: string[]; imports: GraphEdge[] }
  | { type: "consumers"; checkedExclusions: string[]; limitation: string }
  | { type: "resolution"; issue: ResolutionIssue }
  | { type: "parse"; source: string; reason: string };
export type Diagnostic = {
  id: string; ruleId: string; severity: "info" | "warning" | "error"; confidence: "confirmed" | "heuristic";
  category: "architecture" | "coverage"; title: string; explanation: string; suggestion: string;
  nodeIds: string[]; evidence: DiagnosticEvidence;
};
export type AnalysisResult = {
  analysisProfile?: { graphSemantics: string; analyzers: Record<string,[string,string]>; pythonRoots: string[]; excludedPaths?: string[]; limitations: string[] } | null; basis?: SnapshotBasis | null; summary: ProjectSummary; nodes: GraphNode[]; edges: GraphEdge[]; diagnostics: Diagnostic[]; rulesRun: RulesRun | null };
export type ProjectRecord = { id: string; name: string; rootPath: string; lastOpenedAt: number; latestSnapshotId: string | null; latestSnapshotAt: number | null };
export type SnapshotRecord = { basis?: SnapshotBasis | null; id: string; projectId: string; createdAt: number; analyzerVersion: string; rulesVersion: string; parsedFileCount: number; reusedFileCount: number };
export type ProjectView = { project: ProjectRecord; snapshot: SnapshotRecord; result: AnalysisResult };
export type SnapshotBasis = { kind: string; repositoryIdentity: string | null; projectScope: string; requestedRef: string | null; commitOid: string | null; treeOid: string | null; observedHeadOid: string | null; branchLabel: string | null; indexFingerprint: string | null; captureFingerprint: string; startedAt: number; completedAt: number; captureConsistency: string; resolutionProfile: string; capturePolicyHash: string; omittedCount: number };
export type GitStatusRow = { path: string; staged: string[]; unstaged: string[]; untracked: boolean; conflicted: boolean };
export type GitStatus = { root: string; repositoryRoot: string; scope: string; headOid: string | null; branch: string | null; observedAt: number; items: GitStatusRow[]; offset: number; nextOffset: number | null; total: number };
export type GitPage<T> = { items: T[]; nextOffset: number | null };
export type GitRef = { name: string; oid: string };
export type GitCommit = { oid: string; summary: string; timestamp: number };
export type GitJob = { id: string; root: string; phase: string; fileCount: number; done: boolean; error: string | null; snapshotId: string | null };
export type ComparisonSummary = { id: string; baseSnapshotId: string; targetSnapshotId: string; compatible: boolean; inventoryAvailable: boolean; diagnosticsComparable: boolean; warnings: string[]; counts: Record<string, number>; total: number };
export type ComparisonChange = { category: string; status: string; label: string; detail: string; baseNodeId: string | null; targetNodeId: string | null; oldCount: number | null; newCount: number | null };
export type ComparisonPage = { items: ComparisonChange[]; offset: number; nextOffset: number | null; matchingCount: number };
export type InventoryRow = { fact: { path: string; kind: string; mode: number; contentHash: string | null; blobOid: string | null; status: string; reason: string | null; parserVersion: string | null; parseError: boolean | null }; importCount: number };
export type ImpactRequest = { snapshotId: string; roots: string[]; edgeKinds: string[]; maxDepth: number | null };
export type ImpactRow = { nodeId: string; path: string; distance: number; viaRootId: string; isTestCandidate: boolean; hasRecordedCycleDiagnostic: boolean };
export type ImpactPage = { items: ImpactRow[]; offset: number; nextOffset: number | null; knownMatchingCount: number; status: "complete" | "partial" };
export type ImpactPath = { edges: GraphEdge[]; offset: number; nextOffset: number | null; totalEdgeCount: number; reachesRoot: boolean };
export type ImpactSummary = {
  queryId: string; snapshotId: string; impactVersion: string; analyzerVersion: string; rulesVersion: string;
  roots: string[]; effectiveEdgeKinds: string[]; maxDepth: number | null; status: "complete" | "partial"; partialReasons: string[];
  knownAffectedCount: number; knownDirectCount: number; knownTransitiveCount: number; knownTestCandidateCount: number; maxKnownDistance: number;
  coverage: { state: "known-gaps" | "no-recorded-gaps" | "legacy-unknown"; reasons: string[] };
  visitedNodeCount: number; examinedEdgeCount: number; elapsedMs: number; readMs: number; indexMs: number; estimatedIndexBytes: number;
  budgets: { maxNodes: number; maxEdges: number; maxMillis: number }; firstPage: ImpactPage;
};
export type ImpactGraphScope = {
  queryId: string; title: string; roots: string[]; nodeIds: string[]; edges?: GraphEdge[]; edgeKinds: string[];
  knownAffectedCount: number; shownAffectedCount: number; partial: boolean;
};
