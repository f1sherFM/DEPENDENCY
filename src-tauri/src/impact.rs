use crate::{
    analysis::{AnalysisResult, GraphEdge},
    diagnostics::Evidence,
    storage::{AppState, SnapshotRecord},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

pub const VERSION: &str = "file-impact-1";
pub const EDGE_KINDS: [&str; 5] = [
    "import",
    "reexport",
    "commonjs_import",
    "type_only_import",
    "dynamic_import",
];
const MAX_INDEX_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImpactRequest {
    pub snapshot_id: String,
    pub roots: Vec<String>,
    pub edge_kinds: Vec<String>,
    pub max_depth: Option<usize>,
}
impl ImpactRequest {
    fn normalize(&mut self) -> Result<(), String> {
        if self.snapshot_id.is_empty() || self.snapshot_id.len() > 128 {
            return Err("Укажите существующий снимок".into());
        }
        if self.roots.is_empty()
            || self.roots.len() > 32
            || self.roots.iter().any(|id| id.len() > 4096)
        {
            return Err("Выберите от 1 до 32 локальных файлов".into());
        }
        self.roots.sort();
        if self.roots.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("Исходные файлы не должны повторяться".into());
        }
        if self.edge_kinds.is_empty()
            || self.edge_kinds.len() > EDGE_KINDS.len()
            || self
                .edge_kinds
                .iter()
                .any(|kind| !EDGE_KINDS.contains(&kind.as_str()))
        {
            return Err("Выберите известные типы импортов".into());
        }
        self.edge_kinds.sort();
        self.edge_kinds.dedup();
        if self
            .max_depth
            .is_some_and(|depth| !(1..=64).contains(&depth))
        {
            return Err("Глубина должна быть от 1 до 64 или без ограничения".into());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub state: String,
    pub reasons: Vec<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budgets {
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_millis: u64,
}
impl Default for Budgets {
    fn default() -> Self {
        Self {
            max_nodes: 100_000,
            max_edges: 1_000_000,
            max_millis: 2000,
        }
    }
}

struct Node {
    id: String,
    path: String,
    is_test: bool,
    recorded_cycle: bool,
}
struct Edge {
    source: usize,
    target: usize,
    original: GraphEdge,
}
struct Index {
    snapshot: SnapshotRecord,
    nodes: Vec<Node>,
    ids: HashMap<String, usize>,
    edges: Vec<Edge>,
    incoming: Vec<Vec<usize>>,
    coverage: Coverage,
    estimated_bytes: usize,
    read_ms: u128,
    build_ms: u128,
}
impl Index {
    fn build(
        snapshot: SnapshotRecord,
        result: AnalysisResult,
        read_ms: u128,
    ) -> Result<Self, String> {
        let started = Instant::now();
        let estimated_bytes = result
            .nodes
            .iter()
            .map(|node| 512 + node.id.len() * 3 + node.path.len())
            .sum::<usize>()
            + result
                .edges
                .iter()
                .map(|edge| {
                    256 + edge.id.len() + edge.source.len() + edge.target.len() + edge.kind.len()
                })
                .sum::<usize>();
        if estimated_bytes > MAX_INDEX_BYTES {
            return Err("Снимок слишком велик для impact-индекса (оценочный предел 256 МиБ). Граф не усечён".into());
        }
        let recorded_cycles: HashSet<&str> = result
            .diagnostics
            .iter()
            .filter_map(|d| {
                if let Evidence::Cycle { component, .. } = &d.evidence {
                    Some(component)
                } else {
                    None
                }
            })
            .flatten()
            .map(String::as_str)
            .collect();
        let nodes: Vec<Node> = result
            .nodes
            .iter()
            .filter(|node| node.kind == "file")
            .map(|node| Node {
                id: node.id.clone(),
                path: node.path.clone(),
                is_test: test_candidate(&node.path),
                recorded_cycle: recorded_cycles.contains(node.id.as_str()),
            })
            .collect();
        let ids: HashMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.id.clone(), i))
            .collect();
        if ids.len() != nodes.len() {
            return Err("Повреждённый снимок: повторяющиеся идентификаторы файлов".into());
        }
        let mut edges = Vec::new();
        let mut incoming = vec![Vec::new(); nodes.len()];
        for edge in result.edges {
            if let (Some(&source), Some(&target)) = (ids.get(&edge.source), ids.get(&edge.target)) {
                incoming[target].push(edges.len());
                edges.push(Edge {
                    source,
                    target,
                    original: edge,
                });
            }
        }
        for list in &mut incoming {
            list.sort_by(|&a, &b| {
                let a = &edges[a].original;
                let b = &edges[b].original;
                (&a.source, &a.target, &a.kind, a.line, &a.id)
                    .cmp(&(&b.source, &b.target, &b.kind, b.line, &b.id))
            });
        }
        let mut reasons = vec!["Достижимость в файловом графе не доказывает поломку конкретного изменения. Вычисляемые пути, ignore-правила и неподдерживаемые языки ограничивают покрытие.".into()];
        let state = if let Some(run) = &result.rules_run {
            if run.coverage_issue_count > 0 {
                reasons.push(format!(
                    "Снимок содержит {} проблем покрытия",
                    run.coverage_issue_count
                ));
                "known-gaps"
            } else {
                "no-recorded-gaps"
            }
        } else {
            reasons.push("Исторический снимок не содержит полных метаданных покрытия".into());
            if result.summary.parse_error_count
                + result.summary.unresolved_count
                + result.summary.ambiguous_count
                > 0
            {
                reasons.push(
                    "В историческом снимке сохранены ошибки разбора или разрешения импортов".into(),
                );
            }
            "legacy-unknown"
        };
        let mut state = if snapshot.analyzer_version != crate::storage::ANALYZER_VERSION {
            reasons.push(format!("Снимок создан анализатором {}; классификация type-only может отличаться. История не пересчитывается", snapshot.analyzer_version));
            "legacy-unknown"
        } else {
            state
        };
        if edges
            .iter()
            .any(|edge| !EDGE_KINDS.contains(&edge.original.kind.as_str()))
        {
            reasons.push("В снимке есть неизвестные локальные виды рёбер, они не обходятся".into());
            if state != "legacy-unknown" {
                state = "known-gaps";
            }
        }
        Ok(Self {
            snapshot,
            nodes,
            ids,
            edges,
            incoming,
            coverage: Coverage {
                state: state.into(),
                reasons,
            },
            estimated_bytes,
            read_ms,
            build_ms: started.elapsed().as_millis(),
        })
    }
}
fn test_candidate(path: &str) -> bool {
    let path = path.to_lowercase();
    path.contains(".test.")
        || path.contains(".spec.")
        || path
            .split('/')
            .any(|part| matches!(part, "test" | "tests" | "__tests__"))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactRow {
    pub node_id: String,
    pub path: String,
    pub distance: usize,
    pub via_root_id: String,
    pub is_test_candidate: bool,
    pub has_recorded_cycle_diagnostic: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactPage {
    pub items: Vec<ImpactRow>,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub known_matching_count: usize,
    pub status: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactPath {
    pub edges: Vec<GraphEdge>,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub total_edge_count: usize,
    pub reaches_root: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactSummary {
    pub query_id: String,
    pub snapshot_id: String,
    pub impact_version: String,
    pub analyzer_version: String,
    pub rules_version: String,
    pub roots: Vec<String>,
    pub effective_edge_kinds: Vec<String>,
    pub max_depth: Option<usize>,
    pub status: String,
    pub partial_reasons: Vec<String>,
    pub known_affected_count: usize,
    pub known_direct_count: usize,
    pub known_transitive_count: usize,
    pub known_test_candidate_count: usize,
    pub max_known_distance: usize,
    pub coverage: Coverage,
    pub visited_node_count: usize,
    pub examined_edge_count: usize,
    pub elapsed_ms: u128,
    pub read_ms: u128,
    pub index_ms: u128,
    pub estimated_index_bytes: usize,
    pub budgets: Budgets,
    pub first_page: ImpactPage,
}
#[derive(Clone, Copy)]
struct Visit {
    distance: usize,
    root: usize,
    parent_edge: Option<usize>,
}
struct Query {
    id: String,
    request: ImpactRequest,
    index: Arc<Index>,
    visits: Vec<Option<Visit>>,
    ordered: Vec<usize>,
    partial_reasons: Vec<String>,
    examined_edges: usize,
    elapsed_ms: u128,
    budgets: Budgets,
}
impl Query {
    fn run(
        index: Arc<Index>,
        mut request: ImpactRequest,
        budgets: Budgets,
    ) -> Result<Self, String> {
        request.normalize()?;
        if index.snapshot.id != request.snapshot_id {
            return Err("Снимок запроса не совпадает с индексом".into());
        }
        let roots =
            request
                .roots
                .iter()
                .map(|id| {
                    index.ids.get(id).copied().ok_or_else(|| {
                        format!("`{id}` не является локальным файлом выбранного снимка")
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
        let started = Instant::now();
        let mut visits = vec![None; index.nodes.len()];
        let mut queue = roots.clone();
        for &root in &roots {
            visits[root] = Some(Visit {
                distance: 0,
                root,
                parent_edge: None,
            });
        }
        let allowed: HashSet<&str> = request.edge_kinds.iter().map(String::as_str).collect();
        let mut partial = BTreeSet::new();
        let mut examined_edges = 0;
        'walk: for head in 0..index.nodes.len() {
            if head >= queue.len() {
                break;
            }
            let target = queue[head];
            let current = visits[target].expect("queued node is visited");
            for &edge_id in &index.incoming[target] {
                // Count all inspected local edges, including those rejected by the mode.
                if examined_edges >= budgets.max_edges {
                    partial.insert("edge-budget".to_string());
                    break 'walk;
                }
                if started.elapsed() >= Duration::from_millis(budgets.max_millis) {
                    partial.insert("time-budget".to_string());
                    break 'walk;
                }
                examined_edges += 1;
                let edge = &index.edges[edge_id];
                if !allowed.contains(edge.original.kind.as_str()) || visits[edge.source].is_some() {
                    continue;
                }
                if request
                    .max_depth
                    .is_some_and(|depth| current.distance >= depth)
                {
                    partial.insert("depth-limit".into());
                    continue;
                }
                if queue.len() >= budgets.max_nodes {
                    partial.insert("node-budget".into());
                    break 'walk;
                }
                visits[edge.source] = Some(Visit {
                    distance: current.distance + 1,
                    root: current.root,
                    parent_edge: Some(edge_id),
                });
                queue.push(edge.source);
            }
        }
        let mut ordered: Vec<usize> = queue
            .into_iter()
            .filter(|&id| visits[id].is_some_and(|visit| visit.distance > 0))
            .collect();
        ordered.sort_by(|&a, &b| {
            (
                visits[a].unwrap().distance,
                &index.nodes[a].path,
                &index.nodes[a].id,
            )
                .cmp(&(
                    visits[b].unwrap().distance,
                    &index.nodes[b].path,
                    &index.nodes[b].id,
                ))
        });
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            request,
            index,
            visits,
            ordered,
            partial_reasons: partial.into_iter().collect(),
            examined_edges,
            elapsed_ms: started.elapsed().as_millis(),
            budgets,
        })
    }
    fn status(&self) -> String {
        if self.partial_reasons.is_empty() {
            "complete"
        } else {
            "partial"
        }
        .into()
    }
    fn page(
        &self,
        offset: usize,
        limit: usize,
        text: &str,
        tests_only: bool,
    ) -> Result<ImpactPage, String> {
        validate_limit(limit)?;
        if text.len() > 4096 {
            return Err("Поисковый запрос слишком длинный".into());
        }
        let text = text.trim().to_lowercase();
        let matches: Vec<usize> = self
            .ordered
            .iter()
            .copied()
            .filter(|&id| {
                let node = &self.index.nodes[id];
                (!tests_only || node.is_test) && node.path.to_lowercase().contains(&text)
            })
            .collect();
        if offset > matches.len() {
            return Err("Страница вне диапазона результатов".into());
        }
        let items = matches
            .iter()
            .skip(offset)
            .take(limit)
            .map(|&id| {
                let node = &self.index.nodes[id];
                let visit = self.visits[id].unwrap();
                ImpactRow {
                    node_id: node.id.clone(),
                    path: node.path.clone(),
                    distance: visit.distance,
                    via_root_id: self.index.nodes[visit.root].id.clone(),
                    is_test_candidate: node.is_test,
                    has_recorded_cycle_diagnostic: node.recorded_cycle,
                }
            })
            .collect::<Vec<_>>();
        let next = offset + items.len();
        Ok(ImpactPage {
            items,
            offset,
            next_offset: (next < matches.len()).then_some(next),
            known_matching_count: matches.len(),
            status: self.status(),
        })
    }
    fn path(&self, node_id: &str, offset: usize, limit: usize) -> Result<ImpactPath, String> {
        validate_limit(limit)?;
        let mut id = *self
            .index
            .ids
            .get(node_id)
            .ok_or("Файл не найден в снимке")?;
        let visit = self.visits[id].ok_or("Узел не входит в результат запроса")?;
        let total = visit.distance;
        if offset > total {
            return Err("Сегмент пути вне диапазона".into());
        }
        for _ in 0..offset {
            id = self.index.edges[self.visits[id]
                .unwrap()
                .parent_edge
                .ok_or("Нарушена цепочка доказательства")?]
            .target;
        }
        let mut edges = Vec::new();
        for _ in offset..total.min(offset + limit) {
            let edge = &self.index.edges[self.visits[id]
                .unwrap()
                .parent_edge
                .ok_or("Нарушена цепочка доказательства")?];
            edges.push(edge.original.clone());
            id = edge.target;
        }
        let next = offset + edges.len();
        Ok(ImpactPath {
            edges,
            offset,
            next_offset: (next < total).then_some(next),
            total_edge_count: total,
            reaches_root: next == total,
        })
    }
    fn summary(&self) -> ImpactSummary {
        let direct = self
            .ordered
            .iter()
            .filter(|&&id| self.visits[id].unwrap().distance == 1)
            .count();
        ImpactSummary {
            query_id: self.id.clone(),
            snapshot_id: self.request.snapshot_id.clone(),
            impact_version: VERSION.into(),
            analyzer_version: self.index.snapshot.analyzer_version.clone(),
            rules_version: self.index.snapshot.rules_version.clone(),
            roots: self.request.roots.clone(),
            effective_edge_kinds: self.request.edge_kinds.clone(),
            max_depth: self.request.max_depth,
            status: self.status(),
            partial_reasons: self.partial_reasons.clone(),
            known_affected_count: self.ordered.len(),
            known_direct_count: direct,
            known_transitive_count: self.ordered.len() - direct,
            known_test_candidate_count: self
                .ordered
                .iter()
                .filter(|&&id| self.index.nodes[id].is_test)
                .count(),
            max_known_distance: self
                .ordered
                .iter()
                .map(|&id| self.visits[id].unwrap().distance)
                .max()
                .unwrap_or(0),
            coverage: self.index.coverage.clone(),
            visited_node_count: self.ordered.len() + self.request.roots.len(),
            examined_edge_count: self.examined_edges,
            elapsed_ms: self.elapsed_ms,
            read_ms: self.index.read_ms,
            index_ms: self.index.build_ms,
            estimated_index_bytes: self.index.estimated_bytes,
            budgets: self.budgets.clone(),
            first_page: self.page(0, 200, "", false).expect("initial page is valid"),
        }
    }
}
fn validate_limit(limit: usize) -> Result<(), String> {
    if !(1..=200).contains(&limit) {
        Err("Размер страницы/сегмента должен быть от 1 до 200".into())
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct Cache {
    index: Option<Arc<Index>>,
    query: Option<Arc<Query>>,
}
#[derive(Clone, Default)]
pub struct ImpactState {
    cache: Arc<Mutex<Cache>>,
    query_gate: Arc<Mutex<()>>,
}
impl ImpactState {
    pub fn get(
        &self,
        storage: &AppState,
        mut request: ImpactRequest,
    ) -> Result<ImpactSummary, String> {
        request.normalize()?;
        // Serialize computations, but page/path readers only briefly lock the immutable cache.
        let _gate = self
            .query_gate
            .lock()
            .map_err(|_| "Impact worker недоступен")?;
        let index = {
            let mut cache = self.cache.lock().map_err(|_| "Impact cache недоступен")?;
            if let Some(query) = &cache.query
                && query.request == request
            {
                return Ok(query.summary());
            }
            if cache
                .index
                .as_ref()
                .is_some_and(|index| index.snapshot.id != request.snapshot_id)
            {
                *cache = Cache::default();
            }
            cache.index.clone()
        };
        let index = if let Some(index) = index {
            index
        } else {
            let started = Instant::now();
            let (snapshot, result) =
                crate::storage::read_impact_snapshot(storage, &request.snapshot_id)?;
            Arc::new(Index::build(
                snapshot,
                result,
                started.elapsed().as_millis(),
            )?)
        };
        let query = Arc::new(Query::run(index.clone(), request, Budgets::default())?);
        let summary = query.summary();
        *self.cache.lock().map_err(|_| "Impact cache недоступен")? = Cache {
            index: Some(index),
            query: Some(query),
        };
        Ok(summary)
    }
    fn query(&self, id: &str) -> Result<Arc<Query>, String> {
        self.cache
            .lock()
            .map_err(|_| "Impact cache недоступен")?
            .query
            .as_ref()
            .filter(|query| query.id == id)
            .cloned()
            .ok_or("Результат Impact Analysis истёк. Повторите запрос".into())
    }
    pub fn page(
        &self,
        id: &str,
        offset: usize,
        limit: usize,
        text: &str,
        tests_only: bool,
    ) -> Result<ImpactPage, String> {
        self.query(id)?.page(offset, limit, text, tests_only)
    }
    pub fn path(
        &self,
        id: &str,
        node: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ImpactPath, String> {
        self.query(id)?.path(node, offset, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{GraphNode, ProjectSummary};
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn edge(source: &str, target: &str, kind: &str) -> GraphEdge {
        GraphEdge {
            id: format!("{source}:{target}:{kind}"),
            source: source.into(),
            target: target.into(),
            kind: kind.into(),
            line: 7,
        }
    }
    fn result(names: &[&str], edges: Vec<GraphEdge>) -> AnalysisResult {
        AnalysisResult {
            summary: ProjectSummary {
                name: "test".into(),
                root_path: "missing".into(),
                file_count: names.len(),
                source_file_count: names.len(),
                module_count: 1,
                dependency_count: edges.len(),
                external_package_count: 0,
                cycle_count: 0,
                parse_error_count: 0,
                unresolved_count: 0,
                ambiguous_count: 0,
                source_lines: 0,
                languages: vec![],
                packages: vec![],
                cycles: vec![],
                resolution_issues: vec![],
            },
            nodes: names
                .iter()
                .map(|id| GraphNode {
                    id: (*id).into(),
                    path: (*id).into(),
                    label: (*id).into(),
                    kind: "file".into(),
                    language: Some("TypeScript".into()),
                    lines: 0,
                    incoming: 0,
                    outgoing: 0,
                    declared: None,
                })
                .collect(),
            edges,
            diagnostics: vec![],
            rules_run: None,
        }
    }
    fn snapshot() -> SnapshotRecord {
        SnapshotRecord {
            id: "snapshot".into(),
            project_id: "project".into(),
            created_at: 0,
            analyzer_version: crate::storage::ANALYZER_VERSION.into(),
            rules_version: crate::storage::RULES_VERSION.into(),
            parsed_file_count: 0,
            reused_file_count: 0,
        }
    }
    fn request(roots: &[&str]) -> ImpactRequest {
        ImpactRequest {
            snapshot_id: "snapshot".into(),
            roots: roots.iter().map(|id| (*id).into()).collect(),
            edge_kinds: EDGE_KINDS.iter().map(|kind| (*kind).into()).collect(),
            max_depth: None,
        }
    }
    fn query(
        names: &[&str],
        edges: Vec<GraphEdge>,
        request: ImpactRequest,
        budgets: Budgets,
    ) -> Query {
        Query::run(
            Arc::new(Index::build(snapshot(), result(names, edges), 0).unwrap()),
            request,
            budgets,
        )
        .unwrap()
    }
    #[test]
    fn reverse_chain_proves_direction_and_does_not_return_dependencies() {
        let names = ["a", "b", "c"];
        let edges = vec![edge("a", "b", "import"), edge("b", "c", "reexport")];
        let q = query(&names, edges.clone(), request(&["c"]), Budgets::default());
        let summary = q.summary();
        assert_eq!(summary.known_direct_count, 1);
        assert_eq!(summary.known_transitive_count, 1);
        assert_eq!(summary.status, "complete");
        let path = q.path("a", 0, 200).unwrap();
        assert_eq!(path.edges[0].source, "a");
        assert_eq!(path.edges[0].target, "b");
        assert_eq!(path.edges[1].target, "c");
        assert!(path.reaches_root);
        assert_eq!(
            query(&names, edges, request(&["a"]), Budgets::default())
                .summary()
                .known_affected_count,
            0
        );
    }
    #[test]
    fn diamond_is_deterministic_and_multi_roots_are_not_counted_as_consumers() {
        let edges = vec![
            edge("a", "b", "import"),
            edge("a", "c", "import"),
            edge("b", "d", "import"),
            edge("c", "d", "import"),
            edge("d", "d", "import"),
        ];
        let q = query(
            &["a", "b", "c", "d"],
            edges.clone(),
            request(&["d"]),
            Budgets::default(),
        );
        assert_eq!(q.path("a", 0, 200).unwrap().edges[0].target, "b");
        let multi = query(
            &["a", "b", "c", "d"],
            edges,
            request(&["b", "d"]),
            Budgets::default(),
        );
        assert_eq!(multi.summary().known_affected_count, 2);
        assert_eq!(multi.summary().known_direct_count, 2);
        assert_eq!(multi.path("a", 0, 200).unwrap().edges[0].target, "b");
    }
    #[test]
    fn cycles_terminate_and_seed_never_counts_itself() {
        let q = query(
            &["a", "b", "c", "isolated"],
            vec![
                edge("a", "b", "import"),
                edge("b", "c", "import"),
                edge("c", "a", "import"),
                edge("a", "a", "import"),
            ],
            request(&["a"]),
            Budgets::default(),
        );
        assert_eq!(q.summary().known_affected_count, 2);
        assert_eq!(q.path("c", 0, 1).unwrap().total_edge_count, 1);
        assert!(q.path("isolated", 0, 200).is_err());
        assert_eq!(q.path("a", 0, 200).unwrap().total_edge_count, 0);
    }
    #[test]
    fn modes_filter_traversal_but_search_and_test_filters_keep_intermediate_nodes() {
        let names = ["root", "middle", "unit.spec.ts", "dynamic"];
        let edges = vec![
            edge("middle", "root", "type_only_import"),
            edge("unit.spec.ts", "middle", "import"),
            edge("dynamic", "root", "dynamic_import"),
        ];
        let q = query(
            &names,
            edges.clone(),
            request(&["root"]),
            Budgets::default(),
        );
        let page = q.page(0, 1, "spec", true).unwrap();
        assert_eq!(page.known_matching_count, 1);
        assert_eq!(page.items[0].distance, 2);
        assert!(page.items[0].is_test_candidate);
        assert_eq!(q.path("unit.spec.ts", 0, 200).unwrap().edges.len(), 2);
        let mut static_request = request(&["root"]);
        static_request.edge_kinds =
            vec!["import".into(), "reexport".into(), "commonjs_import".into()];
        assert_eq!(
            query(&names, edges, static_request, Budgets::default())
                .summary()
                .known_affected_count,
            0
        );
    }
    #[test]
    fn depth_and_work_budgets_are_partial_only_when_work_remains() {
        let chain = vec![edge("a", "b", "import"), edge("b", "c", "import")];
        let mut req = request(&["c"]);
        req.max_depth = Some(1);
        let q = query(
            &["a", "b", "c"],
            chain.clone(),
            req.clone(),
            Budgets::default(),
        );
        assert_eq!(q.partial_reasons, ["depth-limit"]);
        let exact = query(
            &["b", "c"],
            vec![edge("b", "c", "import")],
            req,
            Budgets {
                max_nodes: 2,
                max_edges: 1,
                max_millis: 2000,
            },
        );
        assert_eq!(exact.status(), "complete");
        let limited = query(
            &["a", "b", "c"],
            chain.clone(),
            request(&["c"]),
            Budgets {
                max_nodes: 2,
                ..Budgets::default()
            },
        );
        assert_eq!(limited.partial_reasons, ["node-budget"]);
        let limited = query(
            &["a", "b", "c"],
            chain.clone(),
            request(&["c"]),
            Budgets {
                max_edges: 1,
                ..Budgets::default()
            },
        );
        assert_eq!(limited.partial_reasons, ["edge-budget"]);
        let limited = query(
            &["a", "b", "c"],
            chain,
            request(&["c"]),
            Budgets {
                max_millis: 0,
                ..Budgets::default()
            },
        );
        assert_eq!(limited.partial_reasons, ["time-budget"]);
    }
    #[test]
    fn long_chain_has_paged_proofs_and_wide_tree_has_stable_nonduplicate_pages() {
        let names: Vec<String> = (0..50_001).map(|id| format!("file{id:05}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let edges = names
            .windows(2)
            .map(|pair| edge(&pair[0], &pair[1], "import"))
            .collect();
        let q = query(
            &refs,
            edges,
            request(&[names.last().unwrap()]),
            Budgets::default(),
        );
        assert_eq!(q.status(), "complete");
        assert_eq!(q.summary().max_known_distance, 50_000);
        let first = q.path(&names[0], 0, 200).unwrap();
        let next = q.path(&names[0], 200, 200).unwrap();
        assert_eq!(first.edges.last().unwrap().target, next.edges[0].source);
        assert!(!first.reaches_root);
        let last = q.path(&names[0], 49_999, 200).unwrap();
        assert!(last.reaches_root);
        assert_eq!(last.edges.len(), 1);
        let edges = names[1..]
            .iter()
            .map(|name| edge(name, &names[0], "import"))
            .collect();
        let q = query(&refs, edges, request(&[&names[0]]), Budgets::default());
        assert_eq!(q.summary().known_direct_count, 50_000);
        let a = q.page(0, 200, "", false).unwrap();
        let b = q.page(200, 200, "", false).unwrap();
        let ids: HashSet<_> = a
            .items
            .iter()
            .chain(&b.items)
            .map(|item| &item.node_id)
            .collect();
        assert_eq!(ids.len(), 400);
    }
    #[test]
    fn dense_graph_budget_does_not_claim_completion() {
        let names: Vec<String> = (0..150).map(|id| format!("f{id}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let edges = names
            .iter()
            .flat_map(|a| names.iter().map(move |b| edge(a, b, "import")))
            .collect();
        let q = query(
            &refs,
            edges,
            request(&[&names[0]]),
            Budgets {
                max_edges: 500,
                ..Budgets::default()
            },
        );
        assert_eq!(q.summary().known_affected_count, 149);
        assert_eq!(q.status(), "partial");
    }
    #[test]
    fn rejects_invalid_requests_and_paging() {
        for roots in [
            vec![],
            vec!["a", "a"],
            vec!["missing"],
            vec!["package:react"],
        ] {
            let index = Arc::new(Index::build(snapshot(), result(&["a"], vec![]), 0).unwrap());
            assert!(Query::run(index, request(&roots), Budgets::default()).is_err());
        }
        let mut req = request(&["a"]);
        req.edge_kinds = vec!["unknown".into()];
        assert!(req.normalize().is_err());
        req = request(&["a"]);
        req.max_depth = Some(0);
        assert!(req.normalize().is_err());
        req = request(&["a"]);
        req.snapshot_id = "another".into();
        assert!(
            Query::run(
                Arc::new(Index::build(snapshot(), result(&["a"], vec![]), 0).unwrap()),
                req,
                Budgets::default()
            )
            .is_err()
        );
        let q = query(&["a"], vec![], request(&["a"]), Budgets::default());
        assert!(q.page(0, 201, "", false).is_err());
        assert!(q.page(1, 200, "", false).is_err());
        assert!(q.path("a", 1, 200).is_err());
    }
    #[test]
    fn historic_snapshot_queries_survive_missing_sources_and_do_not_modify_storage() {
        let root = tempdir().unwrap();
        let db = tempdir().unwrap();
        std::fs::write(root.path().join("a.ts"), "import './b';").unwrap();
        std::fs::write(root.path().join("b.ts"), "export const b = 1;").unwrap();
        let state = crate::storage::initialize(db.path().join("db.sqlite3")).unwrap();
        let first =
            crate::analysis::analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
        let first = crate::storage::save_snapshot(
            &state,
            first.result,
            first.file_cache,
            first.parsed_file_count,
            first.reused_file_count,
        )
        .unwrap();
        std::fs::write(root.path().join("a.ts"), "export const a = 1;").unwrap();
        let second =
            crate::analysis::analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
        let second = crate::storage::save_snapshot(
            &state,
            second.result,
            second.file_cache,
            second.parsed_file_count,
            second.reused_file_count,
        )
        .unwrap();
        drop(root);
        let connection = rusqlite::Connection::open(&state.database_path).unwrap();
        let dump = || {
            let mut statement = connection
                .prepare("SELECT result_json FROM snapshots ORDER BY id")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        let before = dump();
        let impact = ImpactState::default();
        let mut req = request(&["b.ts"]);
        req.snapshot_id = first.snapshot.id;
        let summary = impact.get(&state, req.clone()).unwrap();
        assert_eq!(summary.known_affected_count, 1);
        assert_eq!(impact.get(&state, req).unwrap().query_id, summary.query_id);
        assert_eq!(
            impact
                .path(&summary.query_id, "a.ts", 0, 200)
                .unwrap()
                .edges[0]
                .target,
            "b.ts"
        );
        let mut req = request(&["b.ts"]);
        req.snapshot_id = second.snapshot.id.clone();
        assert_eq!(impact.get(&state, req).unwrap().known_affected_count, 0);
        assert!(
            impact
                .page(&summary.query_id, 0, 200, "", false)
                .err()
                .unwrap()
                .contains("истёк")
        );
        assert_eq!(before, dump());
        assert_eq!(
            crate::storage::load_project(&state, &first.project.id)
                .unwrap()
                .snapshot
                .id,
            second.snapshot.id
        );
    }
    #[test]
    fn legacy_and_saved_gaps_are_reported_without_reinterpreting_history() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diagnostics");
        let mut output = crate::analysis::analyze_project(root.to_str().unwrap(), vec![]).unwrap();
        output
            .result
            .rules_run
            .as_mut()
            .unwrap()
            .coverage_issue_count = 2;
        let index = Index::build(snapshot(), output.result, 0).unwrap();
        assert_eq!(index.coverage.state, "known-gaps");
        let mut old = snapshot();
        old.analyzer_version = "js-ts-parser-1".into();
        let index = Index::build(old, result(&["a"], vec![]), 0).unwrap();
        assert_eq!(index.coverage.state, "legacy-unknown");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/impact");
        let mut output = crate::analysis::analyze_project(root.to_str().unwrap(), vec![]).unwrap();
        output
            .result
            .edges
            .push(edge("src/app.ts", "src/target.ts", "future-kind"));
        let index = Index::build(snapshot(), output.result, 0).unwrap();
        assert_eq!(index.coverage.state, "known-gaps");
    }

    #[test]
    #[ignore = "explicit real engine / storage profiling and UI fixture export"]
    fn profile_and_export_impact() {
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/impact");
        let output = crate::analysis::analyze_project(fixture.to_str().unwrap(), vec![]).unwrap();
        let db = tempdir().unwrap();
        let storage = crate::storage::initialize(db.path().join("db.sqlite3")).unwrap();
        let view = crate::storage::save_snapshot(
            &storage,
            output.result,
            output.file_cache,
            output.parsed_file_count,
            output.reused_file_count,
        )
        .unwrap();
        let state = ImpactState::default();
        let mut cases = Vec::new();
        for (roots, static_only, depth) in [
            (vec!["src/target.ts"], false, None),
            (vec!["src/target.ts"], true, None),
            (vec!["src/target.ts"], false, Some(1)),
            (vec!["src/unit.spec.ts"], false, None),
            (vec!["src/target.ts", "src/app.ts"], false, None),
            (vec!["src/style.css"], false, None),
        ] {
            let mut req = request(&roots);
            req.snapshot_id = view.snapshot.id.clone();
            req.max_depth = depth;
            if static_only {
                req.edge_kinds = EDGE_KINDS[..3].iter().map(|kind| (*kind).into()).collect();
            }
            let summary = state.get(&storage, req.clone()).unwrap();
            let page = state.page(&summary.query_id, 0, 200, "", false).unwrap();
            let paths = page
                .items
                .iter()
                .map(|row| {
                    (
                        row.node_id.clone(),
                        serde_json::to_value(
                            state.path(&summary.query_id, &row.node_id, 0, 200).unwrap(),
                        )
                        .unwrap(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            println!(
                "IMPACT {:?} static={static_only} depth={depth:?}: affected={} status={} read={}ms index={}ms walk={}ms memory={}B",
                req.roots,
                summary.known_affected_count,
                summary.status,
                summary.read_ms,
                summary.index_ms,
                summary.elapsed_ms,
                summary.estimated_index_bytes
            );
            if roots == ["src/target.ts"] && !static_only && depth.is_none() {
                assert_eq!(summary.known_affected_count, 7);
            }
            cases.push(serde_json::json!({ "request": req, "summary": summary, "page": page, "paths": paths }));
        }
        if let Ok(directory) = std::env::var("DEPENDENCY_DUMP_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(std::path::Path::new(&directory).join("impact-ui.json"), serde_json::to_vec(&serde_json::json!({"analysis": view.result, "snapshot": view.snapshot, "cases": cases})).unwrap()).unwrap();
        }
        if let Ok(roots) = std::env::var("DEPENDENCY_PROFILE_ROOTS") {
            for root in roots.split(';').filter(|root| !root.is_empty()) {
                let output = crate::analysis::analyze_project(root, vec![]).unwrap();
                let file =
                    output
                        .result
                        .edges
                        .iter()
                        .find(|edge| edge.target == "src/types.ts")
                        .or_else(|| {
                            output.result.edges.iter().find(|edge| {
                                edge.kind != "package" && edge.target.starts_with("src/")
                            })
                        })
                        .or_else(|| {
                            output
                                .result
                                .edges
                                .iter()
                                .find(|edge| edge.kind != "package")
                        })
                        .unwrap()
                        .target
                        .clone();
                let view = crate::storage::save_snapshot(
                    &storage,
                    output.result,
                    output.file_cache,
                    output.parsed_file_count,
                    output.reused_file_count,
                )
                .unwrap();
                let mut req = request(&[&file]);
                req.snapshot_id = view.snapshot.id;
                let summary = state.get(&storage, req).unwrap();
                println!(
                    "REAL {} seed={} consumers={} read={}ms index={}ms walk={}ms memory={}B",
                    view.result.summary.name,
                    file,
                    summary.known_affected_count,
                    summary.read_ms,
                    summary.index_ms,
                    summary.elapsed_ms,
                    summary.estimated_index_bytes
                );
            }
        }
    }
}
