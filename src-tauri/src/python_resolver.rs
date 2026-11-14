//! File-level Python resolution over captured AST facts, never an interpreter.
use crate::{
    analysis::ResolutionIssue,
    parse_facts::{Fact, ImportDecl, PythonBinding},
};
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Clone)]
struct Module {
    file: Option<String>,
    bases: Vec<String>,
    initializers: BTreeSet<String>,
}
impl Module {
    fn files(&self) -> BTreeSet<String> {
        let mut files = self.initializers.clone();
        files.extend(self.file.clone());
        files
    }
}
struct Gap {
    status: &'static str,
    reason: String,
    candidates: Vec<String>,
}
impl Gap {
    fn unsupported(reason: impl Into<String>) -> Self {
        Self {
            status: "unsupported",
            reason: reason.into(),
            candidates: vec![],
        }
    }
}
enum Name {
    Binding,
    Submodule(Module),
}
// (declaration, conditional, import statement byte offset). Non-imports use
// usize::MAX. An import cannot establish its own binding while resolving;
// line numbers are insufficient because multiple statements can share a line.
type Namespace = HashMap<String, Vec<(PythonBinding, bool, usize)>>;
type BindingVisit = (String, String, Option<usize>);
pub(crate) struct PythonResolver {
    paths: HashSet<String>,
    directories: HashSet<String>,
    roots: Vec<String>,
    bindings: HashMap<String, Namespace>,
}
fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.into()
    } else if name.is_empty() {
        base.into()
    } else {
        format!("{base}/{name}")
    }
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("")
}
impl PythonResolver {
    pub fn new(
        paths: &HashSet<String>,
        sources: &[(String, Vec<ImportDecl>)],
        roots: Vec<String>,
    ) -> Self {
        let paths: HashSet<_> = paths
            .iter()
            .filter(|path| path.ends_with(".py"))
            .cloned()
            .collect();
        let mut directories = HashSet::new();
        for path in &paths {
            let mut directory = parent(path);
            while !directory.is_empty() {
                directories.insert(directory.to_owned());
                directory = parent(directory);
            }
        }
        let mut bindings = HashMap::<String, Namespace>::new();
        for (path, facts) in sources {
            for fact in facts {
                if let Some(Fact::PythonBinding {
                    name,
                    binding,
                    conditional,
                }) = &fact.facts
                {
                    bindings
                        .entry(path.clone())
                        .or_default()
                        .entry(name.clone())
                        .or_default()
                        .push((
                            binding.clone(),
                            *conditional,
                            match binding {
                                PythonBinding::Import { statement, .. } => *statement,
                                _ => usize::MAX,
                            },
                        ));
                }
            }
        }
        Self {
            paths,
            directories,
            roots,
            bindings,
        }
    }
    fn walk(
        &self,
        mut bases: Vec<String>,
        parts: &[&str],
        mut initializers: BTreeSet<String>,
    ) -> Result<Module, Gap> {
        if parts.is_empty() {
            let files: Vec<_> = bases
                .iter()
                .map(|base| join(base, "__init__.py"))
                .filter(|path| self.paths.contains(path))
                .collect();
            if files.len() > 1 {
                return Err(Gap {
                    status: "ambiguous",
                    reason: "Python package has multiple indexed initializers".into(),
                    candidates: files,
                });
            }
            return Ok(Module {
                file: files.into_iter().next(),
                bases,
                initializers,
            });
        }
        for (index, part) in parts.iter().enumerate() {
            let mut files = BTreeSet::new();
            let mut namespaces = BTreeSet::new();
            let mut attempted = Vec::new();
            for base in &bases {
                let path = join(base, part);
                let module = format!("{path}.py");
                let init = join(&path, "__init__.py");
                attempted.extend([module.clone(), init.clone()]);
                if self.paths.contains(&module) {
                    files.insert(module);
                }
                if self.paths.contains(&init) {
                    files.insert(init);
                } else if self.directories.contains(&path) {
                    namespaces.insert(path);
                }
            }
            if files.len() > 1 {
                return Err(Gap{status:"ambiguous",reason:"Python module has multiple indexed candidates; source root order is not assumed".into(),candidates:files.into_iter().collect()});
            }
            if let Some(file) = files.into_iter().next() {
                let package = file.ends_with("/__init__.py");
                if index + 1 == parts.len() {
                    return Ok(Module {
                        bases: if package {
                            vec![parent(&file).into()]
                        } else {
                            vec![]
                        },
                        file: Some(file),
                        initializers,
                    });
                }
                if !package {
                    return Err(Gap {
                        status: "unresolved",
                        reason: "A module file cannot contain the requested package hierarchy"
                            .into(),
                        candidates: vec![file],
                    });
                }
                if self
                    .bindings
                    .get(&file)
                    .is_some_and(|bindings| bindings.contains_key("__path__"))
                {
                    return Err(Gap::unsupported(
                        "Python package changes __path__; child modules are not guessed",
                    ));
                }
                bases = vec![parent(&file).into()];
                initializers.insert(file);
            } else if !namespaces.is_empty() {
                bases = namespaces.into_iter().collect();
                if index + 1 == parts.len() {
                    return Ok(Module {
                        file: None,
                        bases,
                        initializers,
                    });
                }
            } else {
                return Err(Gap {
                    status: "unresolved",
                    reason: "Python module is absent from captured source roots".into(),
                    candidates: attempted,
                });
            }
        }
        unreachable!()
    }
    fn module(&self, source: &str, module: &str, relative: usize) -> Result<Module, Gap> {
        let parts: Vec<_> = module.split('.').filter(|part| !part.is_empty()).collect();
        if relative == 0 {
            return self.walk(self.roots.clone(),&parts,BTreeSet::new()).map_err(|mut gap|{
                // A missing top-level name may be stdlib or an external package;
                // a missing child of an indexed package is a local resolution gap.
                if gap.status=="unresolved" && !gap.candidates.is_empty() && !gap.candidates.iter().any(|path|self.paths.contains(path)){
                    let top=parts.first().copied().unwrap_or("");
                    if !self.roots.iter().any(|root|self.paths.contains(&format!("{}.py",join(root,top)))||self.directories.contains(&join(root,top))){gap.status="external";gap.reason="No local Python top-level module; stdlib/distribution mapping and runtime environment are unknown".into();}
                }
                gap
            });
        }
        let root = self
            .roots
            .iter()
            .filter(|root| root.is_empty() || source.starts_with(&format!("{root}/")))
            .max_by_key(|root| root.len())
            .ok_or_else(|| {
                Gap::unsupported("Relative import source is outside configured Python roots")
            })?;
        let local = if root.is_empty() {
            source
        } else {
            source.strip_prefix(&format!("{root}/")).unwrap()
        };
        let mut package: Vec<_> = parent(local)
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        if relative > package.len() {
            return Err(Gap {
                status: "unresolved",
                reason: "Relative Python import escapes its source root".into(),
                candidates: vec![],
            });
        }
        package.truncate(package.len() - (relative - 1));
        package.extend(parts);
        self.walk(vec![root.clone()], &package, BTreeSet::new())
    }
    fn name(
        &self,
        module: &Module,
        name: &str,
        visiting: &mut HashSet<BindingVisit>,
        exclude: Option<(&str, usize)>,
    ) -> Result<Name, Gap> {
        if name == "*" {
            return Err(Gap::unsupported(
                "Python wildcard exports (__all__ and runtime bindings) are not expanded",
            ));
        }
        if name.contains(['.', '/', '\\', ':']) {
            return Err(Gap::unsupported("Unsupported Python from-import name"));
        }
        // Loader-provided attributes must never be mistaken for child files.
        if matches!(
            name,
            "__name__" | "__package__" | "__loader__" | "__spec__" | "__dict__" | "__doc__"
        ) || name == "__path__" && !module.bases.is_empty()
        {
            return Ok(Name::Binding);
        }
        if matches!(name, "__file__" | "__cached__" | "__builtins__") {
            return if module.file.is_some() {
                Ok(Name::Binding)
            } else {
                Err(Gap::unsupported(
                    "Loader-provided namespace attributes are not inferred",
                ))
            };
        }
        let key = (
            module
                .file
                .clone()
                .unwrap_or_else(|| module.bases.join("|")),
            name.into(),
            exclude
                .filter(|(file, _)| Some(*file) == module.file.as_deref())
                .map(|(_, statement)| statement),
        );
        if visiting.len() >= 64 || !visiting.insert(key.clone()) {
            return Err(Gap::unsupported(
                "Python reexport cycle or depth limit; no target is guessed",
            ));
        }
        let result = self.name_inner(module, name, visiting, exclude);
        visiting.remove(&key);
        result
    }
    fn name_inner(
        &self,
        module: &Module,
        name: &str,
        visiting: &mut HashSet<BindingVisit>,
        exclude: Option<(&str, usize)>,
    ) -> Result<Name, Gap> {
        if let Some(file) = &module.file {
            let namespace = self.bindings.get(file);
            if namespace.is_some_and(|bindings| bindings.contains_key("*")) {
                return Err(Gap::unsupported(format!(
                    "{file}: wildcard import, incomplete parse or dynamic namespace mutation prevents proving `{name}`"
                )));
            }
            let bindings: Vec<_> = namespace
                .and_then(|bindings| bindings.get(name))
                .into_iter()
                .flatten()
                .filter(|binding| exclude != Some((file.as_str(), binding.2)))
                .collect();
            if !bindings.is_empty() {
                if bindings.len() != 1 || bindings[0].1 {
                    return Err(Gap::unsupported(format!(
                        "{file}: `{name}` has conditional or multiple bindings"
                    )));
                }
                return match &bindings[0].0 {
                    PythonBinding::Definition => Ok(Name::Binding),
                    PythonBinding::Unknown => Err(Gap::unsupported(format!(
                        "{file}: binding `{name}` is not statically known"
                    ))),
                    PythonBinding::Import {
                        statement,
                        module: request,
                        name: imported,
                        relative,
                    } => {
                        match self.module(file, request, *relative) {
                            Ok(origin) => {
                                if let Some(imported) = imported {
                                    self.name(
                                        &origin,
                                        imported,
                                        visiting,
                                        Some((file, *statement)),
                                    )?;
                                }
                                Ok(Name::Binding)
                            }
                            Err(gap) if gap.status == "external" => Ok(Name::Binding), // Explicit binding, not an inferred external file.
                            Err(gap) => Err(gap),
                        }
                    }
                };
            }
            if namespace.is_some_and(|bindings| {
                bindings.contains_key("__getattr__") || bindings.contains_key("__path__")
            }) {
                return Err(Gap::unsupported(format!(
                    "{file}: dynamic package attributes/path prevent resolving `{name}`"
                )));
            }
        }
        if module.bases.is_empty() {
            return Err(Gap::unsupported(format!(
                "`{name}` is not a captured module binding; runtime attributes are not evaluated"
            )));
        }
        self.walk(module.bases.clone(),&[name],module.files()).map(Name::Submodule).map_err(|mut gap|{
            if gap.status=="unresolved"{gap.status="unsupported";gap.reason=format!("`{name}` is neither a known attribute nor a captured submodule; runtime attributes are not evaluated");}gap
        })
    }
    pub fn resolve(
        &self,
        source: &str,
        import: &ImportDecl,
    ) -> (Vec<(String, String)>, Vec<ResolutionIssue>) {
        let Some(Fact::Python {
            statement,
            module,
            names,
            relative,
            conditional,
        }) = &import.facts
        else {
            return (vec![], vec![]);
        };
        let mut targets = BTreeSet::new();
        let mut issues = Vec::new();
        let mut report = |gap: Gap| {
            issues.push(ResolutionIssue{source:source.into(),specifier:import.specifier.clone(),line:import.line,status:gap.status.into(),reason:gap.reason,attempted_candidates:gap.candidates,hint:"Используются только захваченные файлы и AST bindings. Окружение, hooks и код не запускаются".into()})
        };
        match self.module(source, module, *relative) {
            Ok(found) => {
                targets.extend(found.files());
                for name in names {
                    match self.name(
                        &found,
                        name,
                        &mut HashSet::new(),
                        statement.map(|statement| (source, statement)),
                    ) {
                        Ok(Name::Binding) => {}
                        Ok(Name::Submodule(child)) => targets.extend(child.files()),
                        Err(gap) => report(gap),
                    }
                }
            }
            Err(gap) => report(gap),
        }
        targets.remove(source);
        (
            targets
                .into_iter()
                .map(|target| {
                    (
                        target,
                        if *conditional {
                            "conditional_import"
                        } else {
                            "python_import"
                        }
                        .into(),
                    )
                })
                .collect(),
            issues,
        )
    }
}
