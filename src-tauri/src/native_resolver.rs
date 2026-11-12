use crate::{
    analysis::ResolutionIssue,
    parse_facts::{Fact, ImportDecl},
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::Path,
};

#[derive(Default)]
pub(crate) struct Resolution {
    pub targets: Vec<(String, String)>,
    pub issues: Vec<ResolutionIssue>,
}
pub(crate) struct NativeResolver {
    paths: HashSet<String>,
    python_roots: Vec<String>,
    modules: HashMap<String, Vec<(String, Vec<String>, String)>>,
    symbols: HashMap<(String, Vec<String>), BTreeSet<String>>,
    pub manifest_issues: Vec<(String, String)>,
    pub declarations: Vec<(String, crate::analysis::PackageInfo)>,
    dependencies: HashMap<(String, String), String>,
    local_dependencies: HashMap<(String, String), String>,
    module_files: HashMap<(String, Vec<String>), BTreeSet<String>>,
}
fn join(a: &str, b: &str) -> Option<String> {
    let mut parts = Vec::new();
    let path = Path::new(b);
    if path.is_absolute() || b.contains('\\') || b.contains(':') {
        return None;
    }
    let joined = Path::new(a).join(b);
    for c in joined.components() {
        match c {
            std::path::Component::Normal(n) => parts.push(n.to_str()?.to_owned()),
            std::path::Component::ParentDir => {
                parts.pop()?;
            }
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}
fn parent(path: &str) -> String {
    Path::new(path)
        .parent()
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .replace('\\', "/")
}
impl NativeResolver {
    fn python_manifests(
        &mut self,
        root: &Path,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<(), String> {
        let mut manifests: Vec<_> = self
            .paths
            .iter()
            .filter(|p| {
                Path::new(p)
                    .file_name()
                    .is_some_and(|n| n == "pyproject.toml" || n == "requirements.txt")
            })
            .cloned()
            .collect();
        manifests.sort();
        let mut seen = HashSet::new();
        while let Some(manifest) = manifests.pop() {
            crate::snapshot::check_cancel(cancel)?;
            if !seen.insert(manifest.clone()) {
                continue;
            }
            if seen.len() > 1000 {
                return Err("Python manifest include limit exceeded".into());
            }
            let input = std::fs::read_to_string(root.join(&manifest))
                .map_err(|e| format!("{manifest}: {e}"))?;
            if input.len() > 1024 * 1024 {
                return Err(format!("{manifest}: manifest exceeds 1 MiB"));
            }
            let mut declarations = Vec::new();
            if manifest.ends_with("pyproject.toml") {
                let value: toml::Value =
                    toml::from_str(&input).map_err(|e| format!("{manifest}: {e}"))?;
                if let Some(project) = value.get("project") {
                    if project.get("dynamic").is_some() {
                        self.manifest_issues.push((
                            manifest.clone(),
                            "Dynamic Python metadata не вычисляется".into(),
                        ));
                    }
                    for item in project
                        .get("dependencies")
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                    {
                        if let Some(text) = item.as_str() {
                            declarations.push(("dependencies".to_string(), text.to_owned()));
                        }
                    }
                    for (group, items) in project
                        .get("optional-dependencies")
                        .and_then(|v| v.as_table())
                        .into_iter()
                        .flatten()
                    {
                        for item in items.as_array().into_iter().flatten() {
                            if let Some(text) = item.as_str() {
                                declarations.push((format!("optional:{group}"), text.to_owned()));
                            }
                        }
                    }
                }
            } else {
                for line in input
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                {
                    if let Some(include) = line
                        .strip_prefix("-r ")
                        .or_else(|| line.strip_prefix("--requirement "))
                    {
                        let path = join(&parent(&manifest), include.trim())
                            .ok_or("Requirements include leaves project scope")?;
                        if !self.paths.contains(&path) {
                            return Err(format!("Requirements include unavailable: {path}"));
                        }
                        manifests.push(path);
                    } else if line.starts_with('-') {
                        self.manifest_issues.push((
                            manifest.clone(),
                            format!("Неподдерживаемая requirements-директива: {line}"),
                        ));
                    } else if !line.starts_with('-') {
                        declarations.push(("requirements".into(), line.into()));
                    }
                }
            }
            for (group, requirement) in declarations {
                let name: String = requirement
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                    .collect();
                if !name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric())
                    || !name
                        .chars()
                        .last()
                        .is_some_and(|c| c.is_ascii_alphanumeric())
                    || requirement[name.len()..].starts_with("://")
                    || requirement[name.len()..].starts_with('+')
                {
                    self.manifest_issues.push((
                        manifest.clone(),
                        format!("Не удалось подтвердить имя distribution: {requirement}"),
                    ));
                    continue;
                }
                let id = format!(
                    "package:pypi:{manifest}:{group}:{}",
                    name.to_lowercase().replace(['_', '.'], "-")
                );
                if self
                    .declarations
                    .iter()
                    .any(|(existing, _)| existing == &id)
                {
                    continue;
                }
                self.declarations.push((
                    id,
                    crate::analysis::PackageInfo {
                        name: format!("PyPI · {name} ({manifest})"),
                        version: requirement,
                        dependency_type: group,
                        imported_by: 0,
                    },
                ));
            }
        }
        Ok(())
    }
    pub fn new(
        root: &Path,
        sources: &[(String, Vec<ImportDecl>)],
        paths: HashSet<String>,
        configured_roots: &[String],
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Self, String> {
        let mut python_roots = if configured_roots.is_empty() {
            let mut r = vec![String::new()];
            if paths
                .iter()
                .any(|p| p.starts_with("src/") && p.ends_with(".py"))
            {
                r.push("src".into());
            }
            r
        } else {
            configured_roots
                .iter()
                .map(|r| join("", r).ok_or("Python source root must stay inside project scope"))
                .collect::<Result<_, _>>()?
        };
        python_roots.sort();
        python_roots.dedup();
        let mut out = Self {
            paths,
            python_roots,
            modules: HashMap::new(),
            symbols: HashMap::new(),
            manifest_issues: Vec::new(),
            declarations: Vec::new(),
            dependencies: HashMap::new(),
            local_dependencies: HashMap::new(),
            module_files: HashMap::new(),
        };
        let sources: HashMap<_, _> = sources.iter().map(|(p, i)| (p.as_str(), i)).collect();
        let mut manifests: Vec<_> = out
            .paths
            .iter()
            .filter(|p| p.ends_with("Cargo.toml"))
            .cloned()
            .collect();
        manifests.sort();
        let manifest_values = manifests
            .iter()
            .map(|p| {
                let input = std::fs::read_to_string(root.join(p)).map_err(|e| e.to_string())?;
                let value =
                    toml::from_str::<toml::Value>(&input).map_err(|e| format!("{p}: {e}"))?;
                Ok((p.clone(), value))
            })
            .collect::<Result<HashMap<_, _>, String>>()?;
        for manifest in manifests {
            crate::snapshot::check_cancel(cancel)?;
            let input = std::fs::read_to_string(root.join(&manifest)).map_err(|e| e.to_string())?;
            let value: toml::Value =
                toml::from_str(&input).map_err(|e| format!("{manifest}: {e}"))?;
            if value.get("package").is_none() {
                continue;
            }
            let base = parent(&manifest);
            if value.get("target").is_some() {
                out.manifest_issues.push((
                    manifest.clone(),
                    "Target-specific Cargo dependencies не разрешаются без cfg профиля".into(),
                ));
            }
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                for (alias, dependency) in value
                    .get(section)
                    .and_then(|v| v.as_table())
                    .into_iter()
                    .flatten()
                {
                    let inherited = if dependency.get("workspace").and_then(|v| v.as_bool())
                        == Some(true)
                    {
                        manifest_values
                            .iter()
                            .filter(|(p, v)| {
                                v.get("workspace").is_some()
                                    && (parent(p).is_empty()
                                        || manifest.starts_with(&(parent(p) + "/")))
                            })
                            .max_by_key(|(p, _)| p.len())
                            .and_then(|(_, v)| v.get("workspace")?.get("dependencies")?.get(alias))
                    } else {
                        None
                    };
                    let dependency = inherited.unwrap_or(dependency);
                    let package = dependency
                        .get("package")
                        .and_then(|v| v.as_str())
                        .unwrap_or(alias);
                    let version = dependency
                        .as_str()
                        .or_else(|| dependency.get("version").and_then(|v| v.as_str()))
                        .unwrap_or("не определена / workspace/path");
                    let id = format!("package:cargo:{manifest}:{section}:{alias}");
                    out.declarations.push((
                        id.clone(),
                        crate::analysis::PackageInfo {
                            name: format!("Cargo · {package} ({manifest}, {alias})"),
                            version: version.into(),
                            dependency_type: section.into(),
                            imported_by: 0,
                        },
                    ));
                    if section == "dependencies" {
                        if inherited.is_none()
                            && let Some(path) = dependency.get("path").and_then(|v| v.as_str())
                        {
                            if let Some(directory) = join(&base, path)
                                && let Some(target_manifest) = join(&directory, "Cargo.toml")
                                && let Some(target) = manifest_values.get(&target_manifest)
                                && let Some(target_file) = join(
                                    &directory,
                                    target
                                        .get("lib")
                                        .and_then(|v| v.get("path"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("src/lib.rs"),
                                )
                                && out.paths.contains(&target_file)
                            {
                                out.local_dependencies.insert(
                                    (manifest.clone(), alias.replace('-', "_")),
                                    format!("{target_manifest}:{target_file}"),
                                );
                            } else {
                                out.manifest_issues.push((manifest.clone(),format!("Path dependency `{alias}` не имеет известного captured lib target")));
                            }
                        }
                        out.dependencies
                            .insert((manifest.clone(), alias.replace('-', "_")), id);
                    }
                }
            }
            let mut targets = Vec::new();
            if let Some(path) = value
                .get("lib")
                .and_then(|v| v.get("path"))
                .and_then(|v| v.as_str())
            {
                targets.push(path.to_owned());
            } else if value
                .pointer_compat("package.autolib")
                .and_then(|v| v.as_bool())
                != Some(false)
            {
                targets.push("src/lib.rs".into());
            }
            if value
                .pointer_compat("package.autobins")
                .and_then(|v| v.as_bool())
                != Some(false)
            {
                targets.push("src/main.rs".into());
                for path in &out.paths {
                    let prefix = join(&base, "src/bin/").unwrap_or_default() + "/";
                    if path.starts_with(&prefix) && path.ends_with(".rs") {
                        let rest = &path[prefix.len()..];
                        if !rest.contains('/')
                            || rest.ends_with("/main.rs") && rest.matches('/').count() == 1
                        {
                            targets.push(
                                path.strip_prefix(&(base.clone() + "/"))
                                    .unwrap_or(path)
                                    .to_owned(),
                            );
                        }
                    }
                }
            }
            if let Some(bins) = value.get("bin").and_then(|v| v.as_array()) {
                for bin in bins {
                    if let Some(path) = bin.get("path").and_then(|v| v.as_str()) {
                        targets.push(path.into());
                    }
                }
            }
            targets.sort();
            targets.dedup();
            for target in targets {
                let path = join(&base, &target).ok_or("Cargo target leaves project scope")?;
                if !out.paths.contains(&path) {
                    continue;
                }
                let unit = format!("{manifest}:{path}");
                let mut pending = vec![(path, Vec::<String>::new(), true, Vec::<String>::new())];
                let mut visited = HashSet::new();
                while let Some((file, context, is_root, ancestors)) = pending.pop() {
                    crate::snapshot::check_cancel(cancel)?;
                    if visited.len() > 100_000 {
                        return Err("Rust module context limit exceeded".into());
                    }
                    if ancestors.contains(&file) {
                        continue;
                    }
                    if !visited.insert((file.clone(), context.clone())) {
                        continue;
                    }
                    out.module_files
                        .entry((unit.clone(), context.clone()))
                        .or_default()
                        .insert(file.clone());
                    out.modules.entry(file.clone()).or_default().push((
                        unit.clone(),
                        context.clone(),
                        if is_root {
                            parent(&file)
                        } else {
                            module_dir(&file)
                        },
                    ));
                    for import in sources
                        .get(file.as_str())
                        .into_iter()
                        .flat_map(|i| i.iter())
                    {
                        if let Some(Fact::RustItem {
                            context: inline_context,
                            name,
                            conditional: false,
                        }) = &import.facts
                        {
                            let mut full = context.clone();
                            full.extend(inline_context.clone());
                            full.push(name.clone());
                            out.symbols
                                .entry((unit.clone(), full))
                                .or_default()
                                .insert(file.clone());
                        }
                        if let Some(Fact::RustModule {
                            context: inline_context,
                            name,
                            path,
                            inline,
                            conditional,
                        }) = &import.facts
                        {
                            if *conditional || path.is_some() && !inline_context.is_empty() {
                                continue;
                            }
                            let mut full = context.clone();
                            full.extend(inline_context.iter().cloned());
                            full.push(name.clone());
                            if *inline {
                                out.module_files
                                    .entry((unit.clone(), full))
                                    .or_default()
                                    .insert(file.clone());
                                continue;
                            }
                            let directory = if is_root {
                                parent(&file)
                            } else {
                                module_dir(&file)
                            };
                            let inline_path = inline_context.join("/");
                            let base_dir = join(&directory, &inline_path).unwrap_or(directory);
                            let candidates = if let Some(path) = path {
                                join(&parent(&file), path).into_iter().collect::<Vec<_>>()
                            } else {
                                [format!("{name}.rs"), format!("{name}/mod.rs")]
                                    .iter()
                                    .filter_map(|name| join(&base_dir, name))
                                    .collect()
                            };
                            let found: Vec<_> = candidates
                                .into_iter()
                                .filter(|p| out.paths.contains(p))
                                .collect();
                            if found.len() == 1 {
                                let mut ancestors = ancestors.clone();
                                ancestors.push(file.clone());
                                pending.push((found[0].clone(), full, false, ancestors));
                            }
                        }
                    }
                }
            }
        }
        out.python_manifests(root, cancel)?;
        Ok(out)
    }
    pub fn python_roots(&self) -> Vec<String> {
        self.python_roots.clone()
    }
    pub fn resolve(&self, source: &str, import: &ImportDecl) -> Resolution {
        let mut result = Resolution::default();
        let Some(fact) = &import.facts else {
            return result;
        };
        let mut issue = |status: &str, reason: &str, candidates: Vec<String>| {
            result.issues.push(ResolutionIssue {source:source.into(),specifier:import.specifier.clone(),line:import.line,status:status.into(),reason:reason.into(),attempted_candidates:candidates,hint:"Статический профиль использует только файлы выбранного проекта; окружение и код не запускаются".into()})
        };
        match fact {
            Fact::RustItem { .. } => {}
            Fact::Gap { reason } => issue("unsupported", reason, vec![]),
            Fact::Python {
                module,
                names,
                relative,
                conditional,
            } => {
                let mut candidates = Vec::new();
                for root in &self.python_roots {
                    let local = if *relative == 0 || root.is_empty() {
                        source
                    } else if let Some(local) = source.strip_prefix(&(root.clone() + "/")) {
                        local
                    } else {
                        continue;
                    };
                    let mut package = parent(local)
                        .split('/')
                        .filter(|p| !p.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    if *relative > 0 {
                        if *relative > package.len() {
                            continue;
                        }
                        for _ in 1..*relative {
                            package.pop();
                        }
                    } else {
                        package.clear();
                    }
                    package.extend(
                        module
                            .split('.')
                            .filter(|p| !p.is_empty())
                            .map(str::to_owned),
                    );
                    if let Some(base) = join(root, &package.join("/")) {
                        candidates.push(base);
                    }
                }
                candidates.sort();
                candidates.dedup();
                let shadowed = candidates.iter().any(|base| {
                    let mut dir = parent(base);
                    while !dir.is_empty() && !self.python_roots.contains(&dir) {
                        if self.paths.contains(&format!("{dir}.py")) {
                            return true;
                        }
                        dir = parent(&dir);
                    }
                    false
                });
                if shadowed {
                    issue(
                        "ambiguous",
                        "A module file shadows the requested package hierarchy",
                        candidates,
                    );
                    return result;
                }
                let mut found = BTreeSet::new();
                let mut initializers = BTreeSet::new();
                for base in &candidates {
                    for file in [format!("{base}.py"), format!("{base}/__init__.py")] {
                        if self.paths.contains(&file) {
                            found.insert(file);
                        }
                    }
                }
                let base_files: Vec<_> = found
                    .iter()
                    .filter(|p| {
                        candidates.iter().any(|base| {
                            **p == format!("{base}.py") || **p == format!("{base}/__init__.py")
                        })
                    })
                    .collect();
                let roots_found: HashSet<_> = candidates
                    .iter()
                    .filter(|b| {
                        found
                            .iter()
                            .any(|p| p.starts_with(&format!("{b}/")) || *p == format!("{b}.py"))
                    })
                    .collect();
                if roots_found.len() > 1 || base_files.len() > 1 {
                    issue(
                        "ambiguous",
                        "Python module has multiple indexed candidates",
                        found.into_iter().collect(),
                    );
                } else if found.is_empty() {
                    issue(
                        if *relative > 0 {
                            "unresolved"
                        } else {
                            "external"
                        },
                        "No unique local Python module; distribution mapping and runtime path are unknown",
                        candidates,
                    );
                } else {
                    for file in &found {
                        let mut dir = parent(file);
                        while !dir.is_empty() && !self.python_roots.contains(&dir) {
                            let init = format!("{dir}/__init__.py");
                            if self.paths.contains(&init) {
                                initializers.insert(init);
                            }
                            dir = parent(&dir);
                        }
                    }
                    found.extend(initializers);
                    for target in found {
                        if target != source {
                            result.targets.push((
                                target,
                                if *conditional {
                                    "conditional_import"
                                } else {
                                    "python_import"
                                }
                                .into(),
                            ));
                        }
                    }
                    if !names.is_empty() {
                        issue(
                            "unsupported",
                            "from-import names may be package attributes; symbol bindings are not evaluated",
                            vec![],
                        );
                    }
                }
            }
            Fact::RustModule {
                context,
                name,
                path,
                inline,
                conditional,
            } => {
                if *inline {
                    return result;
                }
                if *conditional {
                    issue(
                        "unsupported",
                        "Conditional Rust module: active cfg profile is unknown",
                        vec![],
                    );
                    return result;
                }
                let mut found = BTreeSet::new();
                for (unit, base, _) in self.modules.get(source).into_iter().flatten() {
                    let mut key = base.clone();
                    key.extend(context.clone());
                    key.push(name.clone());
                    if path.is_some() && !context.is_empty() {
                        continue;
                    }
                    let directory = self
                        .modules
                        .get(source)
                        .into_iter()
                        .flatten()
                        .find(|(u, c, _)| u == unit && c == base)
                        .map(|(_, _, d)| d.clone())
                        .unwrap_or_default();
                    if path.is_none()
                        && let Some(directory) = join(&directory, &context.join("/"))
                    {
                        for candidate in [format!("{name}.rs"), format!("{name}/mod.rs")] {
                            if let Some(candidate) = join(&directory, &candidate)
                                && self.paths.contains(&candidate)
                            {
                                found.insert(candidate);
                            }
                        }
                    }
                    if let Some(files) = self.module_files.get(&(unit.clone(), key)) {
                        found.extend(files.clone());
                    }
                }
                if found.len() == 1 {
                    result.targets.push((
                        found.into_iter().next().unwrap(),
                        "module_declaration".into(),
                    ));
                } else {
                    issue(
                        if found.len() > 1 {
                            "ambiguous"
                        } else {
                            "unresolved"
                        },
                        "Rust module declaration has no unique captured target",
                        path.iter().cloned().chain(found).collect(),
                    );
                }
            }
            Fact::RustUse {
                context,
                path,
                conditional,
            } => {
                if *conditional {
                    issue(
                        "unsupported",
                        "Conditional Rust use: cfg profile is unknown",
                        vec![],
                    );
                    return result;
                }
                let mut found = BTreeSet::new();
                for (unit, base, _) in self.modules.get(source).into_iter().flatten() {
                    let mut parts = path.split("::").map(str::to_owned).collect::<Vec<_>>();
                    let mut current = base.clone();
                    current.extend(context.clone());
                    if parts.first().is_some_and(|p| p == "crate") {
                        current.clear();
                        parts.remove(0);
                    } else if parts.first().is_some_and(|p| p == "self") {
                        parts.remove(0);
                    } else {
                        let mut escaped_root = false;
                        while parts.first().is_some_and(|p| p == "super") {
                            if current.pop().is_none() {
                                escaped_root = true;
                                break;
                            }
                            parts.remove(0);
                        }
                        if escaped_root {
                            continue;
                        }
                    }
                    if parts.last().is_some_and(|p| p == "*") {
                        parts.pop();
                    }
                    let first = parts.first().cloned();
                    current.extend(parts);
                    if !["crate::", "self::", "super::"]
                        .iter()
                        .any(|prefix| path.starts_with(prefix))
                        && let Some(alias) = first
                        && let Some(manifest) = unit.split(':').next()
                        && let Some(id) = self.dependencies.get(&(manifest.to_string(), alias))
                    {
                        let alias = path.split("::").next().unwrap_or("");
                        let suffix = path
                            .split("::")
                            .skip(1)
                            .filter(|p| *p != "*")
                            .map(str::to_owned)
                            .collect::<Vec<_>>();
                        let local = self
                            .local_dependencies
                            .get(&(manifest.to_string(), alias.to_owned()))
                            .and_then(|unit| {
                                self.module_files
                                    .get(&(unit.clone(), suffix.clone()))
                                    .or_else(|| self.symbols.get(&(unit.clone(), suffix)))
                            });
                        if let Some(files) = local {
                            found.extend(files.clone());
                        } else {
                            found.insert(id.clone());
                        }
                    }
                    if let Some(files) = self
                        .module_files
                        .get(&(unit.clone(), current.clone()))
                        .or_else(|| self.symbols.get(&(unit.clone(), current.clone())))
                    {
                        found.extend(files.clone());
                    }
                }
                if found.len() == 1 {
                    let target = found.into_iter().next().unwrap();
                    if target != source {
                        let kind = if target.starts_with("package:") {
                            "package"
                        } else {
                            "rust_import"
                        };
                        result.targets.push((target, kind.into()));
                    }
                } else {
                    issue(
                        if found.len() > 1 {
                            "ambiguous"
                        } else {
                            "unsupported"
                        },
                        "Rust use requires unresolved item/reexport/external-crate context; no target is guessed",
                        found.into_iter().collect(),
                    );
                }
            }
        }
        result
    }
}
fn module_dir(file: &str) -> String {
    if Path::new(file).file_name().is_some_and(|n| n == "mod.rs") {
        parent(file)
    } else {
        file.trim_end_matches(".rs").into()
    }
}

trait TomlPointer {
    fn pointer_compat(&self, path: &str) -> Option<&toml::Value>;
}
impl TomlPointer for toml::Value {
    fn pointer_compat(&self, path: &str) -> Option<&toml::Value> {
        let mut value = self;
        for part in path.split('.') {
            value = value.get(part)?;
        }
        Some(value)
    }
}
