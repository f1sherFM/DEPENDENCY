use crate::js_ts::parse_imports;
use crate::language::{SourceLanguage, language_for};
use crate::parse_facts::ImportDecl;
use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
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
    #[serde(default)]
    pub ambiguous_count: usize,
    pub source_lines: usize,
    pub languages: Vec<LanguageCount>,
    pub packages: Vec<PackageInfo>,
    pub cycles: Vec<Vec<String>>,
    #[serde(default)]
    pub resolution_issues: Vec<ResolutionIssue>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionIssue {
    pub source: String,
    pub specifier: String,
    pub line: usize,
    pub status: String,
    pub reason: String,
    pub attempted_candidates: Vec<String>,
    pub hint: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageCount {
    pub name: String,
    pub count: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub dependency_type: String,
    pub imported_by: usize,
}

#[derive(Clone, Serialize, Deserialize)]
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub line: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    #[serde(default)]
    pub basis: Option<crate::snapshot::Basis>,
    #[serde(default)]
    pub analysis_profile: Option<crate::language::AnalysisProfile>,
    #[serde(default)]
    pub diagnostics: Vec<crate::diagnostics::Diagnostic>,
    #[serde(default)]
    pub rules_run: Option<crate::diagnostics::RulesRun>,
    pub summary: ProjectSummary,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone)]
struct SourceFile {
    relative: String,
    language: String,
    lines: usize,
    parse_error: bool,
    imports: Vec<ImportDecl>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedImport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facts: Option<crate::parse_facts::Fact>,
    pub specifier: String,
    pub kind: String,
    pub line: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedFileCache {
    pub relative_path: String,
    pub content_hash: String,
    pub analyzer_version: String,
    pub language: String,
    pub lines: usize,
    pub parse_error: bool,
    pub imports: Vec<CachedImport>,
}

pub struct AnalysisOutput {
    pub result: AnalysisResult,
    pub file_cache: Vec<ParsedFileCache>,
    pub parsed_file_count: usize,
    pub reused_file_count: usize,
}

pub fn analyze_project(
    root: &str,
    cached_files: Vec<ParsedFileCache>,
) -> Result<AnalysisOutput, String> {
    analyze_input(
        root,
        cached_files,
        false,
        &std::sync::atomic::AtomicBool::new(false),
    )
}

pub fn analyze_input(
    root: &str,
    cached_files: Vec<ParsedFileCache>,
    isolated: bool,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<AnalysisOutput, String> {
    let root =
        fs::canonicalize(root).map_err(|e| format!("Не удалось открыть каталог проекта: {e}"))?;
    if !root.is_dir() {
        return Err("Выбранный путь не является каталогом".into());
    }
    let settings = crate::diagnostics::Settings::load(&root)?;
    let mut scan_issues = Vec::<(String, String)>::new();
    let project_name = root
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("Проект")
        .to_owned();
    let mut files = Vec::new();
    let mut file_cache = Vec::new();
    let cache_by_path: HashMap<String, ParsedFileCache> = cached_files
        .into_iter()
        .map(|item| (normalize(&item.relative_path), item))
        .collect();
    let mut parsed_file_count = 0;
    let mut reused_file_count = 0;
    let mut all_file_paths = HashMap::<String, String>::new();
    let mut total_files = 0usize;
    let excluded = [
        ".venv",
        "venv",
        "__pycache__",
        ".tox",
        ".mypy_cache",
        ".pytest_cache",
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
        .git_ignore(!isolated)
        .git_global(!isolated)
        .git_exclude(!isolated)
        .ignore(!isolated)
        .filter_entry(move |entry| {
            !entry.file_type().is_some_and(|t| t.is_dir())
                || !excluded.contains(&entry.file_name().to_string_lossy().as_ref())
        })
        .build();
    for entry in walker {
        crate::snapshot::check_cancel(cancel)?;
        let entry = match entry {
            Ok(value) => value,
            Err(error) => {
                scan_issues.push(("Индексирование".into(), error.to_string()));
                continue;
            }
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        total_files += 1;
        let Ok(real_path) = fs::canonicalize(entry.path()) else {
            scan_issues.push((
                entry.path().display().to_string(),
                "Не удалось определить путь файла".into(),
            ));
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
        if settings.excludes_source(&relative) {
            continue;
        }
        all_file_paths.insert(normalize(&relative), relative.clone());
        let Some(language) = language_for(entry.path()) else {
            continue;
        };
        let Ok(bytes) = fs::read(entry.path()) else {
            scan_issues.push((relative.clone(), "Не удалось прочитать исходник".into()));
            continue;
        };
        let content_hash = format!("{:x}", Sha256::digest(&bytes));
        let Ok(text) = String::from_utf8(bytes) else {
            scan_issues.push((relative.clone(), "Исходник не в UTF-8".into()));
            continue;
        };
        let lines = text.lines().count();
        let cached = cache_by_path.get(&normalize(&relative)).filter(|item| {
            item.content_hash == content_hash && item.analyzer_version == language.parser_version()
        });
        let (imports, parse_error) = if let Some(item) = cached {
            reused_file_count += 1;
            (
                item.imports
                    .iter()
                    .map(|import| ImportDecl {
                        specifier: import.specifier.clone(),
                        kind: import.kind.clone(),
                        line: import.line,
                        facts: import.facts.clone(),
                    })
                    .collect(),
                item.parse_error,
            )
        } else {
            parsed_file_count += 1;
            let (imports, error) = match language {
                SourceLanguage::Rust | SourceLanguage::Python => {
                    crate::native_languages::parse(&text, language)
                }
                _ => parse_imports(&text, language),
            }
            .unwrap_or_else(|_| (Vec::new(), true));
            (imports, error)
        };
        file_cache.push(ParsedFileCache {
            relative_path: relative.clone(),
            content_hash,
            analyzer_version: language.parser_version().into(),
            language: language.label().into(),
            lines,
            parse_error,
            imports: imports
                .iter()
                .map(|import| CachedImport {
                    specifier: import.specifier.clone(),
                    kind: import.kind.clone(),
                    line: import.line,
                    facts: import.facts.clone(),
                })
                .collect(),
        });
        files.push(SourceFile {
            relative,
            language: language.label().to_string(),
            lines,
            parse_error,
            imports,
        });
    }
    files.sort_by(|a, b| a.relative.cmp(&b.relative));

    let (declared_packages, package_versions) = read_package_manifests(&root, &all_file_paths);
    let workspace_packages = discover_workspace_packages(&root, &all_file_paths);
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
        for import in &file.imports {
            source_imports.push((index, import.clone()));
        }
    }

    let native = crate::native_resolver::NativeResolver::new(
        &root,
        &files
            .iter()
            .map(|f| (f.relative.clone(), f.imports.clone()))
            .collect::<Vec<_>>(),
        all_file_paths.values().cloned().collect(),
        &settings.config.python.source_roots,
        cancel,
    )?;
    let settings_roots = native.python_roots();
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
    for (id, package) in &native.declarations {
        nodes.push(GraphNode {
            id: id.clone(),
            label: package.name.clone(),
            path: id.clone(),
            kind: "package".into(),
            language: Some(
                if id.starts_with("package:pypi:") {
                    "Python"
                } else {
                    "Rust"
                }
                .into(),
            ),
            lines: 0,
            incoming: 0,
            outgoing: 0,
            declared: Some(true),
        });
    }
    let mut edges = Vec::new();
    let mut extra_file_nodes = HashMap::<String, usize>::new();
    let mut resolution_issues: Vec<ResolutionIssue> = native.manifest_issues.iter().map(|(source,reason)|ResolutionIssue{
        source:source.clone(),specifier:"<manifest>".into(),line:1,status:"unsupported".into(),reason:reason.clone(),attempted_candidates:vec![],
        hint:"Манифест прочитан, но часть зависимостей не входит в поддерживаемую модель или индекс. Это ограничение покрытия, не синтаксическая ошибка исходника.".into(),
    }).collect();
    let mut ambiguous_count = 0usize;
    let mut imported_packages = BTreeMap::<String, usize>::new();
    let mut package_imports = Vec::<(usize, String, usize)>::new();
    let mut resolver_pool = if isolated {
        crate::resolver::ResolverPool::with_isolation(&root, true)
    } else {
        crate::resolver::ResolverPool::new(&root)
    };
    'imports: for (source_index, import) in source_imports {
        crate::snapshot::check_cancel(cancel)?;
        let source = &files[source_index];
        if import.facts.is_some() {
            let resolved = native.resolve(&source.relative, &import);
            for (target, kind) in resolved.targets {
                if let Some(target_index) = file_index
                    .get(&normalize(&target))
                    .copied()
                    .or_else(|| nodes.iter().position(|n| n.id == target))
                {
                    nodes[source_index].outgoing += 1;
                    nodes[target_index].incoming += 1;
                    edges.push(GraphEdge {
                        id: format!(
                            "{}:{}:{target}:{kind}:{}",
                            source.relative,
                            import.line,
                            edges.len()
                        ),
                        source: source.relative.clone(),
                        target,
                        kind,
                        line: import.line,
                    });
                }
            }
            for issue in resolved.issues {
                if issue.status == "ambiguous" {
                    ambiguous_count += 1;
                }
                resolution_issues.push(issue);
            }
            continue;
        }
        if is_builtin(&import.specifier) {
            continue;
        }
        if is_url_specifier(&import.specifier) {
            resolution_issues.push(ResolutionIssue {
                source: source.relative.clone(), specifier: import.specifier.clone(), line: import.line,
                status: "external".into(), reason: "Это URL-импорт; он не указывает на локальный файл проекта.".into(),
                attempted_candidates: Vec::new(), hint: "Проверьте runtime/bundler, который загружает этот URL. DEPENDENCY не обращается к сети.".into(),
            });
            continue;
        }
        let containing_file = root.join(&source.relative);
        let mut resolution =
            resolver_pool.resolve(&containing_file, &import.specifier, &import.kind);
        if matches!(&resolution, Err(oxc_resolver::ResolveError::NotFound(_)))
            && !import.specifier.starts_with('#')
            && let Some(package) = package_name(&import.specifier)
            && let Some(members) = workspace_packages.get(&package)
        {
            if members.len() > 1 {
                ambiguous_count += 1;
                resolution_issues.push(ResolutionIssue {
                    source: source.relative.clone(), specifier: import.specifier.clone(), line: import.line,
                    status: "ambiguous".into(), reason: format!("В workspace найдены несколько пакетов с именем `{package}`."),
                    attempted_candidates: members.iter().map(|member| member.root.to_string_lossy().into_owned()).collect(),
                    hint: "Переименуйте один из workspace-пакетов или исправьте package.json/workspace pattern.".into(),
                });
                continue 'imports;
            }
            if let Some(member) = members.first() {
                let remainder = import
                    .specifier
                    .strip_prefix(&package)
                    .unwrap_or("")
                    .trim_start_matches('/');
                let request = if member.has_exports || remainder.is_empty() {
                    import.specifier.clone()
                } else {
                    format!("./{remainder}")
                };
                resolution = resolver_pool.resolve(&member.package_json, &request, &import.kind);
            }
        }
        match resolution {
            Ok(path) => {
                let resolved = fs::canonicalize(&path).unwrap_or(path);
                if let Some(package) = package_name_from_node_modules(&resolved) {
                    add_package_import(
                        source_index,
                        package,
                        import.line,
                        &mut imported_packages,
                        &mut package_imports,
                    );
                } else if resolved.starts_with(&root) {
                    let Some(relative) = resolved
                        .strip_prefix(&root)
                        .ok()
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                    else {
                        continue;
                    };
                    let key = normalize(&relative);
                    if let Some(&target_index) = file_index.get(&key) {
                        nodes[source_index].outgoing += 1;
                        nodes[target_index].incoming += 1;
                        edges.push(GraphEdge {
                            id: format!(
                                "{}:{}:{}",
                                source.relative, import.line, files[target_index].relative
                            ),
                            source: source.relative.clone(),
                            target: files[target_index].relative.clone(),
                            kind: import.kind.clone(),
                            line: import.line,
                        });
                    } else if let Some(path) = all_file_paths.get(&key) {
                        add_asset_edge(
                            source_index,
                            source,
                            &import,
                            path,
                            &mut nodes,
                            &mut edges,
                            &mut extra_file_nodes,
                        );
                    } else {
                        resolution_issues.push(ResolutionIssue {
                            source: source.relative.clone(), specifier: import.specifier.clone(), line: import.line,
                            status: "unresolved".into(), reason: format!("Resolver выбрал `{relative}`, но файл отсутствует в проиндексированном наборе."),
                            attempted_candidates: vec![relative], hint: "Цель могла быть исключена через .gitignore, находиться в пропускаемом каталоге или иметь неподдерживаемый тип.".into(),
                        });
                    }
                } else if import.specifier.starts_with('#') {
                    if let Some(package) = package_name_from_node_modules(&resolved) {
                        add_package_import(
                            source_index,
                            package,
                            import.line,
                            &mut imported_packages,
                            &mut package_imports,
                        );
                    } else {
                        resolution_issues.push(ResolutionIssue {
                            source: source.relative.clone(), specifier: import.specifier.clone(), line: import.line,
                            status: "external".into(), reason: format!("Внутренний imports mapping разрешился за пределы проекта: {}", resolved.display()),
                            attempted_candidates: vec![resolved.display().to_string()], hint: "Связь находится вне анализируемого root и потому не добавлена в локальный граф.".into(),
                        });
                    }
                } else if let Some(package) = package_name(&import.specifier) {
                    add_package_import(
                        source_index,
                        package,
                        import.line,
                        &mut imported_packages,
                        &mut package_imports,
                    );
                } else {
                    resolution_issues.push(ResolutionIssue {
                        source: source.relative.clone(), specifier: import.specifier.clone(), line: import.line,
                        status: "unresolved".into(), reason: format!("Resolver вышел за пределы проекта: {}", resolved.display()),
                        attempted_candidates: vec![resolved.display().to_string()], hint: "Проверьте alias и убедитесь, что импортируемый файл входит в исследуемый репозиторий.".into(),
                    });
                }
            }
            Err(error) => {
                let is_package_error = matches!(
                    &error,
                    oxc_resolver::ResolveError::PackagePathNotExported(..)
                        | oxc_resolver::ResolveError::InvalidPackageTarget(..)
                );
                let package_specifier = !import.specifier.starts_with('.')
                    && !import.specifier.starts_with('/')
                    && !import.specifier.starts_with('#')
                    && !import.specifier.starts_with("@/")
                    && !import.specifier.starts_with("~/");
                let is_local_package_error = is_local_package_error(&error, &root);
                if package_specifier
                    && !is_local_package_error
                    && !matches!(
                        &error,
                        oxc_resolver::ResolveError::MatchedAliasNotFound(..)
                            | oxc_resolver::ResolveError::TsconfigNotFound(_)
                            | oxc_resolver::ResolveError::Json(_)
                    )
                    && let Some(package) = package_name(&import.specifier)
                {
                    add_package_import(
                        source_index,
                        package,
                        import.line,
                        &mut imported_packages,
                        &mut package_imports,
                    );
                }
                let known_external_not_installed = package_specifier
                    && !is_package_error
                    && matches!(&error, oxc_resolver::ResolveError::NotFound(_));
                if !known_external_not_installed {
                    resolution_issues.push(ResolutionIssue {
                        source: source.relative.clone(),
                        specifier: import.specifier.clone(),
                        line: import.line,
                        status: "unresolved".into(),
                        reason: error.to_string(),
                        attempted_candidates: crate::resolver::attempted_candidates(&error),
                        hint: crate::resolver::diagnostic_hint(&error, &import.specifier),
                    });
                }
            }
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
    let cycles = find_cycles(
        &files,
        &edges
            .iter()
            .filter(|e| !matches!(e.kind.as_str(), "module_declaration" | "conditional_import"))
            .cloned()
            .collect::<Vec<_>>(),
    );
    let unresolved_count = resolution_issues
        .iter()
        .filter(|issue| issue.status == "unresolved")
        .count();
    let mut manifests: Vec<String> = all_file_paths
        .values()
        .filter(|path| {
            Path::new(path)
                .file_name()
                .is_some_and(|name| name == "package.json")
        })
        .cloned()
        .collect();
    manifests.sort();
    let excluded_paths = settings.config.analysis.exclude.clone();
    let (diagnostics, rules_run) = crate::diagnostics::evaluate(
        settings,
        crate::diagnostics::RuleInput {
            root: &root,
            nodes: &nodes,
            edges: &edges,
            cache: &file_cache,
            issues: &resolution_issues,
            scan_issues: &scan_issues,
            manifests: &manifests,
        },
    )?;
    let mut analysis_profile = crate::language::profile(
        &files
            .iter()
            .map(|f| f.language.as_str())
            .collect::<Vec<_>>(),
        &settings_roots,
    );
    analysis_profile.excluded_paths = excluded_paths;
    if !analysis_profile.excluded_paths.is_empty() {
        analysis_profile.limitations.push(format!("Файлы исключены из анализа по шаблонам: {}. Их зависимости и потребители не входят в граф.", analysis_profile.excluded_paths.join(", ")));
    }
    let mut native_counts = HashMap::<&str, usize>::new();
    for edge in &edges {
        if edge.kind == "package" {
            *native_counts.entry(edge.target.as_str()).or_default() += 1;
        }
    }
    packages.extend(native.declarations.iter().map(|(id, p)| {
        let mut p = p.clone();
        p.imported_by = *native_counts.get(id.as_str()).unwrap_or(&0);
        p
    }));
    let summary = ProjectSummary {
        name: project_name,
        root_path: display_root(&root),
        file_count: total_files,
        source_file_count: file_count,
        module_count: module_dirs.len(),
        dependency_count: edges
            .iter()
            .filter(|e| e.kind != "package" && e.kind != "module_declaration")
            .count(),
        external_package_count: nodes.iter().filter(|n| n.kind == "package").count(),
        cycle_count: cycles.len(),
        parse_error_count: parse_errors,
        unresolved_count,
        ambiguous_count,
        source_lines: files.iter().map(|f| f.lines).sum(),
        languages: language_counts
            .into_iter()
            .map(|(name, count)| LanguageCount { name, count })
            .collect(),
        packages,
        cycles,
        resolution_issues,
    };
    Ok(AnalysisOutput {
        result: AnalysisResult {
            basis: None,
            analysis_profile: Some(analysis_profile),
            diagnostics,
            rules_run: Some(rules_run),
            summary,
            nodes,
            edges,
        },
        file_cache,
        parsed_file_count,
        reused_file_count,
    })
}

fn add_asset_edge(
    source_index: usize,
    source: &SourceFile,
    import: &ImportDecl,
    path: &str,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    extra_file_nodes: &mut HashMap<String, usize>,
) {
    let node_index = *extra_file_nodes.entry(path.to_string()).or_insert_with(|| {
        let index = nodes.len();
        nodes.push(GraphNode {
            id: path.to_string(),
            label: Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path)
                .to_string(),
            path: path.to_string(),
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
        id: format!("{}:{}:{path}", source.relative, import.line),
        source: source.relative.clone(),
        target: path.to_string(),
        kind: import.kind.clone(),
        line: import.line,
    });
}

fn add_package_import(
    source_index: usize,
    package: String,
    line: usize,
    imported_packages: &mut BTreeMap<String, usize>,
    package_imports: &mut Vec<(usize, String, usize)>,
) {
    *imported_packages.entry(package.clone()).or_default() += 1;
    package_imports.push((source_index, package, line));
}

fn package_name_from_node_modules(path: &Path) -> Option<String> {
    let parts: Vec<String> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_string))
        .collect();
    for index in (0..parts.len()).rev() {
        if !parts[index].eq_ignore_ascii_case("node_modules") {
            continue;
        }
        let first = parts.get(index + 1)?;
        if first.starts_with('@') {
            let second = parts.get(index + 2)?;
            return Some(format!("{first}/{second}"));
        }
        if first != ".pnpm" {
            return Some(first.clone());
        }
    }
    None
}

fn is_local_package_error(error: &oxc_resolver::ResolveError, root: &Path) -> bool {
    let package_path = match error {
        oxc_resolver::ResolveError::PackagePathNotExported(_, path)
        | oxc_resolver::ResolveError::InvalidPackageConfig(path)
        | oxc_resolver::ResolveError::InvalidPackageConfigDefault(path)
        | oxc_resolver::ResolveError::InvalidPackageConfigDirectory(path) => Some(path.as_path()),
        oxc_resolver::ResolveError::InvalidPackageTarget(_, _, path) => Some(path.as_path()),
        _ => None,
    };
    package_path.is_some_and(|path| {
        path.starts_with(root) && package_name_from_node_modules(path).is_none()
    })
}

fn is_url_specifier(specifier: &str) -> bool {
    ["http://", "https://", "data:", "file:", "blob:"]
        .iter()
        .any(|prefix| specifier.starts_with(prefix))
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}
fn display_root(root: &Path) -> String {
    let raw = root.to_string_lossy();
    raw.strip_prefix("\\\\?\\").unwrap_or(&raw).to_string()
}
fn package_name(specifier: &str) -> Option<String> {
    if specifier.starts_with('@') {
        let mut parts = specifier.split('/');
        let scope = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        if scope.len() > 1 && !name.is_empty() {
            Some(format!("{scope}/{name}"))
        } else {
            None
        }
    } else {
        let name = specifier.split('/').next().unwrap_or_default();
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }
}
fn is_builtin(name: &str) -> bool {
    // These modules require the prefix: bare `test` can be an npm package.
    if matches!(
        name,
        "node:test" | "node:test/reporters" | "node:sea" | "node:sqlite"
    ) {
        return true;
    }
    let builtin = name.strip_prefix("node:").unwrap_or(name);
    oxc_resolver::NODEJS_BUILTINS.contains(&builtin)
}

fn read_package_manifests(
    root: &Path,
    all_paths: &HashMap<String, String>,
) -> (HashSet<String>, HashMap<String, String>) {
    let mut declared = HashSet::new();
    let mut versions = HashMap::new();
    for relative in all_paths.values().filter(|path| {
        Path::new(path)
            .file_name()
            .is_some_and(|name| name == "package.json")
    }) {
        let Ok(raw) = fs::read_to_string(root.join(relative)) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
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
                    versions.entry(name.clone()).or_insert_with(|| {
                        version.as_str().unwrap_or("версия неизвестна").to_string()
                    });
                }
            }
        }
    }
    (declared, versions)
}

#[derive(Clone)]
struct WorkspacePackage {
    root: PathBuf,
    package_json: PathBuf,
    has_exports: bool,
}

fn discover_workspace_packages(
    root: &Path,
    all_paths: &HashMap<String, String>,
) -> HashMap<String, Vec<WorkspacePackage>> {
    let Ok(root_manifest) = fs::read_to_string(root.join("package.json")) else {
        return HashMap::new();
    };
    let Ok(root_manifest) = serde_json::from_str::<serde_json::Value>(&root_manifest) else {
        return HashMap::new();
    };
    let workspace_values = root_manifest.get("workspaces").and_then(|value| {
        value.as_array().or_else(|| {
            value
                .get("packages")
                .and_then(|packages| packages.as_array())
        })
    });
    let Some(workspace_values) = workspace_values else {
        return HashMap::new();
    };
    let mut includes = GlobSetBuilder::new();
    let mut excludes = GlobSetBuilder::new();
    let mut has_include = false;
    for value in workspace_values
        .iter()
        .filter_map(serde_json::Value::as_str)
    {
        let pattern = value.trim_start_matches("./");
        let (builder, pattern) = if let Some(pattern) = pattern.strip_prefix('!') {
            (&mut excludes, pattern)
        } else {
            has_include = true;
            (&mut includes, pattern)
        };
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    if !has_include {
        return HashMap::new();
    }
    let Ok(includes) = includes.build() else {
        return HashMap::new();
    };
    let Ok(excludes) = excludes.build() else {
        return HashMap::new();
    };
    let mut packages = HashMap::<String, Vec<WorkspacePackage>>::new();
    for relative in all_paths.values().filter(|path| {
        Path::new(path)
            .file_name()
            .is_some_and(|name| name == "package.json")
    }) {
        let Some(package_root) = Path::new(relative).parent() else {
            continue;
        };
        let package_root = package_root.to_string_lossy().replace('\\', "/");
        if package_root.is_empty()
            || !includes.is_match(&package_root)
            || excludes.is_match(&package_root)
        {
            continue;
        }
        let package_json = root.join(relative);
        let Ok(raw) = fs::read_to_string(&package_json) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let Some(name) = manifest.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        packages
            .entry(name.to_string())
            .or_default()
            .push(WorkspacePackage {
                root: root.join(&package_root),
                package_json,
                has_exports: manifest.get("exports").is_some(),
            });
    }
    packages
}

fn find_cycles(files: &[SourceFile], edges: &[GraphEdge]) -> Vec<Vec<String>> {
    crate::diagnostics::cyclic_components(
        &files
            .iter()
            .map(|file| file.relative.clone())
            .collect::<Vec<_>>(),
        edges,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn prefix_only_node_builtins_do_not_become_npm_dependencies() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("main.mjs"), "import 'node:test'; import 'node:test/reporters'; import 'node:sqlite'; import 'node:sea'; import 'node:fs/promises';").unwrap();
        let output = analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
        assert!(output.result.summary.packages.is_empty());
        assert!(output.result.summary.resolution_issues.is_empty());
        assert!(!is_builtin("test"));
        assert!(!is_builtin("sqlite"));
        assert!(!is_builtin("node:unknown-module"));
    }

    #[test]
    fn analysis_exclusions_remove_fixture_graph_and_manifests_without_reparsing_sources() {
        let root = tempdir().unwrap();
        for (path, text) in [
            ("main.ts", "import './util';"),
            ("util.ts", "export const x = 1;"),
            ("fixtures/a.ts", "import './b';"),
            ("fixtures/b.ts", "import './a'; import 'demo-package';"),
            (
                "fixtures/package.json",
                r#"{"dependencies":{"demo-package":"1"}}"#,
            ),
        ] {
            let file = root.path().join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }
        let first = analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
        assert_eq!(first.result.summary.cycle_count, 1);
        fs::write(
            root.path().join("dependency.config.json"),
            r#"{"analysis":{"exclude":["fixtures/**"]}}"#,
        )
        .unwrap();
        let next = analyze_project(root.path().to_str().unwrap(), first.file_cache).unwrap();
        assert_eq!(next.parsed_file_count, 0);
        assert_eq!(next.reused_file_count, 2);
        assert_eq!(next.result.summary.cycle_count, 0);
        assert!(next.result.summary.packages.is_empty());
        assert!(
            next.result
                .nodes
                .iter()
                .all(|n| !n.path.starts_with("fixtures/"))
        );
        assert_ne!(first.result.analysis_profile, next.result.analysis_profile);
        assert_eq!(
            next.result.analysis_profile.unwrap().excluded_paths,
            vec!["fixtures/**"]
        );
        // Bad configuration must fail before any new snapshot can be published.
        fs::write(
            root.path().join("dependency.config.json"),
            r#"{"analysis":{"exclude":["["]}}"#,
        )
        .unwrap();
        assert!(analyze_project(root.path().to_str().unwrap(), next.file_cache).is_err());
    }

    #[test]
    fn parses_import_reexport_and_dynamic_import() {
        let source = "import type { User } from './types';\nexport { helper } from './util';\nconst lazy = import('./lazy');";
        let (imports, error) = parse_imports(source, SourceLanguage::TypeScript).unwrap();
        assert!(!error);
        assert_eq!(
            imports
                .iter()
                .map(|i| (i.specifier.as_str(), i.kind.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("./types", "type_only_import"),
                ("./util", "reexport"),
                ("./lazy", "dynamic_import")
            ]
        );
    }

    #[test]
    fn classifies_type_only_exports_and_inline_specifiers_without_confusing_mixed_imports() {
        let source = "export type { A } from './a';\nexport { type B } from './b';\nimport { type C } from './c';\nimport type /*comment*/ { D } from './d';\nimport { type E, value } from './e';\nimport Default, { type F } from './f';\nimport { type as value } from './g';";
        let (imports, error) = parse_imports(source, SourceLanguage::TypeScript).unwrap();
        assert!(!error);
        assert_eq!(
            imports
                .iter()
                .map(|item| item.kind.as_str())
                .collect::<Vec<_>>(),
            [
                "type_only_import",
                "type_only_import",
                "type_only_import",
                "type_only_import",
                "import",
                "import",
                "import"
            ]
        );
    }

    #[test]
    fn older_saved_results_deserialize_without_new_resolver_fields() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolver_aliases");
        let summary = analyze_project(root.to_str().unwrap(), Vec::new())
            .unwrap()
            .result
            .summary;
        let mut old = serde_json::to_value(summary).unwrap();
        let object = old.as_object_mut().unwrap();
        object.remove("resolutionIssues");
        object.remove("ambiguousCount");
        let restored: ProjectSummary = serde_json::from_value(old).unwrap();
        assert_eq!(restored.resolution_issues.len(), 0);
        assert_eq!(restored.ambiguous_count, 0);
    }

    #[test]
    fn resolves_tsconfig_paths_base_url_and_js_extension_aliases() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolver_aliases");
        let result = analyze_project(root.to_str().unwrap(), Vec::new())
            .unwrap()
            .result;
        let main_targets: HashSet<&str> = result
            .edges
            .iter()
            .filter(|edge| edge.source == "src/main.ts")
            .map(|edge| edge.target.as_str())
            .collect();
        assert!(
            main_targets.contains("src/util.ts"),
            "@/* alias not resolved: {main_targets:?}"
        );
        assert!(
            main_targets.contains("packages/shared/src/index.ts"),
            "exact path alias not resolved: {main_targets:?}"
        );
        assert!(
            main_targets.contains("packages/models/src/model.ts"),
            "wildcard path alias not resolved: {main_targets:?}"
        );
        assert!(
            main_targets.contains("src/legacy.ts"),
            "TypeScript .js extension substitution failed: {main_targets:?}"
        );
        assert!(
            result.nodes.iter().any(|node| node.id == "package:react"),
            "bare package import should stay an external package node"
        );
        assert_eq!(
            result.summary.unresolved_count,
            0,
            "unexpected issues: {}",
            result
                .summary
                .resolution_issues
                .iter()
                .map(|issue| issue.reason.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    #[test]
    fn resolves_jsconfig_base_url_for_javascript_projects() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolver_jsconfig");
        let result = analyze_project(root.to_str().unwrap(), Vec::new())
            .unwrap()
            .result;
        assert!(
            result
                .edges
                .iter()
                .any(|edge| edge.source == "src/main.js" && edge.target == "src/app/item.js")
        );
        assert_eq!(result.summary.unresolved_count, 0);
    }

    #[test]
    fn follows_node_next_extension_and_package_conditions() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolver_nodenext");
        let result = analyze_project(root.to_str().unwrap(), Vec::new())
            .unwrap()
            .result;
        let edges: HashSet<(&str, &str)> = result
            .edges
            .iter()
            .map(|edge| (edge.source.as_str(), edge.target.as_str()))
            .collect();
        assert!(
            edges.contains(&("src/main.ts", "src/explicit.ts")),
            "explicit .js specifier should map to TS source"
        );
        assert!(
            edges.contains(&("src/main.ts", "src/esm.ts")),
            "ESM source should use import condition"
        );
        assert!(
            edges.contains(&("src/cjs.cts", "src/cjs.ts")),
            "CommonJS TypeScript source should use require condition"
        );
        assert!(
            !edges.contains(&("src/main.ts", "src/extensionless.ts")),
            "NodeNext ESM must not resolve extensionless imports"
        );
        assert_eq!(result.summary.unresolved_count, 1);
    }

    #[test]
    fn respects_package_exports_imports_conditions_and_blocked_subpaths() {
        let root = copy_fixture("resolver_exports");
        let main = root.path().join("src/main.ts");
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(main)
            .unwrap()
            .write_all(b"\nimport denied from 'resolver-exports-fixture/private';\n")
            .unwrap();
        let result = analyze_project(root.path().to_str().unwrap(), Vec::new())
            .unwrap()
            .result;
        let targets: HashSet<&str> = result
            .edges
            .iter()
            .filter(|edge| edge.source == "src/main.ts")
            .map(|edge| edge.target.as_str())
            .collect();
        assert!(
            targets.contains("src/feature.mjs"),
            "package exports subpath missing: {targets:?}"
        );
        assert!(
            targets.contains("src/internal/tool.mjs"),
            "package imports wildcard missing: {targets:?}"
        );
        assert!(
            targets.contains("src/index.cjs"),
            "require condition did not select CJS export: {targets:?}"
        );
        assert!(
            targets.contains("src/internal/tool.cjs"),
            "require condition did not select CJS imports mapping: {targets:?}"
        );
        assert!(
            targets.contains("src/typed.d.ts"),
            "type-only import did not select the types condition: {targets:?}"
        );
        assert_eq!(result.summary.unresolved_count, 1);
        let blocked = result
            .summary
            .resolution_issues
            .iter()
            .find(|issue| issue.specifier == "resolver-exports-fixture/private")
            .unwrap();
        assert!(blocked.reason.contains("exports"));
        assert!(
            !result
                .nodes
                .iter()
                .any(|node| node.id == "package:resolver-exports-fixture"),
            "self-reference should not be mislabeled as an external package"
        );
    }

    #[test]
    fn resolves_workspace_package_exports_without_installing_workspace_symlinks() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolver_workspace");
        let result = analyze_project(root.to_str().unwrap(), Vec::new())
            .unwrap()
            .result;
        assert!(
            result.edges.iter().any(|edge| edge.source == "src/main.ts"
                && edge.target == "packages/shared/src/feature.ts"),
            "workspace package edge missing: {:?}",
            result
                .edges
                .iter()
                .map(|edge| (&edge.source, &edge.target))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            result.summary.unresolved_count,
            0,
            "unexpected issues: {}",
            result
                .summary
                .resolution_issues
                .iter()
                .map(|issue| issue.reason.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    #[test]
    fn reports_duplicate_workspace_package_names_as_ambiguous() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolver_ambiguous");
        let result = analyze_project(root.to_str().unwrap(), Vec::new())
            .unwrap()
            .result;
        assert_eq!(result.summary.ambiguous_count, 1);
        assert_eq!(result.summary.resolution_issues[0].status, "ambiguous");
        assert_eq!(
            result.summary.resolution_issues[0]
                .attempted_candidates
                .len(),
            2
        );
        assert!(!result.edges.iter().any(|edge| edge.source == "src/main.ts"));
    }

    fn copy_fixture(name: &str) -> tempfile::TempDir {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let target = tempdir().unwrap();
        for entry in walkdir::WalkDir::new(&source)
            .into_iter()
            .filter_map(Result::ok)
        {
            let relative = entry.path().strip_prefix(&source).unwrap();
            let destination = target.path().join(relative);
            if entry.file_type().is_dir() {
                fs::create_dir_all(&destination).unwrap();
            } else {
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::copy(entry.path(), destination).unwrap();
            }
        }
        target
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
        let result = analyze_project(root.path().to_str().unwrap(), Vec::new())
            .unwrap()
            .result;
        assert_eq!(result.summary.source_file_count, 2);
        assert_eq!(result.summary.dependency_count, 3);
        assert_eq!(
            result.summary.unresolved_count,
            0,
            "{}",
            result
                .summary
                .resolution_issues
                .iter()
                .map(|issue| format!(
                    "{}:{} {} => {}",
                    issue.source, issue.line, issue.specifier, issue.reason
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(result.nodes.iter().any(|node| node.path == "src/style.css"));
        assert_eq!(result.summary.cycle_count, 1);
        assert_eq!(result.summary.module_count, 1);
    }

    #[test]
    fn reuses_unchanged_parses_and_keeps_last_snapshot_after_failed_write() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(
            root.path().join("src/a.ts"),
            "import './b';\nexport const a = 1;",
        )
        .unwrap();
        fs::write(root.path().join("src/b.ts"), "export const b = 2;").unwrap();
        let db = tempdir().unwrap();
        let state = crate::storage::initialize(db.path().join("dependency.sqlite3")).unwrap();

        let first = analyze_project(root.path().to_str().unwrap(), Vec::new()).unwrap();
        assert_eq!(first.parsed_file_count, 2);
        let first = crate::storage::save_snapshot(
            &state,
            first.result,
            first.file_cache,
            first.parsed_file_count,
            first.reused_file_count,
        )
        .unwrap();
        let project_id = first.project.id.clone();

        let cache = crate::storage::load_cache(&state, Some(&project_id)).unwrap();
        let second = analyze_project(root.path().to_str().unwrap(), cache).unwrap();
        assert_eq!(second.parsed_file_count, 0);
        assert_eq!(second.reused_file_count, 2);
        let second = crate::storage::save_snapshot(
            &state,
            second.result,
            second.file_cache,
            second.parsed_file_count,
            second.reused_file_count,
        )
        .unwrap();

        fs::write(
            root.path().join("src/a.ts"),
            "import './b';\nexport const a = 3;",
        )
        .unwrap();
        let cache = crate::storage::load_cache(&state, Some(&project_id)).unwrap();
        let third = analyze_project(root.path().to_str().unwrap(), cache).unwrap();
        assert_eq!(third.parsed_file_count, 1);
        assert_eq!(third.reused_file_count, 1);

        let connection = rusqlite::Connection::open(&state.database_path).unwrap();
        connection.execute_batch("CREATE TRIGGER reject_snapshot BEFORE INSERT ON snapshots BEGIN SELECT RAISE(ABORT, 'forced test failure'); END;").unwrap();
        assert!(
            crate::storage::save_snapshot(
                &state,
                third.result,
                third.file_cache,
                third.parsed_file_count,
                third.reused_file_count
            )
            .is_err()
        );
        let latest = crate::storage::load_project(&state, &project_id).unwrap();
        assert_eq!(latest.snapshot.id, second.snapshot.id);
        assert_eq!(
            crate::storage::list_snapshots(&state, &project_id)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn cycle_detection_handles_long_chains_without_call_stack_recursion() {
        let files: Vec<SourceFile> = (0..10_000)
            .map(|index| SourceFile {
                relative: format!("src/{index}.ts"),
                language: "TypeScript".into(),
                lines: 1,
                parse_error: false,
                imports: Vec::new(),
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
        let result = analyze_project(root.to_str().unwrap(), Vec::new())
            .unwrap()
            .result;
        assert!(result.summary.file_count > result.summary.source_file_count);
        assert!(result.summary.source_file_count >= 4);
        for parser in [
            "src-tauri/src/js_ts.rs",
            "src-tauri/src/native_languages.rs",
        ] {
            assert!(!result.edges.iter().any(|edge| edge.source == parser && edge.target == "src-tauri/src/analysis.rs"), "A language parser must not depend on the analysis orchestrator: {parser}");
            assert!(
                result
                    .edges
                    .iter()
                    .any(|edge| edge.source == parser
                        && edge.target == "src-tauri/src/parse_facts.rs"),
                "Shared parser facts must be visible in the real graph: {parser}"
            );
        }
        let unresolved_outside_fixtures: Vec<_> = result
            .summary
            .resolution_issues
            .iter()
            .filter(|issue| {
                issue.status == "unresolved"
                    && !issue.source.starts_with("src-tauri/tests/fixtures/")
            })
            .collect();
        assert!(
            unresolved_outside_fixtures.is_empty(),
            "{}",
            unresolved_outside_fixtures
                .iter()
                .map(|issue| format!(
                    "{}:{} {} => {}",
                    issue.source, issue.line, issue.specifier, issue.reason
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(
            result
                .nodes
                .iter()
                .any(|node| node.path == "src/styles.css")
        );
        assert!(!result.summary.root_path.starts_with("\\\\?\\"));
    }
}
