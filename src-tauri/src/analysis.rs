use ignore::WalkBuilder;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    path::Path,
};
use tree_sitter::{Language, Node, Parser};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub name: String,
    pub root_path: String,
    pub file_count: usize,
    pub source_file_count: usize,
    pub module_count: usize,
    pub dependency_count: usize,
    pub external_package_count: usize,
    pub cycle_count: usize,
    pub parse_error_count: usize,
    pub unresolved_count: usize,
    pub source_lines: usize,
    pub languages: Vec<LanguageCount>,
    pub packages: Vec<PackageInfo>,
    pub cycles: Vec<Vec<String>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageCount {
    pub name: String,
    pub count: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub dependency_type: String,
    pub imported_by: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub path: String,
    pub kind: String,
    pub language: Option<String>,
    pub lines: usize,
    pub incoming: usize,
    pub outgoing: usize,
    pub declared: Option<bool>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub line: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub summary: ProjectSummary,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone)]
struct SourceFile {
    relative: String,
    language: String,
    lines: usize,
    text: String,
    parse_error: bool,
}

struct ImportDecl {
    specifier: String,
    kind: &'static str,
    line: usize,
}

pub fn analyze_project(root: &str) -> Result<AnalysisResult, String> {
    let root =
        fs::canonicalize(root).map_err(|e| format!("Не удалось открыть каталог проекта: {e}"))?;
    if !root.is_dir() {
        return Err("Выбранный путь не является каталогом".into());
    }
    let project_name = root
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("Проект")
        .to_owned();
    let mut files = Vec::new();
    let mut all_file_paths = HashMap::<String, String>::new();
    let mut total_files = 0usize;
    let excluded = [
        ".git",
        "node_modules",
        "target",
        "dist",
        "build",
        "coverage",
        ".next",
        ".turbo",
        "vendor",
        "out",
    ];
    let walker = WalkBuilder::new(&root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(move |entry| {
            !entry.file_type().is_some_and(|t| t.is_dir())
                || !excluded.contains(&entry.file_name().to_string_lossy().as_ref())
        })
        .build();
    for entry in walker {
        let entry = match entry {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        total_files += 1;
        let Ok(real_path) = fs::canonicalize(entry.path()) else {
            continue;
        };
        if !real_path.starts_with(&root) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        all_file_paths.insert(normalize(&relative), relative.clone());
        let Some(language) = language_for(entry.path()) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let lines = text.lines().count();
        let parse_error = parse_imports(&text, language)
            .map(|(_, error)| error)
            .unwrap_or(true);
        files.push(SourceFile {
            relative,
            language: language.label().to_string(),
            lines,
            text,
            parse_error,
        });
    }
    files.sort_by(|a, b| a.relative.cmp(&b.relative));

    let (declared_packages, package_versions) = read_package_manifest(&root);
    let mut file_index = HashMap::new();
    let mut source_imports: Vec<(usize, ImportDecl)> = Vec::new();
    let mut parse_errors = 0usize;
    let mut language_counts = BTreeMap::<String, usize>::new();
    let mut module_dirs = BTreeSet::new();
    for (index, file) in files.iter().enumerate() {
        file_index.insert(normalize(&file.relative), index);
        *language_counts.entry(file.language.clone()).or_default() += 1;
        if let Some(parent) = Path::new(&file.relative).parent() {
            module_dirs.insert(parent.to_string_lossy().to_string());
        }
        if file.parse_error {
            parse_errors += 1;
        }
        if let Ok((imports, _)) =
            parse_imports(&file.text, language_by_label(&file.language).unwrap())
        {
            for import in imports {
                source_imports.push((index, import));
            }
        }
    }

    let mut nodes: Vec<GraphNode> = files
        .iter()
        .map(|file| GraphNode {
            id: file.relative.clone(),
            label: Path::new(&file.relative)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&file.relative)
                .to_string(),
            path: file.relative.clone(),
            kind: "file".into(),
            language: Some(file.language.clone()),
            lines: file.lines,
            incoming: 0,
            outgoing: 0,
            declared: None,
        })
        .collect();
    let mut edges = Vec::new();
    let mut extra_file_nodes = HashMap::<String, usize>::new();
    let mut unresolved = 0usize;
    let mut imported_packages = BTreeMap::<String, usize>::new();
    let mut package_imports = Vec::<(usize, String, usize)>::new();
    for (source_index, import) in source_imports {
        let source = &files[source_index];
        if import.specifier.starts_with('.') || import.specifier.starts_with('/') {
            match resolve_local(
                &root,
                source,
                &import.specifier,
                &file_index,
                &all_file_paths,
            ) {
                Some(LocalTarget::Source(target_index)) => {
                    let target = &files[target_index];
                    let id = format!("{}:{}:{}", source.relative, import.line, target.relative);
                    nodes[source_index].outgoing += 1;
                    nodes[target_index].incoming += 1;
                    edges.push(GraphEdge {
                        id,
                        source: source.relative.clone(),
                        target: target.relative.clone(),
                        kind: import.kind.into(),
                        line: import.line,
                    });
                }
                Some(LocalTarget::Asset(path)) => {
                    let node_index = *extra_file_nodes.entry(path.clone()).or_insert_with(|| {
                        let index = nodes.len();
                        nodes.push(GraphNode {
                            id: path.clone(),
                            label: Path::new(&path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(&path)
                                .to_string(),
                            path: path.clone(),
                            kind: "file".into(),
                            language: None,
                            lines: 0,
                            incoming: 0,
                            outgoing: 0,
                            declared: None,
                        });
                        index
                    });
                    nodes[source_index].outgoing += 1;
                    nodes[node_index].incoming += 1;
                    edges.push(GraphEdge {
                        id: format!("{}:{}:{}", source.relative, import.line, path),
                        source: source.relative.clone(),
                        target: path,
                        kind: import.kind.into(),
                        line: import.line,
                    });
                }
                None => {
                    unresolved += 1;
                }
            }
        } else if !is_builtin(&import.specifier) {
            if import.specifier.starts_with("@/")
                || import.specifier.starts_with("~/")
                || import.specifier.starts_with("#/")
            {
                unresolved += 1;
                continue;
            }
            let package = package_name(&import.specifier);
            if package.is_empty() {
                continue;
            }
            *imported_packages.entry(package.clone()).or_default() += 1;
            package_imports.push((source_index, package, import.line));
        }
    }
    let mut packages = Vec::new();
    for (package, imported_by) in imported_packages {
        let declared = declared_packages.contains(&package);
        let version = package_versions
            .get(&package)
            .cloned()
            .unwrap_or_else(|| "не объявлен".into());
        let id = format!("package:{package}");
        nodes.push(GraphNode {
            id: id.clone(),
            label: package.clone(),
            path: package.clone(),
            kind: "package".into(),
            language: None,
            lines: 0,
            incoming: imported_by,
            outgoing: 0,
            declared: Some(declared),
        });
        packages.push(PackageInfo {
            name: package.clone(),
            version,
            dependency_type: if declared {
                "объявлен"
            } else {
                "не объявлен"
            }
            .into(),
            imported_by,
        });
    }
    for (source_index, package, line) in package_imports {
        let target = format!("package:{package}");
        nodes[source_index].outgoing += 1;
        edges.push(GraphEdge {
            id: format!(
                "package-edge:{}:{line}:{package}",
                files[source_index].relative
            ),
            source: files[source_index].relative.clone(),
            target,
            kind: "package".into(),
            line,
        });
    }
    let file_count = files.len();
    let cycles = find_cycles(&files, &edges);
    let summary = ProjectSummary {
        name: project_name,
        root_path: display_root(&root),
        file_count: total_files,
        source_file_count: file_count,
        module_count: module_dirs.len(),
        dependency_count: edges.iter().filter(|e| e.kind != "package").count(),
        external_package_count: declared_packages.len(),
        cycle_count: cycles.len(),
        parse_error_count: parse_errors,
        unresolved_count: unresolved,
        source_lines: files.iter().map(|f| f.lines).sum(),
        languages: language_counts
            .into_iter()
            .map(|(name, count)| LanguageCount { name, count })
            .collect(),
        packages,
        cycles,
    };
    Ok(AnalysisResult {
        summary,
        nodes,
        edges,
    })
}

#[derive(Clone, Copy)]
enum SourceLanguage {
    JavaScript,
    TypeScript,
    Tsx,
}

impl SourceLanguage {
    fn label(self) -> &'static str {
        match self {
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Tsx => "TSX",
        }
    }
    fn grammar(self) -> Language {
        match self {
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }
}

fn language_for(path: &Path) -> Option<SourceLanguage> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "js" | "mjs" | "cjs" | "jsx" => Some(SourceLanguage::JavaScript),
        "ts" | "mts" | "cts" => Some(SourceLanguage::TypeScript),
        "tsx" => Some(SourceLanguage::Tsx),
        _ => None,
    }
}

fn language_by_label(label: &str) -> Option<SourceLanguage> {
    match label {
        "JavaScript" => Some(SourceLanguage::JavaScript),
        "TypeScript" => Some(SourceLanguage::TypeScript),
        "TSX" => Some(SourceLanguage::Tsx),
        _ => None,
    }
}

fn parse_imports(
    source: &str,
    language: SourceLanguage,
) -> Result<(Vec<ImportDecl>, bool), String> {
    let mut parser = Parser::new();
    parser
        .set_language(&language.grammar())
        .map_err(|e| e.to_string())?;
    let tree = parser
        .parse(source, None)
        .ok_or("Не удалось разобрать исходный файл")?;
    let mut imports = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "import_statement" => {
                if let Some(source_node) = node.child_by_field_name("source") {
                    let text = node.utf8_text(source.as_bytes()).unwrap_or("");
                    if let Some(specifier) = string_value(source_node, source) {
                        imports.push(ImportDecl {
                            specifier,
                            kind: if text.trim_start().starts_with("import type") {
                                "type_only_import"
                            } else {
                                "import"
                            },
                            line: node.start_position().row + 1,
                        });
                    }
                }
            }
            "export_statement" => {
                if let Some(source_node) = node.child_by_field_name("source")
                    && let Some(specifier) = string_value(source_node, source)
                {
                    imports.push(ImportDecl {
                        specifier,
                        kind: "reexport",
                        line: node.start_position().row + 1,
                    });
                }
            }
            "call_expression" => {
                let function = node.child_by_field_name("function");
                let arguments = node.child_by_field_name("arguments");
                if let (Some(function), Some(arguments)) = (function, arguments) {
                    let function_text = function.utf8_text(source.as_bytes()).unwrap_or("");
                    let kind = if function_text == "import" {
                        Some("dynamic_import")
                    } else if function_text == "require" {
                        Some("commonjs_import")
                    } else {
                        None
                    };
                    if let (Some(kind), Some(argument)) = (kind, arguments.named_child(0))
                        && let Some(specifier) = string_value(argument, source)
                    {
                        imports.push(ImportDecl {
                            specifier,
                            kind,
                            line: node.start_position().row + 1,
                        });
                    }
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        let children = node.named_children(&mut cursor);
        for child in children {
            stack.push(child);
        }
    }
    imports.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.specifier.cmp(&b.specifier))
    });
    Ok((imports, tree.root_node().has_error()))
}

fn string_value(node: Node, source: &str) -> Option<String> {
    let raw = node.utf8_text(source.as_bytes()).ok()?;
    if raw.len() < 2 {
        return None;
    }
    let body = &raw[1..raw.len() - 1];
    if body.contains("${") {
        return None;
    }
    Some(
        body.replace("\\'", "'")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"),
    )
}

enum LocalTarget {
    Source(usize),
    Asset(String),
}

fn resolve_local(
    root: &Path,
    source: &SourceFile,
    specifier: &str,
    index: &HashMap<String, usize>,
    all_paths: &HashMap<String, String>,
) -> Option<LocalTarget> {
    let parent = Path::new(&source.relative)
        .parent()
        .unwrap_or(Path::new(""));
    let clean_specifier = specifier.split(['?', '#']).next().unwrap_or(specifier);
    let base = if clean_specifier.starts_with('/') {
        root.join(clean_specifier.trim_start_matches('/'))
    } else {
        root.join(parent).join(clean_specifier)
    };
    let mut candidates = vec![base.clone()];
    if base.extension().is_none() {
        for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs"] {
            candidates.push(base.with_extension(ext));
        }
    }
    for ext in ["ts", "tsx", "js", "jsx", "mjs", "cjs"] {
        candidates.push(base.join(format!("index.{ext}")));
    }
    for candidate in candidates {
        let relative = candidate
            .strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(found) = index.get(&normalize(&relative)) {
            return Some(LocalTarget::Source(*found));
        }
        if let Some(found) = all_paths.get(&normalize(&relative)) {
            return Some(LocalTarget::Asset(found.clone()));
        }
    }
    None
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}
fn display_root(root: &Path) -> String {
    let raw = root.to_string_lossy();
    raw.strip_prefix("\\\\?\\").unwrap_or(&raw).to_string()
}
fn package_name(specifier: &str) -> String {
    if specifier.starts_with('@') {
        specifier.split('/').take(2).collect::<Vec<_>>().join("/")
    } else {
        specifier.split('/').next().unwrap_or("").to_string()
    }
}
fn is_builtin(name: &str) -> bool {
    name.starts_with("node:")
        || [
            "assert",
            "buffer",
            "child_process",
            "crypto",
            "events",
            "fs",
            "http",
            "https",
            "module",
            "os",
            "path",
            "process",
            "stream",
            "url",
            "util",
            "zlib",
        ]
        .contains(&name)
}

fn read_package_manifest(root: &Path) -> (HashSet<String>, HashMap<String, String>) {
    let mut declared = HashSet::new();
    let mut versions = HashMap::new();
    let Ok(raw) = fs::read_to_string(root.join("package.json")) else {
        return (declared, versions);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return (declared, versions);
    };
    for group in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(entries) = value.get(group).and_then(|v| v.as_object()) {
            for (name, version) in entries {
                declared.insert(name.clone());
                versions.insert(
                    name.clone(),
                    version.as_str().unwrap_or("версия неизвестна").to_string(),
                );
            }
        }
    }
    (declared, versions)
}

fn find_cycles(files: &[SourceFile], edges: &[GraphEdge]) -> Vec<Vec<String>> {
    let file_ids: HashSet<&str> = files.iter().map(|f| f.relative.as_str()).collect();
    let mut adjacency = HashMap::<String, Vec<String>>::new();
    let mut reverse = HashMap::<String, Vec<String>>::new();
    for edge in edges
        .iter()
        .filter(|e| file_ids.contains(e.source.as_str()) && file_ids.contains(e.target.as_str()))
    {
        adjacency
            .entry(edge.source.clone())
            .or_default()
            .push(edge.target.clone());
        reverse
            .entry(edge.target.clone())
            .or_default()
            .push(edge.source.clone());
    }
    // Iterative Kosaraju avoids recursion depth failures on long import chains.
    let mut visited = HashSet::<String>::new();
    let mut finish_order = Vec::with_capacity(files.len());
    for file in files {
        if visited.contains(&file.relative) {
            continue;
        }
        let mut stack = vec![(file.relative.clone(), false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                finish_order.push(node);
                continue;
            }
            if !visited.insert(node.clone()) {
                continue;
            }
            stack.push((node.clone(), true));
            if let Some(neighbors) = adjacency.get(&node) {
                for neighbor in neighbors.iter().rev() {
                    if !visited.contains(neighbor) {
                        stack.push((neighbor.clone(), false));
                    }
                }
            }
        }
    }
    visited.clear();
    let mut result = Vec::new();
    for node in finish_order.into_iter().rev() {
        if !visited.insert(node.clone()) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![node.clone()];
        while let Some(current) = stack.pop() {
            component.push(current.clone());
            if let Some(neighbors) = reverse.get(&current) {
                for neighbor in neighbors {
                    if visited.insert(neighbor.clone()) {
                        stack.push(neighbor.clone());
                    }
                }
            }
        }
        let self_loop = component.len() == 1
            && adjacency
                .get(&component[0])
                .is_some_and(|neighbors| neighbors.contains(&component[0]));
        if component.len() > 1 || self_loop {
            component.sort();
            result.push(component);
        }
    }
    result.sort_by(|a, b| a.first().cmp(&b.first()));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn parses_import_reexport_and_dynamic_import() {
        let source = "import type { User } from './types';\nexport { helper } from './util';\nconst lazy = import('./lazy');";
        let (imports, error) = parse_imports(source, SourceLanguage::TypeScript).unwrap();
        assert!(!error);
        assert_eq!(
            imports
                .iter()
                .map(|i| (i.specifier.as_str(), i.kind))
                .collect::<Vec<_>>(),
            vec![
                ("./types", "type_only_import"),
                ("./util", "reexport"),
                ("./lazy", "dynamic_import")
            ]
        );
    }

    #[test]
    fn scans_real_files_resolves_edges_and_finds_cycles() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        let mut a = fs::File::create(root.path().join("src/a.ts")).unwrap();
        a.write_all(b"import { b } from './b';\nimport './style.css';\nexport const a = b;")
            .unwrap();
        let mut b = fs::File::create(root.path().join("src/b.ts")).unwrap();
        b.write_all(b"import { a } from './a';\nexport const b = a;")
            .unwrap();
        fs::write(root.path().join("src/style.css"), ".x { color: red; }").unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"react":"^19.0.0"}}"#,
        )
        .unwrap();
        let result = analyze_project(root.path().to_str().unwrap()).unwrap();
        assert_eq!(result.summary.source_file_count, 2);
        assert_eq!(result.summary.dependency_count, 3);
        assert_eq!(result.summary.unresolved_count, 0);
        assert!(result.nodes.iter().any(|node| node.path == "src/style.css"));
        assert_eq!(result.summary.cycle_count, 1);
        assert_eq!(result.summary.module_count, 1);
    }

    #[test]
    fn cycle_detection_handles_long_chains_without_call_stack_recursion() {
        let files: Vec<SourceFile> = (0..10_000)
            .map(|index| SourceFile {
                relative: format!("src/{index}.ts"),
                language: "TypeScript".into(),
                lines: 1,
                text: String::new(),
                parse_error: false,
            })
            .collect();
        let mut edges: Vec<GraphEdge> = files
            .windows(2)
            .map(|pair| GraphEdge {
                id: format!("{}->{}", pair[0].relative, pair[1].relative),
                source: pair[0].relative.clone(),
                target: pair[1].relative.clone(),
                kind: "import".into(),
                line: 1,
            })
            .collect();
        edges.push(GraphEdge {
            id: "last->first".into(),
            source: files.last().unwrap().relative.clone(),
            target: files[0].relative.clone(),
            kind: "import".into(),
            line: 1,
        });
        let cycles = find_cycles(&files, &edges);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), files.len());
    }

    #[test]
    fn scans_this_git_checkout_and_resolves_imported_stylesheet() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let result = analyze_project(root.to_str().unwrap()).unwrap();
        assert!(result.summary.file_count > result.summary.source_file_count);
        assert!(result.summary.source_file_count >= 4);
        assert_eq!(result.summary.unresolved_count, 0);
        assert!(
            result
                .nodes
                .iter()
                .any(|node| node.path == "src/styles.css")
        );
        assert!(!result.summary.root_path.starts_with("\\\\?\\"));
    }
}
