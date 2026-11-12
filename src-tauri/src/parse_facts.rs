//! Shared parser output; independent of project discovery and resolution.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Fact {
    RustItem {
        context: Vec<String>,
        name: String,
        conditional: bool,
    },
    Python {
        module: String,
        names: Vec<String>,
        relative: usize,
        conditional: bool,
    },
    RustModule {
        context: Vec<String>,
        name: String,
        path: Option<String>,
        inline: bool,
        conditional: bool,
    },
    RustUse {
        context: Vec<String>,
        path: String,
        conditional: bool,
    },
    Gap {
        reason: String,
    },
}

#[derive(Clone)]
pub(crate) struct ImportDecl {
    pub(crate) facts: Option<Fact>,
    pub(crate) specifier: String,
    pub(crate) kind: String,
    pub(crate) line: usize,
}
