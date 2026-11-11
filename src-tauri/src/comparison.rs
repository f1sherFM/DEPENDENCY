use crate::{
    analysis::{AnalysisResult, GraphEdge},
    snapshot::FileFact,
    storage::{self, AppState},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{Arc, Mutex},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub category: String,
    pub status: String,
    pub label: String,
    pub detail: String,
    pub base_node_id: Option<String>,
    pub target_node_id: Option<String>,
    pub old_count: Option<usize>,
    pub new_count: Option<usize>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub base_snapshot_id: String,
    pub target_snapshot_id: String,
    pub compatible: bool,
    pub inventory_available: bool,
    pub diagnostics_comparable: bool,
    pub warnings: Vec<String>,
    pub counts: BTreeMap<String, usize>,
    pub total: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub items: Vec<Change>,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub matching_count: usize,
}
struct Comparison {
    summary: Summary,
    items: Vec<Change>,
}
#[derive(Clone, Default)]
pub struct Comparisons {
    current: Arc<Mutex<Option<Arc<Comparison>>>>,
    gate: Arc<Mutex<()>>,
}
fn change(
    category: &str,
    status: &str,
    label: String,
    detail: String,
    base: Option<String>,
    target: Option<String>,
    counts: Option<(usize, usize)>,
) -> Change {
    Change {
        category: category.into(),
        status: status.into(),
        label,
        detail,
        base_node_id: base,
        target_node_id: target,
        old_count: counts.map(|v| v.0),
        new_count: counts.map(|v| v.1),
    }
}
impl Comparisons {
    pub fn compare(
        &self,
        state: &AppState,
        base_id: &str,
        target_id: &str,
    ) -> Result<Summary, String> {
        let _gate = self
            .gate
            .lock()
            .map_err(|_| "Comparison worker недоступен")?;
        let (base_meta, base) = storage::read_impact_snapshot(state, base_id)?;
        let (target_meta, target) = storage::read_impact_snapshot(state, target_id)?;
        if base_meta.project_id != target_meta.project_id {
            return Err("Выберите два снимка одного проекта".into());
        }
        let base_files = storage::read_inventory(state, base_id)?;
        let target_files = storage::read_inventory(state, target_id)?;
        let inventory_available = base.basis.is_some() && target.basis.is_some();
        let profiles = base
            .basis
            .as_ref()
            .zip(target.basis.as_ref())
            .is_some_and(|(a, b)| {
                a.project_scope == b.project_scope
                    && a.resolution_profile == b.resolution_profile
                    && a.capture_policy_hash == b.capture_policy_hash
            });
        let compatible = profiles
            && base_meta.analyzer_version == target_meta.analyzer_version
            && base_meta.rules_version.split('+').next_back()
                == target_meta.rules_version.split('+').next_back();
        let diagnostics_comparable = compatible
            && base_meta.rules_version == target_meta.rules_version
            && base
                .rules_run
                .as_ref()
                .zip(target.rules_run.as_ref())
                .is_some_and(|(a, b)| a.config_hash == b.config_hash);
        let mut warnings = Vec::new();
        if !compatible {
            warnings.push("Разные/неизвестные профили capture/resolver или версии анализатора: показаны наблюдаемые изменения, не оценка ухудшения архитектуры".into());
        }
        if !inventory_available {
            warnings.push(
                "Legacy snapshot без inventory: сравнение файлов и переименований недоступно"
                    .into(),
            );
        }
        let gaps = |result: &AnalysisResult| {
            result
                .rules_run
                .as_ref()
                .is_none_or(|run| run.coverage_issue_count > 0)
                || result
                    .basis
                    .as_ref()
                    .is_some_and(|basis| basis.omitted_count > 0)
        };
        if gaps(&base) || gaps(&target) {
            warnings.push("Есть пробелы/неизвестное покрытие; исчезнувшая связь не обязательно означает удаление импорта".into());
        }
        if !diagnostics_comparable {
            warnings.push("Состав диагностик не сопоставляется: версии/эффективные настройки правил различаются или неизвестны".into());
        }
        let (mut items, mapping) = file_changes(&base_files, &target_files, inventory_available);
        edge_changes(
            &base,
            &target,
            &base_files,
            &target_files,
            &mapping,
            compatible,
            &mut items,
        );
        for (name, old, new) in [
            (
                "Исходники",
                base.summary.source_file_count,
                target.summary.source_file_count,
            ),
            (
                "Импортные рёбра",
                base.summary.dependency_count,
                target.summary.dependency_count,
            ),
            ("SCC", base.summary.cycle_count, target.summary.cycle_count),
        ] {
            if old != new {
                items.push(change(
                    "metric",
                    "observed-delta",
                    name.into(),
                    "Наблюдаемые значения в сохранённой области покрытия".into(),
                    None,
                    None,
                    Some((old, new)),
                ));
            }
        }
        if diagnostics_comparable {
            let key = |d: &crate::diagnostics::Diagnostic, remap: bool| {
                let subjects = match &d.evidence {
                    crate::diagnostics::Evidence::Coupling { imports, .. } => imports
                        .first()
                        .map(|edge| {
                            vec![if d.rule_id == "high-fan-out" {
                                edge.source.clone()
                            } else {
                                edge.target.clone()
                            }]
                        })
                        .unwrap_or_else(|| d.node_ids.clone()),
                    _ => d.node_ids.clone(),
                };
                let qualifier = match &d.evidence {
                    crate::diagnostics::Evidence::Resolution { issue } => issue.specifier.as_str(),
                    _ => "",
                };
                let mut ids: Vec<_> = subjects
                    .iter()
                    .map(|id| {
                        if remap {
                            mapping.get(id).unwrap_or(id).clone()
                        } else {
                            id.clone()
                        }
                    })
                    .collect();
                ids.sort();
                format!("{}:{ids:?}:{qualifier}", d.rule_id)
            };
            let mut a: BTreeMap<_, Vec<_>> = BTreeMap::new();
            for d in &base.diagnostics {
                a.entry(key(d, true)).or_default().push(d);
            }
            let mut b: BTreeMap<_, Vec<_>> = BTreeMap::new();
            for d in &target.diagnostics {
                b.entry(key(d, false)).or_default().push(d);
            }
            let evidence = |list: &Vec<&crate::diagnostics::Diagnostic>| {
                let mut items: Vec<_> = list
                    .iter()
                    .map(|d| serde_json::to_string(&d.evidence).unwrap_or_default())
                    .collect();
                items.sort();
                items
            };
            for k in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                let left = a.get(k);
                let right = b.get(k);
                let status = match (left, right) {
                    (None, Some(_)) => "observed-added",
                    (Some(_), None) => "no-longer-reported",
                    (Some(a), Some(b)) if evidence(a) != evidence(b) => "evidence-changed",
                    _ => continue,
                };
                let d = right.or(left).unwrap()[0];
                items.push(change(
                    "diagnostic",
                    status,
                    d.title.clone(),
                    format!(
                        "{} · Проверяйте evidence и покрытие обеих сторон",
                        d.rule_id
                    ),
                    left.and_then(|d| d[0].node_ids.first().cloned()),
                    right.and_then(|d| d[0].node_ids.first().cloned()),
                    Some((left.map_or(0, Vec::len), right.map_or(0, Vec::len))),
                ));
            }
        }
        if items.len() > 250_000 {
            return Err("Сравнение превышает 250 000 изменений. Результат не усечён".into());
        }
        let base_nodes: HashSet<_> = base.nodes.iter().map(|node| node.id.as_str()).collect();
        let target_nodes: HashSet<_> = target.nodes.iter().map(|node| node.id.as_str()).collect();
        for item in &mut items {
            item.base_node_id = item
                .base_node_id
                .take()
                .filter(|id| base_nodes.contains(id.as_str()));
            item.target_node_id = item
                .target_node_id
                .take()
                .filter(|id| target_nodes.contains(id.as_str()));
        }
        items.sort_by(|a, b| {
            (&a.category, &a.label, &a.status).cmp(&(&b.category, &b.label, &b.status))
        });
        let mut counts = BTreeMap::new();
        for item in &items {
            *counts.entry(item.category.clone()).or_insert(0) += 1;
        }
        let summary = Summary {
            id: uuid::Uuid::new_v4().to_string(),
            base_snapshot_id: base_id.into(),
            target_snapshot_id: target_id.into(),
            compatible,
            inventory_available,
            diagnostics_comparable,
            warnings,
            counts,
            total: items.len(),
        };
        *self
            .current
            .lock()
            .map_err(|_| "Comparison cache недоступен")? = Some(Arc::new(Comparison {
            summary: summary.clone(),
            items,
        }));
        Ok(summary)
    }
    pub fn page(
        &self,
        id: &str,
        offset: usize,
        limit: usize,
        category: &str,
        text: &str,
    ) -> Result<Page, String> {
        crate::git::page_limit(limit)?;
        if text.len() > 4096 {
            return Err("Слишком длинный поиск".into());
        }
        let data = self
            .current
            .lock()
            .map_err(|_| "Comparison cache недоступен")?
            .clone()
            .filter(|c| c.summary.id == id)
            .ok_or("Сравнение истекло, повторите запрос")?;
        let text = text.to_lowercase();
        let filtered: Vec<_> = data
            .items
            .iter()
            .filter(|item| {
                (category.is_empty() || item.category == category)
                    && format!("{} {}", item.label, item.detail)
                        .to_lowercase()
                        .contains(&text)
            })
            .collect();
        if offset > filtered.len() {
            return Err("Страница сравнения вне диапазона".into());
        }
        let items = filtered
            .iter()
            .skip(offset)
            .take(limit)
            .map(|item| (*item).clone())
            .collect::<Vec<_>>();
        let next = offset + items.len();
        Ok(Page {
            items,
            offset,
            next_offset: (next < filtered.len()).then_some(next),
            matching_count: filtered.len(),
        })
    }
}
fn file_changes(
    base: &[FileFact],
    target: &[FileFact],
    available: bool,
) -> (Vec<Change>, HashMap<String, String>) {
    if !available {
        return (Vec::new(), HashMap::new());
    }
    let a: BTreeMap<_, _> = base
        .iter()
        .filter(|f| f.status == "included")
        .map(|f| (f.path.clone(), f))
        .collect();
    let b: BTreeMap<_, _> = target
        .iter()
        .filter(|f| f.status == "included")
        .map(|f| (f.path.clone(), f))
        .collect();
    let mut mapping = HashMap::new();
    let mut items = Vec::new();
    for (path, left) in &a {
        if let Some(right) = b.get(path) {
            mapping.insert(path.clone(), path.clone());
            if left.content_hash != right.content_hash || left.mode != right.mode {
                items.push(change(
                    "file",
                    "modified",
                    path.clone(),
                    import_delta(left, right),
                    Some(path.clone()),
                    Some(path.clone()),
                    None,
                ));
            }
        }
    }
    let unmatched_a: Vec<_> = a.values().filter(|f| !b.contains_key(&f.path)).collect();
    let unmatched_b: Vec<_> = b.values().filter(|f| !a.contains_key(&f.path)).collect();
    let mut hashes_a = HashMap::<&str, Vec<&FileFact>>::new();
    let mut hashes_b = HashMap::<&str, Vec<&FileFact>>::new();
    for f in unmatched_a {
        if let Some(hash) = f.content_hash.as_deref() {
            hashes_a.entry(hash).or_default().push(f);
        }
    }
    for f in unmatched_b {
        if let Some(hash) = f.content_hash.as_deref() {
            hashes_b.entry(hash).or_default().push(f);
        }
    }
    let mut moved_targets = HashSet::new();
    let mut frequency_a = HashMap::<&str, usize>::new();
    let mut frequency_b = HashMap::<&str, usize>::new();
    for fact in a.values() {
        if let Some(hash) = fact.content_hash.as_deref() {
            *frequency_a.entry(hash).or_default() += 1;
        }
    }
    for fact in b.values() {
        if let Some(hash) = fact.content_hash.as_deref() {
            *frequency_b.entry(hash).or_default() += 1;
        }
    }
    for (hash, left) in &hashes_a {
        if left.len() == 1
            && frequency_a.get(hash) == Some(&1)
            && frequency_b.get(hash) == Some(&1)
            && let Some(right) = hashes_b.get(hash)
            && right.len() == 1
        {
            let left = left[0];
            let right = right[0];
            mapping.insert(left.path.clone(), right.path.clone());
            moved_targets.insert(right.path.clone());
            items.push(change(
                "file",
                "moved-exact",
                format!("{} → {}", left.path, right.path),
                "Уникальное совпадение SHA-256 содержимого; не similarity rename".into(),
                Some(left.path.clone()),
                Some(right.path.clone()),
                None,
            ));
        }
    }
    let target_all: HashMap<_, _> = target.iter().map(|fact| (&fact.path, fact)).collect();
    for path in a.keys().filter(|path| !mapping.contains_key(*path)) {
        let entry = target_all.get(path).copied();
        let ambiguous = a[path].content_hash.as_deref().is_some_and(|hash| {
            hashes_b.contains_key(hash)
                && (frequency_a.get(hash) != Some(&1) || frequency_b.get(hash) != Some(&1))
        });
        let status = if entry.is_some_and(|f| f.status != "absent") {
            "not-in-target-coverage"
        } else if ambiguous {
            "ambiguous"
        } else {
            "removed"
        };
        items.push(change("file",status,path.clone(),entry.and_then(|f|f.reason.clone()).unwrap_or_else(||"Нет надёжного соответствия в target; rename+edit может выглядеть как add/remove".into()),Some(path.clone()),None,None));
    }
    for path in b
        .keys()
        .filter(|path| !a.contains_key(*path) && !moved_targets.contains(*path))
    {
        items.push(change(
            "file",
            if b[path].content_hash.as_deref().is_some_and(|hash| {
                hashes_a.contains_key(hash)
                    && (frequency_a.get(hash) != Some(&1) || frequency_b.get(hash) != Some(&1))
            }) {
                "ambiguous"
            } else {
                "added"
            },
            path.clone(),
            "Новый включённый путь; неоднозначные одинаковые файлы не объединяются".into(),
            None,
            Some(path.clone()),
            None,
        ));
    }
    (items, mapping)
}
fn import_delta(a: &FileFact, b: &FileFact) -> String {
    if a.parser_version.is_none() || b.parser_version.is_none() {
        return "Изменились bytes/mode конфигурации или ресурса; AST-факты импортов не собирались"
            .into();
    }
    let left: BTreeSet<_> = a.imports.iter().map(|i| (&i.specifier, &i.kind)).collect();
    let right: BTreeSet<_> = b.imports.iter().map(|i| (&i.specifier, &i.kind)).collect();
    let added: Vec<_> = right
        .difference(&left)
        .map(|(s, k)| format!("+{s} ({k})"))
        .collect();
    let removed: Vec<_> = left
        .difference(&right)
        .map(|(s, k)| format!("-{s} ({k})"))
        .collect();
    format!(
        "Изменились bytes/mode · AST import facts +{} / -{}: {}. Строки не являются идентичностью импорта",
        added.len(),
        removed.len(),
        added
            .iter()
            .chain(&removed)
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    )
}
fn edge_changes(
    base: &AnalysisResult,
    target: &AnalysisResult,
    base_files: &[FileFact],
    target_files: &[FileFact],
    mapping: &HashMap<String, String>,
    compatible: bool,
    items: &mut Vec<Change>,
) {
    type Key = (String, String, String);
    fn grouped<'a>(
        edges: &'a [GraphEdge],
        mapping: Option<&HashMap<String, String>>,
    ) -> BTreeMap<Key, Vec<&'a GraphEdge>> {
        let mut output = BTreeMap::new();
        for edge in edges {
            let remap = |id: &String| mapping.and_then(|m| m.get(id)).unwrap_or(id).clone();
            output
                .entry((remap(&edge.source), remap(&edge.target), edge.kind.clone()))
                .or_insert_with(Vec::new)
                .push(edge);
        }
        output
    }
    let a = grouped(&base.edges, Some(mapping));
    let b = grouped(&target.edges, None);
    let base_facts: HashMap<_, _> = base_files.iter().map(|f| (f.path.as_str(), f)).collect();
    let target_facts: HashMap<_, _> = target_files.iter().map(|f| (f.path.as_str(), f)).collect();
    for k in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
        let left = a.get(k);
        let right = b.get(k);
        let old = left.map_or(0, Vec::len);
        let new = right.map_or(0, Vec::len);
        if old == new {
            continue;
        }
        let edge = left.or(right).unwrap()[0];
        let mut status = if old == 0 {
            "resolved-added"
        } else if new == 0 {
            "no-longer-confirmed"
        } else {
            "count-changed"
        };
        if !compatible {
            status = "observed-only";
        } else if new == 0
            && let Some(fact) = base_facts
                .get(edge.source.as_str())
                .filter(|f| f.parse_error == Some(false))
        {
            let requests: Vec<_> = fact
                .imports
                .iter()
                .filter(|import| {
                    import.line == edge.line && (edge.kind == "package" || edge.kind == import.kind)
                })
                .collect();
            if requests.len() == 1
                && let Some(target_fact) = target_facts.get(k.0.as_str()).filter(|f| {
                    f.status == "included"
                        && f.parser_version.is_some()
                        && f.parse_error == Some(false)
                })
                && !target_fact.imports.iter().any(|import| {
                    import.specifier == requests[0].specifier && import.kind == requests[0].kind
                })
            {
                status = "removed-import";
            }
        }
        items.push(change("edge",status,format!("{} → {} ({})",k.0,k.1,k.2),format!("Количество импортов {old} → {new}. Строка/id не являются идентичностью связи; сохранённый источник {}:{}",edge.source,edge.line),left.map(|edges|edges[0].source.clone()),right.map(|edges|edges[0].source.clone()),Some((old,new))));
    }
}
