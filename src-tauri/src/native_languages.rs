//! Data-only language facts. Project discovery and resolution never execute code.
use crate::{
    language::SourceLanguage,
    parse_facts::{Fact, ImportDecl, PythonBinding},
};
use tree_sitter::{Node, Parser};

fn text<'a>(node: Node, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}
fn children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}
fn push(out: &mut Vec<ImportDecl>, node: Node, request: String, fact: Fact) {
    out.push(ImportDecl {
        specifier: request,
        kind: "import".into(),
        line: node.start_position().row + 1,
        facts: Some(fact),
    });
}

pub(crate) fn parse(
    source: &str,
    language: SourceLanguage,
) -> Result<(Vec<ImportDecl>, bool), String> {
    let mut parser = Parser::new();
    parser
        .set_language(&language.grammar())
        .map_err(|e| e.to_string())?;
    let tree = parser
        .parse(source, None)
        .ok_or("Parser returned no tree")?;
    let mut out = Vec::new();
    if matches!(language, SourceLanguage::Python) {
        python_bindings(tree.root_node(), source, &mut out);
    }
    let mut stack = vec![(tree.root_node(), Vec::<String>::new(), false)];
    while let Some((node, context, inherited)) = stack.pop() {
        let mut next_context = context.clone();
        let mut conditional = inherited
            || matches!(
                node.kind(),
                "if_statement"
                    | "try_statement"
                    | "match_statement"
                    | "function_definition"
                    | "for_statement"
                    | "while_statement"
                    | "with_statement"
            );
        match language {
            SourceLanguage::Python => match node.kind() {
                "import_statement" => {
                    for child in children(node) {
                        let request = if child.kind() == "aliased_import" {
                            child
                                .child_by_field_name("name")
                                .map(|n| text(n, source))
                                .unwrap_or("")
                        } else {
                            text(child, source)
                        };
                        if !request.is_empty() {
                            push(
                                &mut out,
                                node,
                                request.into(),
                                Fact::Python {
                                    statement: Some(node.start_byte()),
                                    module: request.into(),
                                    names: vec![],
                                    relative: 0,
                                    conditional,
                                },
                            );
                        }
                    }
                }
                "import_from_statement" => {
                    let module = node
                        .child_by_field_name("module_name")
                        .map(|n| text(n, source))
                        .unwrap_or("");
                    let relative = module.chars().take_while(|c| *c == '.').count();
                    let names = children(node)
                        .into_iter()
                        .filter(|n| Some(*n) != node.child_by_field_name("module_name"))
                        .filter(|n| {
                            matches!(
                                n.kind(),
                                "dotted_name" | "aliased_import" | "wildcard_import"
                            )
                        })
                        .map(|n| {
                            let n = n.child_by_field_name("name").unwrap_or(n);
                            text(n, source).to_string()
                        })
                        .collect();
                    push(
                        &mut out,
                        node,
                        text(node, source).into(),
                        Fact::Python {
                            statement: Some(node.start_byte()),
                            module: module.trim_start_matches('.').into(),
                            names,
                            relative,
                            conditional,
                        },
                    );
                }
                "call"
                    if node.child_by_field_name("function").is_some_and(|n| {
                        matches!(text(n, source), "__import__" | "importlib.import_module")
                    }) =>
                {
                    push(
                        &mut out,
                        node,
                        text(node, source).into(),
                        Fact::Gap {
                            reason: "Dynamic Python import is not evaluated".into(),
                        },
                    )
                }
                _ => {}
            },
            SourceLanguage::Rust => {
                let mut ancestor = node.parent();
                while let Some(parent) = ancestor {
                    if matches!(parent.kind(), "block" | "impl_item" | "trait_item") {
                        conditional = true;
                        break;
                    }
                    ancestor = parent.parent();
                }
                let mut attrs = Vec::new();
                let mut previous = node.prev_named_sibling();
                while let Some(p) = previous {
                    if p.kind() != "attribute_item" {
                        break;
                    }
                    attrs.push(text(p, source));
                    previous = p.prev_named_sibling();
                }
                conditional = conditional || attrs.iter().any(|a| a.contains("cfg"));
                match node.kind() {
                    "function_item" | "struct_item" | "enum_item" | "trait_item" | "const_item"
                    | "static_item" | "type_item" => {
                        let top = node.parent().is_some_and(|p| {
                            p.kind() == "source_file"
                                || p.kind() == "declaration_list"
                                    && p.parent().is_some_and(|p| p.kind() == "mod_item")
                        });
                        if top && let Some(name) = node.child_by_field_name("name") {
                            push(
                                &mut out,
                                node,
                                text(name, source).into(),
                                Fact::RustItem {
                                    context: context.clone(),
                                    name: text(name, source).into(),
                                    conditional,
                                },
                            );
                        }
                    }
                    "mod_item" => {
                        let name = node
                            .child_by_field_name("name")
                            .map(|n| text(n, source))
                            .unwrap_or("")
                            .trim_start_matches("r#")
                            .to_string();
                        let inline = node.child_by_field_name("body").is_some();
                        let path = attrs.iter().find_map(|a| {
                            let value = a.strip_prefix("#[path")?.trim().strip_prefix('=')?.trim();
                            serde_json::from_str::<String>(value.strip_suffix(']')?.trim()).ok()
                        });
                        if path.is_none() && attrs.iter().any(|a| a.starts_with("#[path")) {
                            push(&mut out,node,name,Fact::Gap {reason:"Rust path attribute is not a supported literal; default module path is not guessed".into()});
                            continue;
                        }
                        push(
                            &mut out,
                            node,
                            name.clone(),
                            Fact::RustModule {
                                context: context.clone(),
                                name: name.clone(),
                                path,
                                inline,
                                conditional,
                            },
                        );
                        if inline {
                            next_context.push(name);
                        }
                    }
                    "use_declaration" => {
                        if let Some(argument) = node.child_by_field_name("argument") {
                            for path in use_paths(argument, source, "") {
                                push(
                                    &mut out,
                                    node,
                                    path.clone(),
                                    Fact::RustUse {
                                        context: context.clone(),
                                        path,
                                        conditional,
                                    },
                                );
                            }
                        }
                    }
                    "macro_invocation" => push(
                        &mut out,
                        node,
                        text(node, source).chars().take(160).collect(),
                        Fact::Gap {
                            reason:
                                "Rust macros are not expanded; generated dependencies may be absent"
                                    .into(),
                        },
                    ),
                    _ => {}
                }
            }
            _ => return Err("Expected Rust or Python".into()),
        }
        for child in children(node).into_iter().rev() {
            stack.push((child, next_context.clone(), conditional));
        }
    }
    out.sort_by(|a, b| (a.line, &a.specifier).cmp(&(b.line, &b.specifier)));
    Ok((out, tree.root_node().has_error()))
}

// Module namespace facts are deliberately separate from dependency requests.
// Never descend into function/class bodies to discover module bindings.
fn python_bindings(root: Node, source: &str, out: &mut Vec<ImportDecl>) {
    fn record(
        out: &mut Vec<ImportDecl>,
        node: Node,
        name: &str,
        binding: PythonBinding,
        conditional: bool,
    ) {
        push(
            out,
            node,
            format!("#binding:{name}"),
            Fact::PythonBinding {
                name: name.into(),
                binding,
                conditional,
            },
        );
        out.last_mut().unwrap().kind = "binding".into();
    }
    let mut stack = vec![(root, false)];
    if root.has_error() {
        record(out, root, "*", PythonBinding::Unknown, true);
    }
    while let Some((node, inherited)) = stack.pop() {
        let conditional = inherited
            || matches!(
                node.kind(),
                "if_statement"
                    | "try_statement"
                    | "for_statement"
                    | "while_statement"
                    | "with_statement"
                    | "match_statement"
                    | "case_clause"
            );
        match node.kind() {
            "function_definition" | "class_definition" => {
                if let Some(name) = node.child_by_field_name("name") {
                    record(
                        out,
                        node,
                        text(name, source),
                        PythonBinding::Definition,
                        conditional,
                    );
                }
                continue;
            }
            "import_statement" => {
                for child in children(node) {
                    let name = child.child_by_field_name("name").unwrap_or(child);
                    let module = text(name, source);
                    let alias = child.child_by_field_name("alias").map(|n| text(n, source));
                    let bound = alias.unwrap_or(module.split('.').next().unwrap_or(module));
                    let requested = if alias.is_some() { module } else { bound };
                    record(
                        out,
                        node,
                        bound,
                        PythonBinding::Import {
                            statement: node.start_byte(),
                            module: requested.into(),
                            name: None,
                            relative: 0,
                        },
                        conditional,
                    );
                }
                continue;
            }
            "import_from_statement" => {
                let module_node = node.child_by_field_name("module_name");
                let module = module_node.map(|n| text(n, source)).unwrap_or("");
                let relative = module.chars().take_while(|c| *c == '.').count();
                for child in children(node)
                    .into_iter()
                    .filter(|n| Some(*n) != module_node)
                {
                    if child.kind() == "wildcard_import" {
                        record(out, node, "*", PythonBinding::Unknown, true);
                    } else if matches!(child.kind(), "dotted_name" | "aliased_import") {
                        let name = text(child.child_by_field_name("name").unwrap_or(child), source);
                        let alias = child
                            .child_by_field_name("alias")
                            .map(|n| text(n, source))
                            .unwrap_or(name);
                        record(
                            out,
                            node,
                            alias,
                            PythonBinding::Import {
                                statement: node.start_byte(),
                                module: module.trim_start_matches('.').into(),
                                name: Some(name.into()),
                                relative,
                            },
                            conditional,
                        );
                    }
                }
                continue;
            }
            "assignment" | "augmented_assignment" => {
                if let Some(left) = node.child_by_field_name("left") {
                    if left.kind() == "identifier" {
                        if node.child_by_field_name("right").is_some() {
                            record(
                                out,
                                node,
                                text(left, source),
                                if node.kind() == "assignment" {
                                    PythonBinding::Definition
                                } else {
                                    PythonBinding::Unknown
                                },
                                conditional,
                            );
                        }
                    } else {
                        record(out, node, "*", PythonBinding::Unknown, true);
                    }
                }
            }
            "type_alias_statement" => {
                if let Some(mut name) = node.child_by_field_name("left") {
                    while name.kind() != "identifier" && name.named_child_count() > 0 {
                        name = name.named_child(0).unwrap();
                    }
                    if name.kind() == "identifier" {
                        record(
                            out,
                            node,
                            text(name, source),
                            PythonBinding::Definition,
                            conditional,
                        );
                    } else {
                        record(out, node, "*", PythonBinding::Unknown, true);
                    }
                }
                continue;
            }
            "delete_statement" | "named_expression" | "for_statement" | "with_statement"
            | "match_statement" => record(out, node, "*", PythonBinding::Unknown, true),
            "call" => {
                if node.child_by_field_name("function").is_some_and(|n| {
                    matches!(
                        text(n, source),
                        "exec"
                            | "eval"
                            | "globals"
                            | "locals"
                            | "setattr"
                            | "__import__"
                            | "importlib.import_module"
                    )
                }) {
                    record(out, node, "*", PythonBinding::Unknown, true);
                }
                if node
                    .child_by_field_name("function")
                    .is_some_and(|n| text(n, source).starts_with("__path__."))
                {
                    record(out, node, "__path__", PythonBinding::Unknown, true);
                }
            }
            _ => {}
        }
        for child in children(node).into_iter().rev() {
            stack.push((child, conditional));
        }
    }
}

fn use_paths(node: Node, source: &str, prefix: &str) -> Vec<String> {
    match node.kind() {
        "use_list" => children(node)
            .into_iter()
            .flat_map(|n| use_paths(n, source, prefix))
            .collect(),
        "scoped_use_list" => {
            let path = node
                .child_by_field_name("path")
                .map(|n| text(n, source))
                .unwrap_or("");
            let joined = format!("{prefix}{path}::");
            node.child_by_field_name("list")
                .map(|n| use_paths(n, source, &joined))
                .unwrap_or_default()
        }
        "use_as_clause" => node
            .child_by_field_name("path")
            .map(|n| use_paths(n, source, prefix))
            .unwrap_or_default(),
        _ => vec![format!("{prefix}{}", text(node, source))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn python_313_type_defaults_preserve_imports_and_reject_broken_code() {
        let cases = [
            "class Box[T = int]:\n pass\n",
            "class Box[T: Bound = Default]:\n pass\n",
            "class Box[T: (int, str) = int]:\n pass\n",
            "class Box[*Ts = *tuple[int, str]]:\n pass\n",
            "class Box[**P = [int, str]]:\n pass\n",
            "def call[T = int, **P = ...](x: T) -> T:\n return x\n",
            "async def call[T = int](x: T) -> T:\n return x\n",
            "type Alias[T = int] = list[T]\n",
            "type Alias[*Ts = *tuple[int, str]] = tuple[*Ts]\n",
            "type Alias[**P = [int, str]] = Callable[P, int]\n",
            "class NewStyleGenerics[\n TypeVarDefault=int,\n *FollowingTuple=*tuple[int, ...] # noqa: WPS477\n]:\n pass\n",
        ];
        for case in cases {
            let source = format!("from .before import Value\n{case}from .after import Other\n");
            let (facts, error) = parse(&source, SourceLanguage::Python).unwrap();
            assert!(!error, "valid Python 3.13 declaration: {case}");
            let imports: Vec<_> = facts
                .iter()
                .filter(|fact| matches!(fact.facts, Some(Fact::Python { .. })))
                .collect();
            assert_eq!(imports.len(), 2);
            assert_eq!(imports[0].line, 1);
            assert_eq!(imports[1].line, source.lines().count());
        }
        for source in [
            "class Bad[T = ]:\n pass\n",
            "def bad[T = int](\n",
            "type Bad[T = int = str] = T\n",
            "type Bad = list[T = int]\n",
            "import pkg as\n",
            "def bad():\n  x = (\n",
        ] {
            assert!(
                parse(source, SourceLanguage::Python).unwrap().1,
                "broken syntax must not be hidden: {source}"
            );
        }
    }
    #[test]
    fn python_facts_keep_relative_imports_conditions_and_dynamic_gaps() {
        let (mut facts,error)=parse("from ..pkg import thing as alias\nif TYPE_CHECKING:\n import typing\nimportlib.import_module(name)\n",SourceLanguage::Python).unwrap();
        facts.retain(|fact| !matches!(fact.facts, Some(Fact::PythonBinding { .. })));
        assert!(!error);
        assert!(
            matches!(&facts[0].facts,Some(Fact::Python {relative:2,module,names,..}) if module=="pkg" && names==&vec!["thing".to_string()])
        );
        assert!(matches!(
            facts[1].facts,
            Some(Fact::Python {
                conditional: true,
                ..
            })
        ));
        assert!(matches!(facts[2].facts, Some(Fact::Gap { .. })));
    }
    #[test]
    fn rust_facts_keep_module_context_and_grouped_uses() {
        let (facts, error) = parse(
            "#[path = \"other.rs\"] mod foo;\nmod inline {use crate::{foo, bar::Thing as X};}\n",
            SourceLanguage::Rust,
        )
        .unwrap();
        assert!(!error);
        assert!(
            matches!(&facts[0].facts,Some(Fact::RustModule {path:Some(path),..}) if path=="other.rs")
        );
        assert!(facts.iter().any(|i|matches!(&i.facts,Some(Fact::RustUse {context,path,..}) if context==&vec!["inline".to_string()] && path=="crate::bar::Thing")));
    }
}
