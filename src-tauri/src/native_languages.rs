//! Data-only language facts. Project discovery and resolution never execute code.
use crate::{
    language::SourceLanguage,
    parse_facts::{Fact, ImportDecl},
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
    let mut stack = vec![(tree.root_node(), Vec::<String>::new(), false)];
    while let Some((node, context, inherited)) = stack.pop() {
        let mut next_context = context.clone();
        let mut conditional = inherited
            || matches!(
                node.kind(),
                "if_statement" | "try_statement" | "match_statement" | "function_definition"
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
    fn python_facts_keep_relative_imports_conditions_and_dynamic_gaps() {
        let (facts,error)=parse("from ..pkg import thing as alias\nif TYPE_CHECKING:\n import typing\nimportlib.import_module(name)\n",SourceLanguage::Python).unwrap();
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
