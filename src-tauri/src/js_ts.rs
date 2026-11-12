use crate::language::SourceLanguage;
use crate::parse_facts::ImportDecl;
use tree_sitter::{Node, Parser};

pub(crate) fn parse_imports(
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
                if let Some(source_node) = node.child_by_field_name("source")
                    && let Some(specifier) = string_value(source_node, source)
                {
                    imports.push(ImportDecl {
                        specifier,
                        kind: if is_type_only_statement(node) {
                            "type_only_import"
                        } else {
                            "import"
                        }
                        .into(),
                        line: node.start_position().row + 1,
                        facts: None,
                    });
                }
            }
            "export_statement" => {
                if let Some(source_node) = node.child_by_field_name("source")
                    && let Some(specifier) = string_value(source_node, source)
                {
                    imports.push(ImportDecl {
                        specifier,
                        kind: if is_type_only_statement(node) {
                            "type_only_import"
                        } else {
                            "reexport"
                        }
                        .into(),
                        line: node.start_position().row + 1,
                        facts: None,
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
                            kind: kind.into(),
                            line: node.start_position().row + 1,
                            facts: None,
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

fn is_type_only_statement(node: Node) -> bool {
    fn has_type_token(node: Node) -> bool {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .any(|child| child.kind() == "type")
    }
    if has_type_token(node) {
        return true;
    }
    let mut cursor = node.walk();
    let Some(clause) = node
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "import_clause" | "export_clause"))
    else {
        return false;
    };
    let mut cursor = clause.walk();
    let children: Vec<_> = clause
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment")
        .collect();
    let specifiers = if clause.kind() == "import_clause" {
        if children.len() != 1 || children[0].kind() != "named_imports" {
            return false;
        }
        let mut cursor = children[0].walk();
        children[0]
            .named_children(&mut cursor)
            .filter(|child| child.kind() != "comment")
            .collect::<Vec<_>>()
    } else {
        children
    };
    !specifiers.is_empty()
        && specifiers
            .iter()
            .all(|specifier| has_type_token(*specifier))
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
