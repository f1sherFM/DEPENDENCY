use crate::analysis::{GraphEdge, GraphNode, ParsedFileCache, ResolutionIssue};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
    time::Instant,
};

const RULES: [&str; 6] = [
    "dependency-cycle",
    "high-fan-out",
    "high-fan-in",
    "no-known-consumers",
    "resolution",
    "parse-error",
];
const COVERAGE_LIMITATION: &str = "Анализ ограничен индексированными JS/TS и статически извлечёнными импортами. Ignore-правила, framework entry points и вычисляемые пути могут скрывать потребителей. Отсутствие сигнала не доказывает отсутствие проблем.";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub id: String,
    pub rule_id: String,
    pub severity: String,
    pub confidence: String,
    pub category: String,
    pub title: String,
    pub explanation: String,
    pub suggestion: String,
    pub node_ids: Vec<String>,
    pub evidence: Evidence,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Evidence {
    Cycle {
        graph: String,
        component: Vec<String>,
        route: Vec<GraphEdge>,
        edge_kinds: Vec<String>,
    },
    Coupling {
        observed: usize,
        threshold: usize,
        neighbors: Vec<String>,
        imports: Vec<GraphEdge>,
    },
    Consumers {
        checked_exclusions: Vec<String>,
        limitation: String,
    },
    Resolution {
        issue: ResolutionIssue,
    },
    Parse {
        source: String,
        reason: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleConfig {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub threshold: Option<usize>,
}
fn yes() -> bool {
    true
}
impl Default for RuleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            exclude: Vec::new(),
            threshold: None,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RulesConfig {
    #[serde(default)]
    pub rules: BTreeMap<String, RuleConfig>,
}

pub struct Settings {
    pub config: RulesConfig,
    exclusions: HashMap<String, GlobSet>,
}
impl Settings {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("dependency.config.json");
        let mut config: RulesConfig = match fs::File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(65537)
                    .read_to_end(&mut bytes)
                    .map_err(|error| format!("dependency.config.json: {error}"))?;
                if bytes.len() > 65536 {
                    return Err("dependency.config.json: максимальный размер 64 КиБ".into());
                }
                serde_json::from_slice(&bytes)
                    .map_err(|error| format!("dependency.config.json: {error}"))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => RulesConfig::default(),
            Err(error) => return Err(format!("dependency.config.json: {error}")),
        };
        for name in config.rules.keys() {
            if !RULES.contains(&name.as_str()) {
                return Err(format!(
                    "dependency.config.json: неизвестное правило `{name}`"
                ));
            }
        }
        let mut exclusions = HashMap::new();
        for name in RULES {
            let rule = config.rules.entry(name.into()).or_default();
            if name == "high-fan-out" || name == "high-fan-in" {
                let threshold =
                    rule.threshold
                        .get_or_insert(if name == "high-fan-out" { 20 } else { 30 });
                if *threshold == 0 || *threshold > 1_000_000 {
                    return Err(format!(
                        "{name}: threshold должен быть в диапазоне 1..1000000"
                    ));
                }
            } else if rule.threshold.is_some() {
                return Err(format!(
                    "{name}: threshold поддерживается только для fan-in/out"
                ));
            }
            let mut builder = GlobSetBuilder::new();
            for pattern in &rule.exclude {
                builder.add(
                    Glob::new(pattern).map_err(|error| {
                        format!("{name}: неверный exclude `{pattern}`: {error}")
                    })?,
                );
            }
            exclusions.insert(
                name.into(),
                builder.build().map_err(|error| error.to_string())?,
            );
        }
        Ok(Self { config, exclusions })
    }
    fn allows(&self, rule: &str, path: &str) -> bool {
        !self.exclusions[rule].is_match(path)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedRule {
    pub rule_id: String,
    pub reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesRun {
    pub version: String,
    pub config_hash: String,
    pub effective_config: RulesConfig,
    pub completed_rules: Vec<String>,
    pub skipped_rules: Vec<SkippedRule>,
    pub limitation: String,
    pub source_file_count: usize,
    pub coverage_issue_count: usize,
    pub duration_ms: u128,
}

// Keep the serialized assessment explicit at each rule; a builder obscures these few fields.
#[allow(clippy::too_many_arguments)]
fn diagnostic(
    rule: &str,
    severity: &str,
    confidence: &str,
    title: &str,
    explanation: String,
    suggestion: &str,
    mut ids: Vec<String>,
    evidence: Evidence,
) -> Diagnostic {
    ids.sort();
    ids.dedup();
    let identity = serde_json::to_vec(&(crate::storage::RULES_VERSION, rule, &ids, &evidence))
        .expect("diagnostic identity serializes");
    Diagnostic {
        id: format!("{rule}:{:x}", Sha256::digest(identity)),
        rule_id: rule.into(),
        severity: severity.into(),
        confidence: confidence.into(),
        category: if rule.starts_with("resolution") || rule == "parse-error" {
            "coverage"
        } else {
            "architecture"
        }
        .into(),
        title: title.into(),
        explanation,
        suggestion: suggestion.into(),
        node_ids: ids,
        evidence,
    }
}

pub struct RuleInput<'a> {
    pub root: &'a Path,
    pub nodes: &'a [GraphNode],
    pub edges: &'a [GraphEdge],
    pub cache: &'a [ParsedFileCache],
    pub issues: &'a [ResolutionIssue],
    pub scan_issues: &'a [(String, String)],
    pub manifests: &'a [String],
}
pub fn evaluate(
    settings: Settings,
    input: RuleInput<'_>,
) -> Result<(Vec<Diagnostic>, RulesRun), String> {
    let RuleInput {
        root,
        nodes,
        edges,
        cache,
        issues,
        scan_issues,
        manifests,
    } = input;
    let started = Instant::now();
    let sources: Vec<String> = nodes
        .iter()
        .filter(|node| node.kind == "file" && node.language.is_some())
        .map(|node| node.id.clone())
        .collect();
    let source_ids: HashSet<&str> = sources.iter().map(String::as_str).collect();
    let file_ids: HashSet<&str> = nodes
        .iter()
        .filter(|node| node.kind == "file")
        .map(|node| node.id.as_str())
        .collect();
    let internal: Vec<GraphEdge> = edges
        .iter()
        .filter(|edge| {
            file_ids.contains(edge.source.as_str()) && file_ids.contains(edge.target.as_str())
        })
        .cloned()
        .collect();
    let mut inbound: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    let mut outbound: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    let mut incoming_imports: HashMap<&str, Vec<GraphEdge>> = HashMap::new();
    let mut outgoing_imports: HashMap<&str, Vec<GraphEdge>> = HashMap::new();
    let mut by_source: HashMap<&str, Vec<GraphEdge>> = HashMap::new();
    let mut consumers = HashSet::new();
    for edge in &internal {
        by_source
            .entry(&edge.source)
            .or_default()
            .push(edge.clone());
        consumers.insert(edge.target.as_str());
        if edge.source != edge.target && edge.kind != "type_only_import" {
            inbound
                .entry(&edge.target)
                .or_default()
                .insert(&edge.source);
            outbound
                .entry(&edge.source)
                .or_default()
                .insert(&edge.target);
            incoming_imports
                .entry(&edge.target)
                .or_default()
                .push(edge.clone());
            outgoing_imports
                .entry(&edge.source)
                .or_default()
                .push(edge.clone());
        }
    }
    let parse_errors = cache.iter().filter(|file| file.parse_error).count();
    let mut coverage_issue_count = parse_errors
        + scan_issues.len()
        + issues
            .iter()
            .filter(|issue| issue.status != "external")
            .count();
    let mut diagnostics = Vec::new();
    let mut completed_rules = Vec::new();
    let mut skipped_rules = Vec::new();
    for rule in RULES {
        let config = &settings.config.rules[rule];
        if !config.enabled {
            skipped_rules.push(SkippedRule {
                rule_id: rule.into(),
                reason: "Отключено конфигурацией снимка".into(),
            });
            continue;
        }
        match rule {
            "dependency-cycle" => {
                let static_edges: Vec<GraphEdge> = internal
                    .iter()
                    .filter(|edge| {
                        matches!(
                            edge.kind.as_str(),
                            "import" | "reexport" | "commonjs_import"
                        )
                    })
                    .cloned()
                    .collect();
                let mut seen = HashSet::new();
                for (graph, graph_edges) in [("static", &static_edges), ("full", &internal)] {
                    for component in cyclic_components(&sources, graph_edges) {
                        if !component.iter().any(|path| settings.allows(rule, path))
                            || !seen.insert(component.clone())
                        {
                            continue;
                        }
                        let members: HashSet<&str> = component.iter().map(String::as_str).collect();
                        let component_edges: Vec<GraphEdge> = component
                            .iter()
                            .flat_map(|path| by_source.get(path.as_str()).into_iter().flatten())
                            .filter(|edge| members.contains(edge.target.as_str()))
                            .cloned()
                            .collect();
                        let kinds = component_edges
                            .iter()
                            .map(|edge| edge.kind.clone())
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect();
                        let route_edges: Vec<GraphEdge> = component_edges
                            .into_iter()
                            .filter(|edge| {
                                graph == "full"
                                    || matches!(
                                        edge.kind.as_str(),
                                        "import" | "reexport" | "commonjs_import"
                                    )
                            })
                            .collect();
                        let route = cycle_route(&component, &route_edges)
                            .ok_or("Не удалось построить доказательство SCC")?;
                        diagnostics.push(diagnostic(rule, if graph == "static" { "warning" } else { "info" }, "confirmed", "Циклические зависимости",
                            format!("{} файлов в сильно связанной компоненте ({graph}). Показан один подтверждённый замкнутый маршрут; остальные файлы компоненты могут в него не входить. Классификация статических рёбер соответствует сохранённым данным анализатора.", component.len()),
                            "Проверьте направление ответственности и возможность выделить общий контракт. Наличие цикла само по себе не доказывает ошибку выполнения.", component.clone(), Evidence::Cycle { graph: graph.into(), component, route, edge_kinds: kinds }));
                    }
                }
            }
            "high-fan-out" | "high-fan-in" => {
                let threshold = config.threshold.expect("validated threshold");
                let index = if rule == "high-fan-out" {
                    &outbound
                } else {
                    &inbound
                };
                for path in &sources {
                    if !settings.allows(rule, path) {
                        continue;
                    }
                    let neighbors: Vec<String> = index
                        .get(path.as_str())
                        .into_iter()
                        .flat_map(|items| items.iter().map(|path| (*path).to_owned()))
                        .collect();
                    if neighbors.len() <= threshold {
                        continue;
                    }
                    let imports = if rule == "high-fan-out" {
                        &outgoing_imports
                    } else {
                        &incoming_imports
                    }
                    .get(path.as_str())
                    .cloned()
                    .unwrap_or_default();
                    let mut ids = neighbors.clone();
                    ids.push(path.clone());
                    diagnostics.push(diagnostic(rule, if rule == "high-fan-out" { "warning" } else { "info" }, "heuristic",
                        if rule == "high-fan-out" { "Много прямых зависимостей" } else { "Много потребителей" },
                        format!("`{path}`: {} уникальных внутренних файлов, порог {threshold}. Повторные импорты, type-only, npm и самоссылки не учитываются. Порог — ориентир для ревью, не универсальная норма.", neighbors.len()),
                        "Оцените ответственность файла. Высокая связанность может быть оправдана ролью entry point, barrel или общего модуля.", ids, Evidence::Coupling { observed: neighbors.len(), threshold, neighbors, imports }));
                }
            }
            "no-known-consumers" => {
                if coverage_issue_count > 0 {
                    skipped_rules.push(SkippedRule { rule_id: rule.into(), reason: "Пропущено: ошибки разбора, чтения или разрешения импортов делают поиск потребителей неполным".into() });
                    continue;
                }
                let entries = match entry_points(root, manifests, nodes) {
                    Ok(entries) => entries,
                    Err(reason) => {
                        coverage_issue_count += 1;
                        skipped_rules.push(SkippedRule {
                            rule_id: rule.into(),
                            reason,
                        });
                        continue;
                    }
                };
                for path in &sources {
                    if consumers.contains(path.as_str())
                        || entries.contains(path)
                        || conventional_exclusion(path)
                        || !settings.allows(rule, path)
                    {
                        continue;
                    }
                    diagnostics.push(diagnostic(rule, "info", "heuristic", "Не найдены потребители",
                        format!("У `{path}` нет входящих импортов в известном графе. Это не доказательство неиспользования файла."),
                        "Проверьте framework routes, entry points, плагины и динамические загрузчики. Не удаляйте файл только на основании этого сигнала.", vec![path.clone()],
                        Evidence::Consumers { checked_exclusions: vec!["package.json main/module/exports/bin".into(), "тесты, декларации, конфигурации".into(), "все известные виды входящих импортов".into()], limitation: COVERAGE_LIMITATION.into() }));
                }
            }
            "resolution" => {
                for issue in issues {
                    if settings.allows(rule, &issue.source) {
                        diagnostics.push(diagnostic(
                            &format!("resolution-{}", issue.status),
                            if issue.status == "external" {
                                "info"
                            } else {
                                "warning"
                            },
                            "confirmed",
                            "Разрешение импорта",
                            issue.reason.clone(),
                            &issue.hint,
                            vec![issue.source.clone()],
                            Evidence::Resolution {
                                issue: issue.clone(),
                            },
                        ));
                    }
                }
            }
            "parse-error" => {
                for (source, reason) in cache.iter().filter(|file| file.parse_error).map(|file| (file.relative_path.clone(), "AST содержит синтаксические ошибки; импортные связи могут быть неполными".to_string())).chain(scan_issues.iter().cloned()) {
                    if settings.allows(rule, &source) {
                        diagnostics.push(diagnostic(rule, "warning", "confirmed", "Неполное покрытие анализа", reason.clone(), "Проверьте доступность файла, кодировку и поддержку синтаксиса. Отсутствие найденных связей не гарантирует их отсутствия.", if source_ids.contains(source.as_str()) { vec![source.clone()] } else { Vec::new() }, Evidence::Parse { source, reason }));
                    }
                }
            }
            _ => unreachable!(),
        }
        completed_rules.push(rule.into());
    }
    diagnostics.sort_by(|a, b| a.id.cmp(&b.id));
    diagnostics.dedup_by(|a, b| a.id == b.id);
    let config_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&settings.config).map_err(|error| error.to_string())?)
    );
    Ok((
        diagnostics,
        RulesRun {
            version: crate::storage::RULES_VERSION.into(),
            config_hash,
            effective_config: settings.config,
            completed_rules,
            skipped_rules,
            limitation: COVERAGE_LIMITATION.into(),
            source_file_count: sources.len(),
            coverage_issue_count,
            duration_ms: started.elapsed().as_millis(),
        },
    ))
}

fn conventional_exclusion(path: &str) -> bool {
    let lower = path.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    lower
        .split('/')
        .any(|part| matches!(part, "test" | "tests" | "__tests__" | "__mocks__"))
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.contains(".config.")
        || name.ends_with(".d.ts")
        || name.ends_with(".d.mts")
        || name.ends_with(".d.cts")
        || name.starts_with(".")
}

fn entry_points(
    root: &Path,
    manifests: &[String],
    nodes: &[GraphNode],
) -> Result<HashSet<String>, String> {
    let mut entries = HashSet::new();
    fn targets(value: &serde_json::Value, result: &mut Vec<String>) {
        match value {
            serde_json::Value::String(path) => result.push(path.clone()),
            serde_json::Value::Object(map) => {
                for value in map.values() {
                    targets(value, result);
                }
            }
            serde_json::Value::Array(items) => {
                for value in items {
                    targets(value, result);
                }
            }
            _ => {}
        }
    }
    for manifest in manifests {
        let mut bytes = Vec::new();
        fs::File::open(root.join(manifest))
            .map_err(|error| format!("Entry points: {manifest}: {error}"))?
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Entry points: {manifest}: {error}"))?;
        if bytes.len() > 1_048_576 {
            return Err(format!("Entry points: {manifest} превышает 1 МиБ"));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Entry points: {manifest}: {error}"))?;
        let parent = Path::new(manifest).parent().unwrap_or(Path::new(""));
        let mut paths = Vec::new();
        for field in ["main", "module", "exports", "bin"] {
            if let Some(value) = value.get(field) {
                targets(value, &mut paths);
            }
        }
        for path in paths {
            if Path::new(&path).is_absolute() || path.split('/').any(|part| part == "..") {
                continue;
            }
            let pattern = parent
                .join(path.strip_prefix("./").unwrap_or(&path))
                .to_string_lossy()
                .replace('\\', "/");
            let mut variants = vec![pattern.clone()];
            if let Some(stem) = pattern.strip_suffix(".js") {
                variants.extend([format!("{stem}.ts"), format!("{stem}.tsx")]);
            }
            if let Some(stem) = pattern.strip_suffix(".mjs") {
                variants.push(format!("{stem}.mts"));
            }
            if let Some(stem) = pattern.strip_suffix(".cjs") {
                variants.push(format!("{stem}.cts"));
            }
            let mut builder = GlobSetBuilder::new();
            for variant in variants {
                builder.add(
                    Glob::new(&variant)
                        .map_err(|error| format!("Entry points: {manifest}: {error}"))?,
                );
            }
            let matcher = builder.build().map_err(|error| error.to_string())?;
            entries.extend(
                nodes
                    .iter()
                    .filter(|node| node.kind == "file" && matcher.is_match(&node.path))
                    .map(|node| node.id.clone()),
            );
        }
    }
    Ok(entries)
}

// Iterative Kosaraju: SCC members are sorted, not a claimed cycle route.
pub(crate) fn cyclic_components(ids: &[String], edges: &[GraphEdge]) -> Vec<Vec<String>> {
    let file_ids: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut adjacency = HashMap::<&str, Vec<&str>>::new();
    let mut reverse = HashMap::<&str, Vec<&str>>::new();
    for edge in edges.iter().filter(|edge| {
        file_ids.contains(edge.source.as_str()) && file_ids.contains(edge.target.as_str())
    }) {
        adjacency
            .entry(&edge.source)
            .or_default()
            .push(&edge.target);
        reverse.entry(&edge.target).or_default().push(&edge.source);
    }
    for list in adjacency.values_mut().chain(reverse.values_mut()) {
        list.sort_unstable();
        list.dedup();
    }
    let mut visited = HashSet::new();
    let mut finish = Vec::new();
    for id in ids {
        let mut stack = vec![(id.as_str(), false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                finish.push(node);
                continue;
            }
            if !visited.insert(node) {
                continue;
            }
            stack.push((node, true));
            if let Some(neighbors) = adjacency.get(node) {
                for neighbor in neighbors.iter().rev() {
                    if !visited.contains(neighbor) {
                        stack.push((neighbor, false));
                    }
                }
            }
        }
    }
    visited.clear();
    let mut result = Vec::new();
    for node in finish.into_iter().rev() {
        if !visited.insert(node) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            component.push(current.to_owned());
            if let Some(neighbors) = reverse.get(current) {
                for neighbor in neighbors {
                    if visited.insert(neighbor) {
                        stack.push(neighbor);
                    }
                }
            }
        }
        if component.len() > 1
            || adjacency
                .get(node)
                .is_some_and(|neighbors| neighbors.contains(&node))
        {
            component.sort();
            result.push(component);
        }
    }
    result.sort();
    result
}

fn cycle_route(component: &[String], edges: &[GraphEdge]) -> Option<Vec<GraphEdge>> {
    let members: HashSet<&str> = component.iter().map(String::as_str).collect();
    let mut adjacency = BTreeMap::<&str, Vec<&GraphEdge>>::new();
    for edge in edges.iter().filter(|edge| {
        members.contains(edge.source.as_str()) && members.contains(edge.target.as_str())
    }) {
        adjacency.entry(&edge.source).or_default().push(edge);
    }
    for list in adjacency.values_mut() {
        list.sort_by(|a, b| (&a.target, &a.kind, a.line).cmp(&(&b.target, &b.kind, b.line)));
    }
    let mut color = HashMap::<&str, u8>::new();
    let mut parents = HashMap::<&str, &GraphEdge>::new();
    for root in component {
        if color.contains_key(root.as_str()) {
            continue;
        }
        color.insert(root, 1);
        let mut stack = vec![(root.as_str(), 0usize)];
        while let Some((node, index)) = stack.last_mut() {
            let Some(edge) = adjacency
                .get(node)
                .and_then(|list| list.get(*index))
                .copied()
            else {
                color.insert(node, 2);
                stack.pop();
                continue;
            };
            *index += 1;
            match color.get(edge.target.as_str()).copied().unwrap_or(0) {
                0 => {
                    parents.insert(&edge.target, edge);
                    color.insert(&edge.target, 1);
                    stack.push((&edge.target, 0));
                }
                1 => {
                    let mut route = vec![edge.clone()];
                    let mut current = edge.source.as_str();
                    while current != edge.target {
                        let parent = parents.get(current)?;
                        route.push((*parent).clone());
                        current = &parent.source;
                    }
                    route.reverse();
                    return Some(route);
                }
                _ => {}
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::analyze_project;
    use tempfile::{TempDir, tempdir};

    fn project(files: &[(&str, &str)]) -> TempDir {
        let root = tempdir().unwrap();
        for (path, contents) in files {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        root
    }
    fn edge(source: &str, target: &str, kind: &str) -> GraphEdge {
        GraphEdge {
            id: format!("{source}:{target}"),
            source: source.into(),
            target: target.into(),
            kind: kind.into(),
            line: 1,
        }
    }
    #[test]
    fn cycle_witness_follows_edges_instead_of_sorted_component() {
        let edges = vec![
            edge("a", "c", "import"),
            edge("c", "b", "reexport"),
            edge("b", "a", "import"),
        ];
        let ids = vec!["a".into(), "b".into(), "c".into()];
        let components = cyclic_components(&ids, &edges);
        assert_eq!(components, vec![ids]);
        let route = cycle_route(&components[0], &edges).unwrap();
        assert_eq!(route.len(), 3);
        for index in 0..route.len() {
            assert_eq!(route[index].target, route[(index + 1) % route.len()].source);
        }
        assert_eq!(route[0].target, "c");
    }
    #[test]
    fn cycles_distinguish_static_type_only_dynamic_and_self() {
        let root = project(&[
            (
                "a.ts",
                "import type { B } from './b'; export type A = number;",
            ),
            (
                "b.ts",
                "import type { A } from './a'; export type B = number;",
            ),
            ("c.ts", "import('./d');"),
            ("d.ts", "import('./c');"),
            ("e.ts", "import './e';"),
            ("f.ts", "import './g'; import type { G } from './g';"),
            ("g.ts", "import './f'; export type G = number;"),
        ]);
        let result = analyze_project(root.path().to_str().unwrap(), vec![])
            .unwrap()
            .result;
        let cycles: Vec<_> = result
            .diagnostics
            .iter()
            .filter_map(|d| match &d.evidence {
                Evidence::Cycle {
                    graph,
                    component,
                    route,
                    edge_kinds,
                } => Some((graph, component, route, edge_kinds)),
                _ => None,
            })
            .collect();
        assert_eq!(cycles.len(), 4);
        assert_eq!(
            cycles
                .iter()
                .filter(|(graph, _, _, _)| *graph == "full")
                .count(),
            2
        );
        assert_eq!(
            cycles
                .iter()
                .filter(|(graph, _, _, _)| *graph == "static")
                .count(),
            2
        );
        assert!(cycles.iter().any(|(_, component, route, _)| *component
            == &vec!["e.ts".to_string()]
            && route.len() == 1));
        assert!(
            cycles
                .iter()
                .any(|(graph, component, _, kinds)| *graph == "static"
                    && component.contains(&"f.ts".into())
                    && kinds.contains(&"type_only_import".into()))
        );
        for (_, _, route, _) in cycles {
            for i in 0..route.len() {
                assert_eq!(route[i].target, route[(i + 1) % route.len()].source);
            }
        }
    }
    #[test]
    fn coupling_counts_unique_internal_neighbors_and_preserves_evidence() {
        let root = project(&[
            (
                "dependency.config.json",
                r#"{"rules":{"high-fan-out":{"threshold":1},"high-fan-in":{"threshold":1}}}"#,
            ),
            (
                "a.ts",
                "import './b'; import './b'; export { c } from './c'; import type { D } from './d'; import 'react'; import './a';",
            ),
            ("b.ts", "export const b = 1;"),
            ("c.ts", "export const c = 1;"),
            ("d.ts", "export type D = number;"),
            ("e.ts", "import './b';"),
        ]);
        let result = analyze_project(root.path().to_str().unwrap(), vec![])
            .unwrap()
            .result;
        let fan_out: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.rule_id == "high-fan-out")
            .collect();
        assert_eq!(fan_out.len(), 1);
        if let Evidence::Coupling {
            observed,
            neighbors,
            imports,
            ..
        } = &fan_out[0].evidence
        {
            assert_eq!(*observed, 2);
            assert_eq!(neighbors, &["b.ts", "c.ts"]);
            assert_eq!(imports.len(), 3);
        } else {
            panic!("missing coupling evidence");
        }
        assert_eq!(
            result
                .diagnostics
                .iter()
                .filter(|d| d.rule_id == "high-fan-in")
                .count(),
            1
        );
    }
    #[test]
    fn consumer_candidates_protect_entry_points_tests_declarations_and_config() {
        let root = project(&[
            (
                "package.json",
                r#"{"main":"./entry.js","module":"./esm.ts","exports":{"./*":"./public/*.ts"},"bin":{"tool":"./cli.ts"}}"#,
            ),
            ("entry.ts", "export const x = 1"),
            ("esm.ts", "export const x = 1"),
            ("public/api.ts", "export const x = 1"),
            ("cli.ts", "export const x = 1"),
            ("a.test.ts", "export const x = 1"),
            ("test/b.ts", "export const x = 1"),
            ("types.d.ts", "export type X = number"),
            ("vite.config.ts", "export const x = 1"),
            ("orphan.ts", "export const x = 1"),
            ("packages/tool/package.json", r#"{"bin":"./src/run.ts"}"#),
            ("packages/tool/src/run.ts", "export const x = 1"),
        ]);
        let result = analyze_project(root.path().to_str().unwrap(), vec![])
            .unwrap()
            .result;
        let candidates: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.rule_id == "no-known-consumers")
            .collect();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].node_ids, ["orphan.ts"]);
        assert_eq!(candidates[0].confidence, "heuristic");
    }
    #[test]
    fn incomplete_coverage_skips_consumer_rule_even_if_coverage_diagnostics_disabled() {
        let root = project(&[
            ("a.ts", "import './missing';"),
            ("broken.ts", "export const = ;"),
            (
                "dependency.config.json",
                r#"{"rules":{"resolution":{"enabled":false},"parse-error":{"enabled":false}}}"#,
            ),
        ]);
        let result = analyze_project(root.path().to_str().unwrap(), vec![])
            .unwrap()
            .result;
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.rule_id == "no-known-consumers")
        );
        let run = result.rules_run.unwrap();
        assert!(run.coverage_issue_count >= 2);
        assert!(
            run.skipped_rules
                .iter()
                .any(|r| r.rule_id == "no-known-consumers")
        );
    }
    #[test]
    fn invalid_configuration_fails_instead_of_silently_using_defaults() {
        for config in [
            r#"{"rules":{"unknown":{}}}"#,
            r#"{"rules":{"high-fan-in":{"threshold":0}}}"#,
            r#"{"rules":{"dependency-cycle":{"threshold":2}}}"#,
            r#"{"rules":{"high-fan-in":{"exclude":["["]}}}"#,
            r#"{"rules":{"high-fan-in":{"extra":true}}}"#,
            "{bad",
        ] {
            let root = project(&[
                ("a.ts", "export const a = 1;"),
                ("dependency.config.json", config),
            ]);
            assert!(
                analyze_project(root.path().to_str().unwrap(), vec![]).is_err(),
                "accepted {config}"
            );
        }
        let root = project(&[("dependency.config.json", &" ".repeat(65537))]);
        assert!(Settings::load(root.path()).is_err());
    }
    #[test]
    fn config_changes_reuse_ast_preserve_history_and_invalid_config_does_not_publish() {
        let root = project(&[
            ("a.ts", "import './b'; import './c';"),
            ("b.ts", "export const b = 1;"),
            ("c.ts", "export const c = 1;"),
        ]);
        let db = tempdir().unwrap();
        let state = crate::storage::initialize(db.path().join("db.sqlite3")).unwrap();
        let first = analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
        let first = crate::storage::save_snapshot(
            &state,
            first.result,
            first.file_cache,
            first.parsed_file_count,
            first.reused_file_count,
        )
        .unwrap();
        let old = serde_json::to_string(&first.result).unwrap();
        fs::write(root.path().join("dependency.config.json"), r#"{"rules":{"high-fan-out":{"threshold":1},"no-known-consumers":{"exclude":["a.ts"]}}}"#).unwrap();
        let second = analyze_project(
            root.path().to_str().unwrap(),
            crate::storage::load_cache(&state, Some(&first.project.id)).unwrap(),
        )
        .unwrap();
        assert_eq!(second.parsed_file_count, 0);
        assert_eq!(second.reused_file_count, 3);
        assert!(
            second
                .result
                .diagnostics
                .iter()
                .any(|d| d.rule_id == "high-fan-out")
        );
        assert!(
            !second
                .result
                .diagnostics
                .iter()
                .any(|d| d.rule_id == "no-known-consumers")
        );
        assert_ne!(
            first.result.rules_run.as_ref().unwrap().config_hash,
            second.result.rules_run.as_ref().unwrap().config_hash
        );
        let second = crate::storage::save_snapshot(
            &state,
            second.result,
            second.file_cache,
            second.parsed_file_count,
            second.reused_file_count,
        )
        .unwrap();
        assert_eq!(
            old,
            serde_json::to_string(
                &crate::storage::load_snapshot(&state, &first.snapshot.id)
                    .unwrap()
                    .result
            )
            .unwrap()
        );
        let reopened = crate::storage::initialize(state.database_path.clone()).unwrap();
        assert_eq!(
            second.result.diagnostics.len(),
            crate::storage::load_project(&reopened, &first.project.id)
                .unwrap()
                .result
                .diagnostics
                .len()
        );
        fs::write(root.path().join("dependency.config.json"), "bad").unwrap();
        assert!(analyze_project(root.path().to_str().unwrap(), vec![]).is_err());
        assert_eq!(
            crate::storage::load_project(&reopened, &first.project.id)
                .unwrap()
                .snapshot
                .id,
            second.snapshot.id
        );
        let mut legacy = serde_json::to_value(first.result).unwrap();
        legacy.as_object_mut().unwrap().remove("diagnostics");
        legacy.as_object_mut().unwrap().remove("rulesRun");
        let legacy: crate::analysis::AnalysisResult = serde_json::from_value(legacy).unwrap();
        assert!(legacy.rules_run.is_none());
        assert!(legacy.diagnostics.is_empty());
    }
    #[test]
    fn long_cycle_witness_is_iterative() {
        let ids: Vec<String> = (0..15000).map(|n| format!("n{n:05}")).collect();
        let edges: Vec<GraphEdge> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| edge(id, &ids[(i + 1) % ids.len()], "import"))
            .collect();
        let components = cyclic_components(&ids, &edges);
        assert_eq!(components.len(), 1);
        assert_eq!(
            cycle_route(&components[0], &edges).unwrap().len(),
            ids.len()
        );
    }

    #[test]
    fn scanner_reports_non_utf8_sources_and_skips_consumer_guessing() {
        let root = project(&[("orphan.ts", "export const x = 1;")]);
        fs::write(root.path().join("bad.ts"), [0xff, 0xfe]).unwrap();
        let result = analyze_project(root.path().to_str().unwrap(), vec![])
            .unwrap()
            .result;
        assert!(result.diagnostics.iter().any(
            |item| matches!(&item.evidence, Evidence::Parse { source, .. } if source == "bad.ts")
        ));
        assert!(
            result
                .rules_run
                .unwrap()
                .skipped_rules
                .iter()
                .any(|rule| rule.rule_id == "no-known-consumers")
        );
    }

    #[test]
    fn old_parser_cache_is_not_reused_with_wrong_type_only_classification() {
        let root = project(&[
            ("a.ts", "export type { B } from './b';"),
            ("b.ts", "export type B = number;"),
        ]);
        let mut first = analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
        for file in &mut first.file_cache {
            file.analyzer_version = "js-ts-parser-1".into();
            for import in &mut file.imports {
                import.kind = "reexport".into();
            }
        }
        let second = analyze_project(root.path().to_str().unwrap(), first.file_cache).unwrap();
        assert_eq!(second.reused_file_count, 0);
        assert_eq!(second.result.edges[0].kind, "type_only_import");
        let third = analyze_project(root.path().to_str().unwrap(), second.file_cache).unwrap();
        assert_eq!(third.parsed_file_count, 0);
    }

    #[test]
    #[ignore = "explicit read-only profiling of locally selected repositories"]
    fn profile_selected_repositories() {
        let roots = std::env::var("DEPENDENCY_PROFILE_ROOTS")
            .expect("set DEPENDENCY_PROFILE_ROOTS separated by semicolons");
        for (index, root) in roots.split(';').filter(|root| !root.is_empty()).enumerate() {
            let started = Instant::now();
            let output = analyze_project(root, vec![]).unwrap();
            let run = output.result.rules_run.as_ref().unwrap();
            let mut counts = BTreeMap::<&str, usize>::new();
            for item in &output.result.diagnostics {
                *counts.entry(&item.rule_id).or_default() += 1;
            }
            println!(
                "PROFILE {} sources={} edges={} analysis_ms={} rules_ms={} coverage={} counts={counts:?}",
                output.result.summary.name,
                run.source_file_count,
                output.result.edges.len(),
                started.elapsed().as_millis(),
                run.duration_ms,
                run.coverage_issue_count
            );
            if let Ok(directory) = std::env::var("DEPENDENCY_DUMP_DIR") {
                fs::create_dir_all(&directory).unwrap();
                fs::write(
                    Path::new(&directory).join(format!("analysis-{index}.json")),
                    serde_json::to_vec(&output.result).unwrap(),
                )
                .unwrap();
            }
        }
    }
}
