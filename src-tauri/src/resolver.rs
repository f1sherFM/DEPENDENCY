use oxc_resolver::{
    FileMetadata, FileSystem, FileSystemOs, FsCache, ResolveError, ResolveOptions, ResolverGeneric,
    TsconfigOptions, TsconfigReferences,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// A resolver is cached per nearest project config and import condition. It reads
/// package/config files but never evaluates JavaScript or TypeScript config code.
pub struct ResolverPool {
    root: PathBuf,
    resolvers: HashMap<(Option<PathBuf>, u8, bool), ResolverGeneric<FsCache<FencedFs>>>,
    node_resolution: HashMap<Option<PathBuf>, bool>,
    isolated: bool,
}

impl ResolverPool {
    pub fn new(root: &Path) -> Self {
        Self::with_isolation(root, false)
    }
    pub fn with_isolation(root: &Path, isolated: bool) -> Self {
        Self {
            root: root.to_path_buf(),
            resolvers: HashMap::new(),
            node_resolution: HashMap::new(),
            isolated,
        }
    }

    pub fn resolve(
        &mut self,
        source_file: &Path,
        specifier: &str,
        kind: &str,
    ) -> Result<PathBuf, ResolveError> {
        let directory = source_file.parent().unwrap_or(&self.root);
        let config = nearest_config(directory, &self.root);
        let node_resolution = *self
            .node_resolution
            .entry(config.clone())
            .or_insert_with(|| {
                config
                    .as_ref()
                    .is_some_and(|path| node_resolution_config(path, &self.root))
            });
        let source_is_esm = node_resolution && is_esm_source(source_file, &self.root);
        let is_require = kind == "commonjs_import"
            || (node_resolution && !source_is_esm && kind != "dynamic_import");
        let strict_esm = node_resolution && source_is_esm && kind != "commonjs_import";
        let mode = match (kind == "type_only_import", is_require) {
            (true, true) => 3,
            (true, false) => 2,
            (false, true) => 1,
            (false, false) => 0,
        };
        let key = (config.clone(), mode, strict_esm);
        let resolver = self.resolvers.entry(key).or_insert_with(|| {
            let conditions: Vec<String> = if kind == "type_only_import" {
                vec!["types".into(), "node".into(), "import".into()]
            } else if is_require {
                vec!["node".into(), "require".into()]
            } else {
                vec!["node".into(), "import".into()]
            };
            let config = config.map(|config_file| TsconfigOptions {
                config_file,
                references: TsconfigReferences::Auto,
            });
            ResolverGeneric::new_with_cache(
                Arc::new(FsCache::new(FencedFs {
                    root: self.isolated.then(|| self.root.clone()),
                })),
                ResolveOptions {
                    tsconfig: config,
                    condition_names: conditions,
                    extensions: [
                        ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json",
                        ".css", ".scss", ".sass", ".less", ".svg", ".png", ".jpg", ".jpeg",
                        ".webp", ".gif", ".avif", ".wasm",
                    ]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                    extension_alias: vec![
                        (
                            ".js".into(),
                            vec![".ts".into(), ".tsx".into(), ".js".into()],
                        ),
                        (".mjs".into(), vec![".mts".into(), ".mjs".into()]),
                        (".cjs".into(), vec![".cts".into(), ".cjs".into()]),
                    ],
                    main_fields: vec!["module".into(), "main".into()],
                    fully_specified: strict_esm,
                    ..ResolveOptions::default()
                },
            )
        });
        resolver
            .resolve(directory, specifier)
            .map(|resolution| resolution.into_path_buf())
    }
}

fn read_jsonc(path: &Path) -> Option<serde_json::Value> {
    let mut source = std::fs::read_to_string(path).ok()?;
    json_strip_comments::strip(&mut source).ok()?;
    serde_json::from_str(&source).ok()
}

fn node_resolution_config(config: &Path, root: &Path) -> bool {
    let mut current = Some(config.to_path_buf());
    let mut visited = std::collections::HashSet::new();
    while let Some(path) = current {
        if !path.starts_with(root) || !visited.insert(path.clone()) {
            return false;
        }
        let Some(value) = read_jsonc(&path) else {
            return false;
        };
        if let Some(mode) = value
            .pointer("/compilerOptions/moduleResolution")
            .and_then(serde_json::Value::as_str)
        {
            return matches!(
                mode.to_ascii_lowercase().as_str(),
                "node16" | "nodenext" | "node18" | "node20"
            );
        }
        if let Some(module) = value
            .pointer("/compilerOptions/module")
            .and_then(serde_json::Value::as_str)
            && matches!(module.to_ascii_lowercase().as_str(), "node16" | "nodenext")
        {
            return true;
        }
        let extends = value.get("extends");
        let parent = extends
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                extends
                    .and_then(serde_json::Value::as_array)
                    .and_then(|items| items.last())
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            });
        current = parent.and_then(|specifier| resolve_extends(&path, &specifier, root));
    }
    false
}

fn resolve_extends(config: &Path, specifier: &str, root: &Path) -> Option<PathBuf> {
    if specifier.starts_with('.') || Path::new(specifier).is_absolute() {
        let base = if Path::new(specifier).is_absolute() {
            PathBuf::from(specifier)
        } else {
            config.parent()?.join(specifier)
        };
        return config_candidate(base, root);
    }
    let parts: Vec<&str> = specifier.split('/').collect();
    let package_parts = if specifier.starts_with('@') { 2 } else { 1 };
    if parts.len() < package_parts {
        return None;
    }
    let package = parts[..package_parts].join("/");
    let subpath = parts[package_parts..].join("/");
    let mut directory = config.parent()?;
    loop {
        let package_root = directory.join("node_modules").join(&package);
        let target = if subpath.is_empty() {
            let manifest = read_jsonc(&package_root.join("package.json"));
            manifest
                .and_then(|value| {
                    value
                        .get("tsconfig")
                        .and_then(serde_json::Value::as_str)
                        .map(|path| package_root.join(path))
                })
                .unwrap_or_else(|| package_root.join("tsconfig.json"))
        } else {
            package_root.join(subpath.replace('/', std::path::MAIN_SEPARATOR_STR))
        };
        if let Some(path) = config_candidate(target, root) {
            return Some(path);
        }
        if directory == root {
            return None;
        }
        directory = directory.parent()?;
        if !directory.starts_with(root) {
            return None;
        }
    }
}

fn config_candidate(path: PathBuf, root: &Path) -> Option<PathBuf> {
    let path = lexical_path(&path);
    if !path.starts_with(root) {
        return None;
    }
    if path.is_file() {
        return Some(path);
    }
    if path.extension().is_none() {
        let json = path.with_extension("json");
        if json.is_file() {
            return Some(json);
        }
        let tsconfig = path.join("tsconfig.json");
        if tsconfig.is_file() {
            return Some(tsconfig);
        }
    }
    None
}

fn lexical_path(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                output.pop();
            }
            std::path::Component::CurDir => {}
            _ => output.push(component.as_os_str()),
        }
    }
    output
}
struct FencedFs {
    root: Option<PathBuf>,
}
impl FencedFs {
    fn check(&self, path: &Path) -> std::io::Result<()> {
        if self
            .root
            .as_ref()
            .is_some_and(|root| !lexical_path(path).starts_with(root))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "outside captured repository",
            ));
        }
        Ok(())
    }
}
impl FileSystem for FencedFs {
    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        self.check(path)?;
        FileSystemOs::read_to_string(path)
    }
    fn metadata(&self, path: &Path) -> std::io::Result<FileMetadata> {
        self.check(path)?;
        FileSystemOs::metadata(path)
    }
    fn symlink_metadata(&self, path: &Path) -> std::io::Result<FileMetadata> {
        self.check(path)?;
        FileSystemOs::symlink_metadata(path)
    }
    fn read_link(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.check(path)?;
        if self.root.is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "captured links are not followed",
            ));
        }
        FileSystemOs::read_link(path)
    }
}

fn is_esm_source(source_file: &Path, root: &Path) -> bool {
    match source_file
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mjs" | "mts" => return true,
        "cjs" | "cts" => return false,
        _ => {}
    }
    let mut directory = source_file.parent();
    while let Some(path) = directory {
        if let Some(value) = read_jsonc(&path.join("package.json")) {
            return value.get("type").and_then(serde_json::Value::as_str) == Some("module");
        }
        if path == root {
            break;
        }
        directory = path.parent();
        if directory.is_some_and(|parent| !parent.starts_with(root)) {
            break;
        }
    }
    false
}

fn nearest_config(directory: &Path, root: &Path) -> Option<PathBuf> {
    let mut current = directory;
    loop {
        for name in ["tsconfig.json", "jsconfig.json"] {
            let candidate = current.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if current == root {
            return None;
        }
        current = current.parent()?;
        if !current.starts_with(root) {
            return None;
        }
    }
}

pub fn attempted_candidates(error: &ResolveError) -> Vec<String> {
    match error {
        ResolveError::NotFound(path) => vec![path.clone()],
        ResolveError::MatchedAliasNotFound(specifier, alias) => {
            vec![format!("{alias} → {specifier}")]
        }
        ResolveError::PackagePathNotExported(subpath, package_json_path) => vec![
            subpath.clone(),
            package_json_path.to_string_lossy().into_owned(),
        ],
        ResolveError::PackageImportNotDefined(specifier, package_json_path) => vec![
            specifier.clone(),
            package_json_path.to_string_lossy().into_owned(),
        ],
        ResolveError::TsconfigNotFound(path) => vec![path.to_string_lossy().into_owned()],
        ResolveError::Json(error) => vec![error.path.to_string_lossy().into_owned()],
        _ => Vec::new(),
    }
}

pub fn diagnostic_hint(error: &ResolveError, specifier: &str) -> String {
    if specifier.starts_with("@/") || specifier.starts_with("~/") || specifier.starts_with("#/") {
        return "Спецификатор похож на alias, но подходящая цель не найдена. Проверьте tsconfig/jsconfig paths или статический resolve.alias; исполняемые конфиги Vite/Webpack DEPENDENCY не запускает.".into();
    }
    match error {
        ResolveError::MatchedAliasNotFound(..) => "Проверьте цели compilerOptions.paths/baseUrl в ближайшем tsconfig.json или jsconfig.json. Конфигурации бандлера не исполняются и пока не интерпретируются.".into(),
        ResolveError::PackagePathNotExported(..) => "Подпуть закрыт полем package.json exports. Используйте экспортируемый подпуть из публичного API пакета.".into(),
        ResolveError::PackageImportNotDefined(..) if specifier.starts_with('#') => "Добавьте соответствующий ключ в imports ближайшего package.json либо укажите относительный путь.".into(),
        ResolveError::TsconfigNotFound(_) | ResolveError::Json(_) => "Проверьте JSON/JSONC, extends и references в tsconfig.json/jsconfig.json.".into(),
        ResolveError::NotFound(_) if specifier.starts_with('.') || specifier.starts_with('/') => "Проверьте путь и регистр имени файла, расширение, index/package entry и то, что цель не исключена из сканирования.".into(),
        _ => "Проверьте конфигурацию разрешения модулей и наличие целевого файла/пакета. DEPENDENCY не выполняет конфигурационные скрипты.".into(),
    }
}
