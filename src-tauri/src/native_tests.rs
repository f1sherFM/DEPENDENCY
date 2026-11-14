use crate::{analysis, storage};
use std::{fs, path::Path};
#[test]
fn cargo_workspace_path_inheritance_uses_defining_workspace_not_member_directory() {
    let root = fixture();
    write(
        root.path(),
        "Cargo.toml",
        "[workspace]\nmembers=['crates/*']\n[workspace.dependencies]\nlocal={package='helper',path='crates/helper'}\n",
    );
    write(
        root.path(),
        "crates/app/Cargo.toml",
        "[package]\nname='app'\nversion='0.1.0'\n[dependencies]\nlocal.workspace=true\n",
    );
    write(root.path(), "crates/app/src/lib.rs", "use local::Thing;\n");
    write(
        root.path(),
        "crates/helper/Cargo.toml",
        "[package]\nname='helper'\nversion='0.1.0'\n",
    );
    write(
        root.path(),
        "crates/helper/src/lib.rs",
        "pub struct Thing;\n",
    );
    let first = analysis::analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
    assert!(
        first
            .result
            .edges
            .iter()
            .any(|edge| edge.source == "crates/app/src/lib.rs"
                && edge.target == "crates/helper/src/lib.rs"
                && edge.kind == "rust_import")
    );
    let second =
        analysis::analyze_project(root.path().to_str().unwrap(), first.file_cache).unwrap();
    assert_eq!(second.parsed_file_count, 0);
    assert_eq!(
        serde_json::to_value(second.result.edges).unwrap(),
        serde_json::to_value(first.result.edges).unwrap()
    );
    // Explicit workspace outside the ancestor chain, but inside captured scope.
    write(
        root.path(),
        "shared/Cargo.toml",
        "[workspace]\nmembers=['../crates/app','helper']\n[workspace.dependencies]\nlocal={package='helper',path='helper'}\n",
    );
    write(
        root.path(),
        "shared/helper/Cargo.toml",
        "[package]\nname='helper'\nversion='0.1.0'\n",
    );
    write(
        root.path(),
        "shared/helper/src/lib.rs",
        "pub struct Thing;\n",
    );
    write(
        root.path(),
        "crates/app/Cargo.toml",
        "[package]\nname='app'\nversion='0.1.0'\nworkspace='../../shared'\n[dependencies]\nlocal.workspace=true\n",
    );
    let explicit =
        analysis::analyze_project(root.path().to_str().unwrap(), second.file_cache).unwrap();
    assert_eq!(explicit.parsed_file_count, 1); // Only the newly captured helper source.
    assert!(
        explicit
            .result
            .edges
            .iter()
            .any(|edge| edge.source == "crates/app/src/lib.rs"
                && edge.target == "shared/helper/src/lib.rs")
    );
    assert!(
        !explicit
            .result
            .edges
            .iter()
            .any(|edge| edge.source == "crates/app/src/lib.rs"
                && edge.target == "crates/helper/src/lib.rs")
    );
    write(
        root.path(),
        "shared/Cargo.toml",
        "[workspace]\nmembers=['../crates/app']\n",
    );
    let missing =
        analysis::analyze_project(root.path().to_str().unwrap(), explicit.file_cache).unwrap();
    assert_eq!(missing.parsed_file_count, 0);
    assert!(
        !missing
            .result
            .edges
            .iter()
            .any(|edge| edge.source == "crates/app/src/lib.rs")
    );
    assert!(
        missing
            .result
            .diagnostics
            .iter()
            .any(|issue| issue.explanation.contains("Workspace dependency `local`"))
    );
}
#[test]
fn rust_super_cannot_escape_the_crate_root() {
    let root = fixture();
    write(root.path(), "src/lib.rs", "pub struct Thing;\nmod child;\n");
    write(root.path(), "src/child.rs", "use super::super::Thing;\n");
    let output = analysis::analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
    assert!(
        !output
            .result
            .edges
            .iter()
            .any(|edge| edge.source == "src/child.rs"),
        "invalid super traversal must not create a confirmed dependency"
    );
    assert!(
        output
            .result
            .summary
            .resolution_issues
            .iter()
            .any(|issue| issue.source == "src/child.rs")
    );
}
#[test]
fn excluded_local_library_is_a_coverage_limit_not_a_parse_failure() {
    let root = fixture();
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname='app'\nversion='0.1.0'\n[dependencies]\nlocal={path='vendor/local'}\n",
    );
    write(
        root.path(),
        "vendor/local/Cargo.toml",
        "[package]\nname='local'\nversion='0.1.0'\n",
    );
    write(
        root.path(),
        "vendor/local/src/lib.rs",
        "pub struct Thing;\n",
    );
    write(root.path(), "src/lib.rs", "use local::Thing;\n");
    write(
        root.path(),
        "dependency.config.json",
        r#"{"analysis":{"exclude":["vendor/**"]}}"#,
    );
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    assert_eq!(result.summary.parse_error_count, 0);
    assert!(
        result
            .summary
            .resolution_issues
            .iter()
            .any(|issue| issue.source == "Cargo.toml"
                && issue.status == "unsupported"
                && issue.reason.contains("Path dependency"))
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "parse-error")
    );
    assert!(
        !result
            .nodes
            .iter()
            .any(|node| node.path.starts_with("vendor/"))
    );
}
fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in [
        (
            "Cargo.toml",
            "[package]\nname='mixed'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nserde='1'\n",
        ),
        (
            "src/lib.rs",
            "mod helpers;\nuse crate::helpers::Thing;\nuse serde::Serialize;\n",
        ),
        ("src/helpers.rs", "pub struct Thing;\n"),
        ("src/pkg/__init__.py", ""),
        ("src/pkg/service.py", "import pkg.data\n"),
        ("src/pkg/data.py", "VALUE=1\n"),
        (
            "test.py",
            "import pkg.service\nif TYPE_CHECKING:\n import pkg.data\n",
        ),
        ("main.ts", "import './util';"),
        ("util.ts", "export const x=1;"),
        (".venv/ignored.py", "import missing"),
    ] {
        write(root.path(), path, text);
    }
    root
}
#[test]
fn mixed_graph_has_real_targets_separate_relations_and_reuses_all_parsers() {
    let root = fixture();
    let first = analysis::analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
    assert!(
        !first
            .result
            .diagnostics
            .iter()
            .any(|d| d.rule_id == "parse-error")
    );
    assert!(
        first
            .result
            .analysis_profile
            .as_ref()
            .unwrap()
            .limitations
            .len()
            >= 2
    );
    let edge = |source: &str, target: &str, kind: &str| {
        first
            .result
            .edges
            .iter()
            .any(|e| e.source == source && e.target == target && e.kind == kind)
    };
    assert!(edge("src/lib.rs", "src/helpers.rs", "module_declaration"));
    assert!(edge("src/lib.rs", "src/helpers.rs", "rust_import"));
    assert!(first.result.edges.iter().any(|e| e.source == "src/lib.rs"
        && e.target.starts_with("package:cargo:")
        && e.kind == "package"));
    assert!(edge("test.py", "src/pkg/service.py", "python_import"));
    assert!(edge("test.py", "src/pkg/__init__.py", "python_import"));
    assert!(edge("test.py", "src/pkg/data.py", "conditional_import"));
    assert!(edge("main.ts", "util.ts", "import"));
    assert!(!first.result.nodes.iter().any(|n| n.id.contains(".venv")));
    assert!(
        !first
            .result
            .diagnostics
            .iter()
            .any(|d| d.rule_id == "no-known-consumers")
    );
    let count = first.file_cache.len();
    assert!(count >= 7);
    let db = tempfile::tempdir().unwrap();
    let state = storage::initialize(db.path().join("db.sqlite3")).unwrap();
    let saved = storage::save_snapshot(
        &state,
        first.result,
        first.file_cache,
        first.parsed_file_count,
        first.reused_file_count,
    )
    .unwrap();
    let second = analysis::analyze_project(
        root.path().to_str().unwrap(),
        storage::load_cache(&state, Some(&saved.project.id)).unwrap(),
    )
    .unwrap();
    assert_eq!(second.parsed_file_count, 0);
    assert_eq!(second.reused_file_count, count);
    let mut cache = second.file_cache;
    cache
        .iter_mut()
        .filter(|f| f.language == "Python")
        .for_each(|f| f.analyzer_version = "python-parser-1".into());
    let third = analysis::analyze_project(root.path().to_str().unwrap(), cache).unwrap();
    assert_eq!(third.parsed_file_count, 4);
    assert_eq!(third.reused_file_count, count - 4);
    let connection = rusqlite::Connection::open(&state.database_path).unwrap();
    connection.execute("UPDATE parsed_files SET analyzer_version='python-future', cache_json='{unreadable future format' WHERE project_id=?1 AND relative_path='test.py'",[&saved.project.id]).unwrap();
    assert_eq!(
        storage::load_cache(&state, Some(&saved.project.id))
            .unwrap()
            .len(),
        count - 1
    );
}
#[test]
fn python_ambiguity_and_attributes_never_select_arbitrary_file() {
    let root = fixture();
    write(root.path(), "pkg/__init__.py", "");
    write(root.path(), "pkg/service.py", "");
    write(root.path(), "from.py", "from pkg import service\n");
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    assert!(
        !result
            .edges
            .iter()
            .any(|e| e.source == "test.py" && e.target.ends_with("service.py"))
    );
    assert!(
        result
            .summary
            .resolution_issues
            .iter()
            .any(|i| i.source == "test.py" && i.status == "ambiguous")
    );
    assert!(
        !result
            .edges
            .iter()
            .any(|e| e.source == "from.py" && e.target.ends_with("service.py"))
    );
}
#[test]
fn unsafe_python_roots_fail_before_publishing_and_cfg_is_not_guessed() {
    let root = fixture();
    write(
        root.path(),
        "dependency.config.json",
        r#"{"python":{"sourceRoots":["../outside"]}}"#,
    );
    assert!(analysis::analyze_project(root.path().to_str().unwrap(), vec![]).is_err());
    fs::remove_file(root.path().join("dependency.config.json")).unwrap();
    write(
        root.path(),
        "src/lib.rs",
        "#[cfg(feature=\"a\")] mod helpers;\n",
    );
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    assert!(!result.edges.iter().any(|e| e.source == "src/lib.rs"));
    assert!(
        result
            .summary
            .resolution_issues
            .iter()
            .any(|i| i.reason.contains("cfg"))
    );
}

#[test]
fn manifests_keep_ecosystems_workspace_versions_and_requirements_includes_separate() {
    let root = fixture();
    write(
        root.path(),
        "Cargo.toml",
        "[workspace]\nmembers=['crates/*']\n[workspace.dependencies]\nserialize={package='serde',version='1.0'}\n",
    );
    write(
        root.path(),
        "crates/a/Cargo.toml",
        "[package]\nname='a'\nversion='0.1.0'\n[dependencies]\nserialize={workspace=true}\n",
    );
    write(
        root.path(),
        "crates/a/src/lib.rs",
        "use serialize::Serialize;",
    );
    write(
        root.path(),
        "pyproject.toml",
        "[project]\nname='example'\nversion='0.1.0'\ndependencies=['serde>=1','Pillow>=10']\n",
    );
    write(
        root.path(),
        "requirements.txt",
        "-r deps.txt\nrequests>=2\n",
    );
    write(root.path(), "deps.txt", "numpy>=1\n-r requirements.txt\n");
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    assert!(
        result
            .summary
            .packages
            .iter()
            .any(|p| p.name.starts_with("Cargo · serde") && p.version == "1.0")
    );
    assert!(
        result
            .summary
            .packages
            .iter()
            .any(|p| p.name.starts_with("PyPI · serde"))
    );
    assert!(
        result
            .summary
            .packages
            .iter()
            .any(|p| p.name.starts_with("PyPI · numpy"))
    );
    assert!(
        result
            .edges
            .iter()
            .any(|e| e.source == "crates/a/src/lib.rs"
                && e.target.starts_with("package:cargo:")
                && e.kind == "package")
    );
    assert!(
        !result
            .edges
            .iter()
            .any(|e| e.target.starts_with("package:pypi:"))
    );
}

#[test]
fn collisions_module_shadowing_and_unsupported_dynamic_imports_have_no_invented_targets() {
    let root = fixture();
    write(root.path(), "src/helpers/mod.rs", "pub struct Other;");
    write(root.path(), "src/pkg.py", "");
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    assert!(
        !result
            .edges
            .iter()
            .any(|e| e.source == "src/lib.rs" && e.kind == "module_declaration")
    );
    assert!(
        result
            .summary
            .resolution_issues
            .iter()
            .any(|i| i.source == "src/lib.rs" && i.status == "ambiguous")
    );
    assert!(
        !result
            .edges
            .iter()
            .any(|e| e.source == "test.py" && e.target.ends_with("service.py"))
    );
}

#[test]
fn mixed_git_snapshots_restore_compare_and_impact_without_executing_sentinels() {
    let root = fixture();
    let repo = git2::Repository::init(root.path()).unwrap();
    write(
        root.path(),
        "build.rs",
        "fn main(){std::fs::write(\"EXECUTED\",\"unsafe\").unwrap();}",
    );
    write(
        root.path(),
        "setup.py",
        "open('EXECUTED','w').write('unsafe')\n",
    );
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, "fixture", &tree, &[])
        .unwrap();
    let db = tempfile::tempdir().unwrap();
    let state = storage::initialize(db.path().join("db.sqlite3")).unwrap();
    let capture = crate::git::capture_commit(
        root.path().to_str().unwrap(),
        &oid.to_string(),
        &std::sync::atomic::AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    let (out, inventory) = crate::git_jobs::analyze_capture(
        &state,
        capture,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    let view = storage::save_captured_snapshot(
        &state,
        out.result,
        out.file_cache,
        out.parsed_file_count,
        out.reused_file_count,
        inventory,
    )
    .unwrap();
    assert!(!root.path().join("EXECUTED").exists());
    let restored = storage::load_snapshot(&state, &view.snapshot.id).unwrap();
    assert!(
        restored
            .result
            .analysis_profile
            .unwrap()
            .analyzers
            .contains_key("Python")
    );
    let facts = storage::read_inventory(&state, &view.snapshot.id).unwrap();
    assert!(
        facts
            .iter()
            .any(|f| f.path == "test.py" && f.imports.iter().any(|i| i.facts.is_some()))
    );
    let comparison = crate::comparison::Comparisons::default()
        .compare(&state, &view.snapshot.id, &view.snapshot.id)
        .unwrap();
    assert!(comparison.compatible);
    assert_eq!(comparison.total, 0);
    let impact = crate::impact::ImpactState::default()
        .get(
            &state,
            crate::impact::ImpactRequest {
                snapshot_id: view.snapshot.id,
                roots: vec!["src/helpers.rs".into()],
                edge_kinds: vec!["rust_import".into()],
                max_depth: None,
            },
        )
        .unwrap();
    assert_eq!(impact.known_affected_count, 1);
}

#[test]
fn unsupported_path_literals_and_external_requirements_do_not_fall_back() {
    let root = fixture();
    write(
        root.path(),
        "src/lib.rs",
        "#[path = r\"custom.rs\"] mod helpers;\n",
    );
    let output = analysis::analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
    assert!(!output.result.edges.iter().any(|e| e.source == "src/lib.rs"));
    assert!(
        output
            .result
            .summary
            .resolution_issues
            .iter()
            .any(|i| i.reason.contains("path attribute"))
    );
    write(root.path(), "requirements.txt", "-r ../outside.txt\n");
    assert!(analysis::analyze_project(root.path().to_str().unwrap(), vec![]).is_err());
}

#[test]
fn comparison_rejects_unknown_language_profile_without_reparsing_history() {
    let root = fixture();
    let mut output = analysis::analyze_project(root.path().to_str().unwrap(), vec![]).unwrap();
    git2::Repository::init(root.path()).unwrap();
    let capture = crate::git::capture_worktree(
        root.path().to_str().unwrap(),
        &std::sync::atomic::AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    output.result.basis = Some(capture.basis);
    let db = tempfile::tempdir().unwrap();
    let state = storage::initialize(db.path().join("db.sqlite3")).unwrap();
    let mut legacy: analysis::AnalysisResult =
        serde_json::from_value(serde_json::to_value(&output.result).unwrap()).unwrap();
    legacy.analysis_profile = None;
    let current = storage::save_snapshot(
        &state,
        output.result,
        output.file_cache,
        output.parsed_file_count,
        0,
    )
    .unwrap();
    let old = storage::save_snapshot(&state, legacy, vec![], 0, 0).unwrap();
    let summary = crate::comparison::Comparisons::default()
        .compare(&state, &old.snapshot.id, &current.snapshot.id)
        .unwrap();
    assert!(!summary.compatible);
    assert!(
        storage::load_snapshot(&state, &old.snapshot.id)
            .unwrap()
            .result
            .analysis_profile
            .is_none()
    );
}

#[test]
fn cargo_path_dependency_resolves_to_captured_library_item() {
    let root = fixture();
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname='a'\nversion='0.1.0'\n[dependencies]\nlocal={package='helper',path='crates/helper'}\n",
    );
    write(root.path(), "src/lib.rs", "use local::Thing;\n");
    write(
        root.path(),
        "crates/helper/Cargo.toml",
        "[package]\nname='helper'\nversion='0.1.0'\n",
    );
    write(
        root.path(),
        "crates/helper/src/lib.rs",
        "pub struct Thing;\n",
    );
    let result = analysis::analyze_project(root.path().to_str().unwrap(), vec![])
        .unwrap()
        .result;
    assert!(result.edges.iter().any(|e| e.source == "src/lib.rs"
        && e.target == "crates/helper/src/lib.rs"
        && e.kind == "rust_import"));
}
