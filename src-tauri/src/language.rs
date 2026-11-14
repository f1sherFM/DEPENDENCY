use std::path::Path;
use tree_sitter::Language;
pub const CACHE_VERSIONS: [&str; 3] = ["js-ts-parser-2", "rust-parser-1", "python-parser-3"];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisProfile {
    pub graph_semantics: String,
    pub analyzers: std::collections::BTreeMap<String, (String, String)>,
    pub python_roots: Vec<String>,
    #[serde(default)]
    pub excluded_paths: Vec<String>,
    pub limitations: Vec<String>,
}
pub fn profile(languages: &[&str], roots: &[String]) -> AnalysisProfile {
    let mut analyzers = std::collections::BTreeMap::new();
    let mut limitations = Vec::new();
    for language in languages {
        let (parser, resolver) = match *language {
            "Rust" => ("rust-parser-1", "rust-resolver-3"),
            "Python" => ("python-parser-3", "python-resolver-2"),
            _ => ("js-ts-parser-2", "js-ts-resolver-4"),
        };
        analyzers.insert(language.to_string(), (parser.into(), resolver.into()));
    }
    if analyzers.contains_key("Rust") {
        limitations.push("Rust: статическое дерево модулей; cfg, macros, generated code и полное разрешение символов не вычисляются".into());
    }
    if analyzers.contains_key("Python") {
        limitations.push("Python: файловые импорты в известных source roots; runtime path, distribution mapping и динамические импорты не вычисляются".into());
    }
    let mut python_roots = roots.to_vec();
    python_roots.sort();
    python_roots.dedup();
    AnalysisProfile {
        graph_semantics: "file-dependencies-2".into(),
        analyzers,
        python_roots,
        excluded_paths: Vec::new(),
        limitations,
    }
}
#[derive(Clone, Copy)]
pub(crate) enum SourceLanguage {
    JavaScript,
    TypeScript,
    Tsx,
    Rust,
    Python,
}

impl SourceLanguage {
    pub(crate) fn parser_version(self) -> &'static str {
        match self {
            Self::Rust => CACHE_VERSIONS[1],
            Self::Python => CACHE_VERSIONS[2],
            _ => CACHE_VERSIONS[0],
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Tsx => "TSX",
            Self::Rust => "Rust",
            Self::Python => "Python",
        }
    }
    pub(crate) fn grammar(self) -> Language {
        match self {
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }
}

pub(crate) fn language_for(path: &Path) -> Option<SourceLanguage> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "js" | "mjs" | "cjs" | "jsx" => Some(SourceLanguage::JavaScript),
        "ts" | "mts" | "cts" => Some(SourceLanguage::TypeScript),
        "tsx" => Some(SourceLanguage::Tsx),
        "rs" => Some(SourceLanguage::Rust),
        "py" => Some(SourceLanguage::Python),
        _ => None,
    }
}
