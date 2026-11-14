use crate::{analysis, storage};
use std::fs;
fn write(root: &std::path::Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}
fn has(result: &analysis::AnalysisResult, source: &str, target: &str) -> bool {
    result
        .edges
        .iter()
        .any(|edge| edge.source == source && edge.target == target)
}
#[test]
fn python_from_imports_distinguish_submodules_attributes_and_reexports() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        (
            "pkg/__init__.py",
            "from .api import Service as Public\nservice = 42\nopen('EXECUTED', 'w').write('unsafe')\n",
        ),
        ("pkg/api.py", "class Service:\n pass\n"),
        ("pkg/service.py", ""),
        ("pkg/worker.py", ""),
        (
            "entry.py",
            "from pkg import worker as w, service, Public\nfrom pkg.api import Service\n",
        ),
        ("pkg/client.py", "from . import worker\n"),
    ] {
        write(root.path(), path, source);
    }
    let first = analysis::analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
    assert!(!root.path().join("EXECUTED").exists());
    assert!(has(&first.result, "entry.py", "pkg/worker.py"));
    assert!(has(&first.result, "entry.py", "pkg/__init__.py"));
    assert!(has(&first.result, "entry.py", "pkg/api.py"));
    assert!(has(&first.result, "pkg/__init__.py", "pkg/api.py"));
    assert!(has(&first.result, "pkg/client.py", "pkg/worker.py"));
    assert!(
        !has(&first.result, "entry.py", "pkg/service.py"),
        "an attribute takes precedence over a same-named submodule"
    );
    assert!(
        !first
            .result
            .summary
            .resolution_issues
            .iter()
            .any(|issue| matches!(
                issue.source.as_str(),
                "entry.py" | "pkg/__init__.py" | "pkg/client.py"
            )),
        "simple static bindings should not produce blanket unsupported warnings"
    );
    let db = tempfile::tempdir().unwrap();
    let state = storage::initialize(db.path().join("store.db")).unwrap();
    let saved = storage::save_snapshot(
        &state,
        first.result,
        first.file_cache,
        first.parsed_file_count,
        first.reused_file_count,
    )
    .unwrap();
    let old = serde_json::to_value(&saved.result).unwrap();
    let cached = storage::load_cache(&state, Some(&saved.project.id)).unwrap();
    let second = analysis::analyze_project(root.path().to_str().unwrap(), cached).unwrap();
    assert_eq!(second.parsed_file_count, 0);
    write(
        root.path(),
        "pkg/__init__.py",
        "from .api import Service as Public\n",
    );
    let changed =
        analysis::analyze_project(root.path().to_str().unwrap(), second.file_cache).unwrap();
    assert_eq!(changed.parsed_file_count, 1);
    assert!(has(&changed.result, "entry.py", "pkg/service.py"));
    let updated = storage::save_snapshot(
        &state,
        changed.result,
        changed.file_cache,
        changed.parsed_file_count,
        changed.reused_file_count,
    )
    .unwrap();
    assert_ne!(updated.snapshot.id, saved.snapshot.id);
    assert!(!root.path().join("EXECUTED").exists());
    assert_eq!(
        serde_json::to_value(
            storage::read_impact_snapshot(&state, &saved.snapshot.id)
                .unwrap()
                .1
        )
        .unwrap(),
        old
    );
}
#[test]
fn python_uncertain_bindings_do_not_guess_child_modules() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("conditional/__init__.py", "if FLAG:\n value = 1\n"),
        ("conditional/value.py", ""),
        ("dynamic/__init__.py", "def __getattr__(name):\n return 1\n"),
        ("dynamic/value.py", ""),
        ("wild/__init__.py", "from unknown import *\n"),
        ("wild/value.py", ""),
        ("cycle/__init__.py", "from .other import value\n"),
        ("cycle/other.py", "from . import value\n"),
        ("cycle/value.py", ""),
        (
            "entry.py",
            "from conditional import value\nfrom dynamic import value\nfrom wild import value\nfrom cycle import value\n",
        ),
    ] {
        write(root.path(), path, source);
    }
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    for package in ["conditional", "dynamic", "wild", "cycle"] {
        assert!(has(&result, "entry.py", &format!("{package}/__init__.py")));
        assert!(!has(&result, "entry.py", &format!("{package}/value.py")));
    }
    assert!(
        result
            .summary
            .resolution_issues
            .iter()
            .filter(|issue| issue.source == "entry.py")
            .count()
            >= 4
    );
}
#[test]
fn python_self_reexport_does_not_create_transitive_shortcuts() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        (
            "pkg/__init__.py",
            "from . import worker\nfrom .api import Service as Public\nimport pkg.worker as exported_worker\n",
        ),
        ("pkg/worker.py", ""),
        ("pkg/api.py", "class Service:\n pass\n"),
        (
            "entry.py",
            "from pkg import worker, Public, exported_worker\n",
        ),
    ] {
        write(root.path(), path, source);
    }
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    assert!(has(&result, "pkg/__init__.py", "pkg/worker.py"));
    assert!(has(&result, "pkg/__init__.py", "pkg/api.py"));
    assert!(has(&result, "entry.py", "pkg/__init__.py"));
    assert!(!has(&result, "entry.py", "pkg/api.py"));
    assert!(!has(&result, "entry.py", "pkg/worker.py"));
    assert!(result.summary.resolution_issues.is_empty());
}
#[test]
fn python_same_line_statements_and_dynamic_path_do_not_produce_guessed_children() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("pkg/__init__.py", "value=42; from . import value\n"),
        ("pkg/value.py", ""),
        ("loop/__init__.py", "for value in items:\n pass\n"),
        ("loop/value.py", ""),
        ("pathpkg/__init__.py", "__path__.append('other')\n"),
        ("pathpkg/value.py", ""),
        (
            "entry.py",
            "from pkg import value\nfrom loop import value\nfrom pathpkg import value\nimport pathpkg.value\n",
        ),
    ] {
        write(root.path(), path, source);
    }
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    for package in ["pkg", "loop", "pathpkg"] {
        assert!(!has(&result, "entry.py", &format!("{package}/value.py")));
    }
    assert!(!has(&result, "pkg/__init__.py", "pkg/value.py"));
    let old: crate::parse_facts::Fact = serde_json::from_str(
        r#"{"type":"python","module":"pkg","names":[],"relative":0,"conditional":false}"#,
    )
    .unwrap();
    assert!(matches!(
        old,
        crate::parse_facts::Fact::Python {
            statement: None,
            ..
        }
    ));
}
#[test]
fn python_implicit_attributes_are_not_submodule_dependencies() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("pkg/__init__.py", ""),
        ("pkg/__name__.py", ""),
        ("pkg/__path__.py", ""),
        ("entry.py", "from pkg import __name__, __path__\n"),
    ] {
        write(root.path(), path, source);
    }
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    assert!(has(&result, "entry.py", "pkg/__init__.py"));
    assert!(!has(&result, "entry.py", "pkg/__name__.py"));
    assert!(!has(&result, "entry.py", "pkg/__path__.py"));
    assert!(result.summary.resolution_issues.is_empty());
}
#[test]
fn python_313_declarations_have_bindings_and_preserve_cache_history() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("helper.py", "class Base:\n pass\n"),
        (
            "lib.py",
            "from helper import Base\nclass Box[T: Base = Base]:\n pass\ntype Alias[T = int] = list[T]\n",
        ),
        ("entry.py", "from lib import Box, Alias\n"),
        ("main.ts", "export const x=1;"),
        ("native.rs", "pub struct Native;\n"),
    ] {
        write(root.path(), path, source);
    }
    let first = analysis::analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
    assert_eq!(first.result.summary.parse_error_count, 0);
    assert!(first.result.summary.resolution_issues.is_empty());
    assert!(has(&first.result, "entry.py", "lib.py"));
    assert!(has(&first.result, "lib.py", "helper.py"));
    let mut cache = first.file_cache.clone();
    for file in &mut cache {
        if file.language == "Python" {
            file.analyzer_version = "python-parser-2".into();
        }
    }
    let reparsed = analysis::analyze_project(root.path().to_str().unwrap(), cache).unwrap();
    assert_eq!(reparsed.parsed_file_count, 3);
    assert_eq!(reparsed.reused_file_count, 2);
    let db = tempfile::tempdir().unwrap();
    let state = storage::initialize(db.path().join("store.db")).unwrap();
    let saved = storage::save_snapshot(
        &state,
        first.result,
        first.file_cache,
        first.parsed_file_count,
        first.reused_file_count,
    )
    .unwrap();
    let old = serde_json::to_value(&saved.result).unwrap();
    let second =
        analysis::analyze_project(root.path().to_str().unwrap(), reparsed.file_cache).unwrap();
    assert_eq!(second.parsed_file_count, 0);
    write(
        root.path(),
        "lib.py",
        "from helper import Base\nclass Box[T = ]:\n pass\n",
    );
    let broken =
        analysis::analyze_project(root.path().to_str().unwrap(), second.file_cache).unwrap();
    assert_eq!(broken.parsed_file_count, 1);
    assert_eq!(broken.result.summary.parse_error_count, 1);
    storage::save_snapshot(
        &state,
        broken.result,
        broken.file_cache,
        broken.parsed_file_count,
        broken.reused_file_count,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(
            storage::read_impact_snapshot(&state, &saved.snapshot.id)
                .unwrap()
                .1
        )
        .unwrap(),
        old
    );
}
#[test]
fn python_namespace_packages_resolve_only_unique_captured_children() {
    let root = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("ns/worker.py", ""),
        ("entry.py", "from ns import worker\n"),
        ("src/real/__init__.py", ""),
        ("src/real/item.py", ""),
        ("real/other.py", ""),
        ("regular.py", "from real import item\n"),
    ] {
        write(root.path(), path, source);
    }
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    assert!(has(&result, "entry.py", "ns/worker.py"));
    assert!(has(&result, "regular.py", "src/real/item.py"));
    assert!(!result.nodes.iter().any(|node| node.id == "ns/__init__.py"));
    write(root.path(), "src/ns/worker.py", "");
    let ambiguous = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    assert!(!has(&ambiguous, "entry.py", "ns/worker.py"));
    assert!(!has(&ambiguous, "entry.py", "src/ns/worker.py"));
    assert!(
        ambiguous
            .summary
            .resolution_issues
            .iter()
            .any(|issue| issue.source == "entry.py" && issue.status == "ambiguous")
    );
}
