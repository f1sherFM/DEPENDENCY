use crate::{comparison, git, git_jobs, snapshot, storage};
use git2::{Repository, Signature};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use tempfile::{TempDir, tempdir};

fn repository() -> (TempDir, Repository) {
    let dir = tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    (dir, repo)
}
fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn commit(repo: &Repository, paths: &[&str]) -> String {
    let mut index = repo.index().unwrap();
    for path in paths {
        index.add_path(Path::new(path)).unwrap();
    }
    index.write().unwrap();
    let oid = index.write_tree().unwrap();
    let tree = repo.find_tree(oid).unwrap();
    let signature = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents: Vec<_> = parent.iter().collect();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "fixture",
        &tree,
        &parents,
    )
    .unwrap()
    .to_string()
}
fn save(state: &storage::AppState, capture: git::Capture) -> storage::ProjectView {
    let (output, files) =
        git_jobs::analyze_capture(state, capture, &AtomicBool::new(false)).unwrap();
    storage::save_captured_snapshot(
        state,
        output.result,
        output.file_cache,
        output.parsed_file_count,
        output.reused_file_count,
        files,
    )
    .unwrap()
}
#[test]
fn status_is_read_only_and_distinguishes_staged_unstaged_untracked_and_unborn() {
    let (root, repo) = repository();
    let root_str = root.path().to_str().unwrap();
    assert!(git::status(root_str, 0, 200).unwrap().head_oid.is_none());
    write(root.path(), "a.ts", "export const a=1;");
    commit(&repo, &["a.ts"]);
    write(root.path(), "a.ts", "export const a=2;");
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.ts")).unwrap();
    index.write().unwrap();
    write(root.path(), "a.ts", "export const a=3;");
    write(root.path(), "untracked.ts", "export const b=1;");
    write(root.path(), ".gitignore", "ignored.ts\n");
    write(root.path(), "ignored.ts", "export const ignored=1;");
    let sentinel = root.path().join("EXECUTED");
    let hook = root.path().join(".git/hooks/sentinel.cmd");
    write(
        root.path(),
        ".git/hooks/sentinel.cmd",
        "@echo unsafe>EXECUTED\n",
    );
    let mut config = repo.config().unwrap();
    config
        .set_str("core.fsmonitor", hook.to_str().unwrap())
        .unwrap();
    config
        .set_str("filter.fixture.clean", hook.to_str().unwrap())
        .unwrap();
    config
        .set_str("diff.external", hook.to_str().unwrap())
        .unwrap();
    let before = fs::read(repo.path().join("index")).unwrap();
    let head = repo.head().unwrap().target();
    let status = git::status(root_str, 0, 200).unwrap();
    let a = status.items.iter().find(|row| row.path == "a.ts").unwrap();
    assert_eq!(a.staged, ["modified"]);
    assert_eq!(a.unstaged, ["modified"]);
    assert!(
        status
            .items
            .iter()
            .any(|row| row.path == "untracked.ts" && row.untracked)
    );
    assert!(!status.items.iter().any(|row| row.path == "ignored.ts"));
    assert_eq!(before, fs::read(repo.path().join("index")).unwrap());
    assert_eq!(head, repo.head().unwrap().target());
    assert!(!sentinel.exists());
    repo.set_head_detached(head.unwrap()).unwrap();
    assert!(git::status(root_str, 0, 1).unwrap().branch.is_none());
}
#[test]
fn commit_capture_is_immutable_and_resolver_cannot_read_host_config_or_packages() {
    let (root, repo) = repository();
    write(
        root.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["old/*"]}}}"#,
    );
    write(root.path(), "main.ts", "import '@/value';");
    write(root.path(), "old/value.ts", "export const value=1;");
    let first = commit(&repo, &["tsconfig.json", "main.ts", "old/value.ts"]);
    write(
        root.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["new/*"]}}}"#,
    );
    write(root.path(), "new/value.ts", "export const value=2;");
    commit(&repo, &["tsconfig.json", "new/value.ts"]);
    let db = tempdir().unwrap();
    let state = storage::initialize(db.path().join("db.sqlite3")).unwrap();
    let before_index = fs::read(repo.path().join("index")).unwrap();
    let before_head = repo.head().unwrap().target();
    let capture = git::capture_commit(
        root.path().to_str().unwrap(),
        &first,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    let physical = capture.directory.path().to_string_lossy().into_owned();
    let view = save(&state, capture);
    assert!(
        view.result
            .edges
            .iter()
            .any(|edge| edge.target == "old/value.ts")
    );
    assert!(
        !view
            .result
            .edges
            .iter()
            .any(|edge| edge.target == "new/value.ts")
    );
    assert!(
        !serde_json::to_string(&view.result)
            .unwrap()
            .contains(&physical)
    );
    assert_eq!(
        view.result.basis.as_ref().unwrap().commit_oid.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(before_index, fs::read(repo.path().join("index")).unwrap());
    assert_eq!(before_head, repo.head().unwrap().target());
    // Poison a staging parent, not the captured tree. FileSystem must deny all reads above its root.
    let outer = tempdir().unwrap();
    let captured = outer.path().join("capture");
    fs::create_dir(&captured).unwrap();
    write(
        outer.path(),
        "tsconfig-poison.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"evil":["host.ts"]}}}"#,
    );
    write(outer.path(), "host.ts", "export const x=1;");
    write(
        &captured,
        "tsconfig.json",
        r#"{"extends":"../tsconfig-poison.json"}"#,
    );
    write(&captured, "main.ts", "import 'evil';");
    let output = crate::analysis::analyze_input(
        captured.to_str().unwrap(),
        vec![],
        true,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(
        !output
            .result
            .edges
            .iter()
            .any(|edge| edge.target.contains("host.ts"))
    );
    assert!(
        output
            .result
            .summary
            .resolution_issues
            .iter()
            .any(|issue| issue.specifier == "evil")
    );
}
#[test]
fn worktree_capture_uses_actual_bytes_detects_races_and_preserves_latest() {
    let (root, repo) = repository();
    write(root.path(), "a.ts", "export const a=1;");
    let oid = commit(&repo, &["a.ts"]);
    write(root.path(), "a.ts", "export const a=2;");
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.ts")).unwrap();
    index.write().unwrap();
    write(root.path(), "a.ts", "export const a=3;");
    let capture = git::capture_worktree(
        root.path().to_str().unwrap(),
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(capture.directory.path().join("a.ts")).unwrap(),
        "export const a=3;"
    );
    let db = tempdir().unwrap();
    let state = storage::initialize(db.path().join("db.sqlite3")).unwrap();
    let work = save(&state, capture);
    let historical = save(
        &state,
        git::capture_commit(
            root.path().to_str().unwrap(),
            &oid,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    assert_eq!(work.project.id, historical.project.id);
    assert_eq!(
        storage::load_project(&state, &work.project.id)
            .unwrap()
            .snapshot
            .id,
        work.snapshot.id
    );
    let changed = git::capture_worktree(
        root.path().to_str().unwrap(),
        &AtomicBool::new(false),
        &|_| write(root.path(), "a.ts", "export const a=4;"),
    );
    assert!(changed.is_err());
    let capture = git::capture_commit(
        root.path().to_str().unwrap(),
        &oid,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    let temporary = capture.directory.path().to_path_buf();
    assert!(git_jobs::analyze_capture(&state, capture, &AtomicBool::new(true)).is_err());
    assert!(!temporary.exists());
    assert_eq!(
        storage::list_snapshots(&state, &work.project.id)
            .unwrap()
            .len(),
        2
    );
}
#[test]
fn links_lfs_windows_paths_and_missing_objects_are_explicit() {
    for path in [
        "../escape.ts",
        "/absolute.ts",
        "CON.ts",
        "a:stream",
        "a\\b",
        "bad./file",
        "a//b",
    ] {
        assert!(git::validate_path(path).is_err(), "accepted {path}");
    }
    let (root, repo) = repository();
    let blob = repo.blob(b"../outside").unwrap();
    let lfs = repo
        .blob(b"version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 1\n")
        .unwrap();
    let mut builder = repo.treebuilder(None).unwrap();
    builder.insert("link.ts", blob, 0o120000).unwrap();
    builder.insert("large.ts", lfs, 0o100644).unwrap();
    let tree = repo.find_tree(builder.write().unwrap()).unwrap();
    let sig = Signature::now("fixture", "fixture@example.invalid").unwrap();
    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, "links", &tree, &[])
        .unwrap();
    let capture = git::capture_commit(
        root.path().to_str().unwrap(),
        &oid.to_string(),
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert_eq!(capture.basis.omitted_count, 2);
    assert!(
        capture
            .files
            .iter()
            .all(|fact| fact.status == "unsupported")
    );
    assert!(!capture.directory.path().join("link.ts").exists());
    assert!(
        git::capture_commit(
            root.path().to_str().unwrap(),
            "0000000000000000000000000000000000000001",
            &AtomicBool::new(false),
            &|_| {}
        )
        .is_err()
    );
    let cancelled = AtomicBool::new(false);
    assert!(
        git::capture_commit(
            root.path().to_str().unwrap(),
            &oid.to_string(),
            &cancelled,
            &|_| cancelled.store(true, Ordering::Relaxed)
        )
        .is_err()
    );
}
#[test]
fn scope_case_collisions_and_resource_limits_do_not_publish_wrong_trees() {
    let (root, repo) = repository();
    write(root.path(), "packages/app/a.ts", "export const a=1;");
    write(root.path(), "outside.ts", "export const b=1;");
    let oid = commit(&repo, &["packages/app/a.ts", "outside.ts"]);
    let capture = git::capture_commit(
        root.path().join("packages/app").to_str().unwrap(),
        &oid,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert_eq!(capture.files.len(), 1);
    assert_eq!(capture.files[0].path, "a.ts");
    let blob = repo.blob(b"export const a=1;").unwrap();
    let mut builder = repo.treebuilder(None).unwrap();
    builder.insert("A.ts", blob, 0o100644).unwrap();
    builder.insert("a.ts", blob, 0o100644).unwrap();
    let tree = repo.find_tree(builder.write().unwrap()).unwrap();
    let sig = Signature::now("fixture", "fixture@example.invalid").unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, "case", &tree, &[&parent])
        .unwrap();
    assert!(
        git::capture_commit(
            root.path().to_str().unwrap(),
            &oid.to_string(),
            &AtomicBool::new(false),
            &|_| {}
        )
        .is_err()
    );
    write(root.path(), "huge.ts", &" ".repeat(10 * 1024 * 1024 + 1));
    assert!(
        git::capture_worktree(
            root.path().to_str().unwrap(),
            &AtomicBool::new(false),
            &|_| {}
        )
        .is_err()
    );
}
#[test]
fn schema_migration_and_inventory_write_rollback_keep_existing_snapshots() {
    let (root, repo) = repository();
    write(root.path(), "a.ts", "export const a=1;");
    commit(&repo, &["a.ts"]);
    let db = tempdir().unwrap();
    let path = db.path().join("db.sqlite3");
    let state = storage::initialize(path.clone()).unwrap();
    let first = save(
        &state,
        git::capture_worktree(
            root.path().to_str().unwrap(),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER deny_inventory BEFORE INSERT ON snapshot_files BEGIN SELECT RAISE(ABORT,'fixture rejection'); END;").unwrap();
    let capture = git::capture_worktree(
        root.path().to_str().unwrap(),
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    let (out, facts) = git_jobs::analyze_capture(&state, capture, &AtomicBool::new(false)).unwrap();
    assert!(
        storage::save_captured_snapshot(
            &state,
            out.result,
            out.file_cache,
            out.parsed_file_count,
            out.reused_file_count,
            facts
        )
        .is_err()
    );
    assert_eq!(
        storage::list_snapshots(&state, &first.project.id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        storage::load_project(&state, &first.project.id)
            .unwrap()
            .snapshot
            .id,
        first.snapshot.id
    );
    connection.execute_batch("DROP TRIGGER deny_inventory; DROP TABLE snapshot_files; ALTER TABLE snapshots DROP COLUMN basis_json; PRAGMA user_version=1;").unwrap();
    drop(connection);
    let migrated = storage::initialize(path.clone()).unwrap();
    assert_eq!(
        storage::load_snapshot(&migrated, &first.snapshot.id)
            .unwrap()
            .result
            .summary
            .source_file_count,
        1
    );
    assert_eq!(
        rusqlite::Connection::open(path)
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
}
#[test]
fn comparison_ignores_line_shifts_and_marks_unresolved_links_as_unconfirmed() {
    let (root, repo) = repository();
    write(root.path(), "a.ts", "import './b';");
    write(root.path(), "b.ts", "export const b=1;");
    let base = commit(&repo, &["a.ts", "b.ts"]);
    let db = tempdir().unwrap();
    let state = storage::initialize(db.path().join("db.sqlite3")).unwrap();
    let base = save(
        &state,
        git::capture_commit(
            root.path().to_str().unwrap(),
            &base,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    write(root.path(), "a.ts", "\n\nimport './b';");
    let shifted = save(
        &state,
        git::capture_worktree(
            root.path().to_str().unwrap(),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    let comparisons = comparison::Comparisons::default();
    let summary = comparisons
        .compare(&state, &base.snapshot.id, &shifted.snapshot.id)
        .unwrap();
    assert_eq!(summary.counts.get("edge"), None);
    fs::remove_file(root.path().join("b.ts")).unwrap();
    let missing = save(
        &state,
        git::capture_worktree(
            root.path().to_str().unwrap(),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    let summary = comparisons
        .compare(&state, &base.snapshot.id, &missing.snapshot.id)
        .unwrap();
    let page = comparisons.page(&summary.id, 0, 200, "edge", "").unwrap();
    assert!(
        page.items
            .iter()
            .any(|item| item.status == "no-longer-confirmed")
    );
    assert!(comparisons.page("expired", 0, 200, "", "").is_err());
}

#[test]
fn comparison_exact_moves_do_not_guess_between_duplicate_files_or_changed_rules() {
    let (root, repo) = repository();
    write(root.path(), "old.ts", "export const unique=1;");
    write(root.path(), "dup1.ts", "export const same=1;");
    write(root.path(), "dup2.ts", "export const same=1;");
    let oid = commit(&repo, &["old.ts", "dup1.ts", "dup2.ts"]);
    let db = tempdir().unwrap();
    let state = storage::initialize(db.path().join("db.sqlite3")).unwrap();
    let base = save(
        &state,
        git::capture_commit(
            root.path().to_str().unwrap(),
            &oid,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    fs::rename(root.path().join("old.ts"), root.path().join("new.ts")).unwrap();
    fs::rename(root.path().join("dup2.ts"), root.path().join("dup3.ts")).unwrap();
    let target = save(
        &state,
        git::capture_worktree(
            root.path().to_str().unwrap(),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    let comparisons = comparison::Comparisons::default();
    let summary = comparisons
        .compare(&state, &base.snapshot.id, &target.snapshot.id)
        .unwrap();
    let page = comparisons.page(&summary.id, 0, 200, "file", "").unwrap();
    assert!(
        page.items
            .iter()
            .any(|item| item.status == "moved-exact" && item.label == "old.ts → new.ts")
    );
    assert!(
        page.items
            .iter()
            .filter(|item| item.label.starts_with("dup"))
            .all(|item| item.status == "ambiguous")
    );
    write(
        root.path(),
        "dependency.config.json",
        r#"{"rules":{"high-fan-out":{"threshold":1}}}"#,
    );
    let configured = save(
        &state,
        git::capture_worktree(
            root.path().to_str().unwrap(),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    assert!(
        !comparisons
            .compare(&state, &base.snapshot.id, &configured.snapshot.id)
            .unwrap()
            .diagnostics_comparable
    );
}
#[test]
fn config_only_change_reuses_ast_and_changes_resolution_without_removing_imports() {
    let (root, repo) = repository();
    write(
        root.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["one/*"]}}}"#,
    );
    write(root.path(), "main.ts", "import '@/value';");
    write(root.path(), "one/value.ts", "export const x=1;");
    write(root.path(), "two/value.ts", "export const x=2;");
    let oid = commit(
        &repo,
        &["tsconfig.json", "main.ts", "one/value.ts", "two/value.ts"],
    );
    let db = tempdir().unwrap();
    let state = storage::initialize(db.path().join("db.sqlite3")).unwrap();
    let base = save(
        &state,
        git::capture_commit(
            root.path().to_str().unwrap(),
            &oid,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    write(
        root.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["two/*"]}}}"#,
    );
    let target = save(
        &state,
        git::capture_worktree(
            root.path().to_str().unwrap(),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    assert_eq!(target.snapshot.parsed_file_count, 0);
    assert_eq!(target.snapshot.reused_file_count, 3);
    let comparisons = comparison::Comparisons::default();
    let summary = comparisons
        .compare(&state, &base.snapshot.id, &target.snapshot.id)
        .unwrap();
    let page = comparisons.page(&summary.id, 0, 200, "edge", "").unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(
        page.items
            .iter()
            .all(|item| item.status != "removed-import")
    );
    assert!(
        page.items
            .iter()
            .any(|item| item.status == "resolved-added")
    );
}
#[test]
fn linked_and_nested_worktrees_have_explicit_identity_and_scope() {
    let (root, repo) = repository();
    write(root.path(), "a.ts", "export const a=1;");
    commit(&repo, &["a.ts"]);
    let parent = tempdir().unwrap();
    let linked = parent.path().join("linked");
    repo.worktree("linked", &linked, None).unwrap();
    let original = git::capture_worktree(
        root.path().to_str().unwrap(),
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    let linked_capture =
        git::capture_worktree(linked.to_str().unwrap(), &AtomicBool::new(false), &|_| {}).unwrap();
    assert_eq!(
        original.basis.repository_identity,
        linked_capture.basis.repository_identity
    );
    assert_ne!(original.logical_root, linked_capture.logical_root);
    let nested = root.path().join("nested");
    let inner = Repository::init(&nested).unwrap();
    write(&nested, "inner.ts", "export const inner=1;");
    commit(&inner, &["inner.ts"]);
    let captured = git::capture_worktree(
        root.path().to_str().unwrap(),
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert!(
        captured
            .files
            .iter()
            .any(|file| file.path == "nested" && file.status == "unsupported")
    );
    assert!(
        !captured
            .files
            .iter()
            .any(|file| file.path == "nested/inner.ts")
    );
}
#[test]
fn job_cancel_has_a_clear_commit_point_and_leaves_no_cancelled_snapshot() {
    let (root, repo) = repository();
    write(root.path(), "a.ts", "export const a=1;");
    commit(&repo, &["a.ts"]);
    let db = tempdir().unwrap();
    let state = storage::initialize(db.path().join("db.sqlite3")).unwrap();
    let jobs = git_jobs::GitJobs::default();
    let id = jobs
        .start(
            state.clone(),
            root.path().to_string_lossy().into_owned(),
            None,
        )
        .unwrap();
    let accepted = jobs.cancel(&id).is_ok();
    let started = std::time::Instant::now();
    let final_state = loop {
        let status = jobs.status(&id).unwrap();
        if status.done {
            break status;
        }
        assert!(started.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    if accepted {
        assert_eq!(final_state.phase, "cancelled");
        assert!(final_state.snapshot_id.is_none());
        assert!(storage::list_projects(&state).unwrap().is_empty());
    } else {
        assert_eq!(final_state.phase, "complete");
        assert!(final_state.snapshot_id.is_some());
    }
}
#[test]
fn conflicted_index_is_reported_without_rewriting_it() {
    let (root, repo) = repository();
    write(root.path(), "conflict.ts", "export const value=1;");
    commit(&repo, &["conflict.ts"]);
    let mut index = repo.index().unwrap();
    let mut entry = index.get_path(Path::new("conflict.ts"), 0).unwrap();
    index.remove_path(Path::new("conflict.ts")).unwrap();
    for stage in 1..=3 {
        entry.flags = (entry.flags & !0x3000) | (stage << 12);
        index.add(&entry).unwrap();
    }
    index.write().unwrap();
    let before = fs::read(repo.path().join("index")).unwrap();
    let status = git::status(root.path().to_str().unwrap(), 0, 200).unwrap();
    assert!(
        status
            .items
            .iter()
            .any(|row| row.path == "conflict.ts" && row.conflicted)
    );
    assert_eq!(before, fs::read(repo.path().join("index")).unwrap());
}

#[test]
fn commit_only_project_restores_and_impact_uses_saved_graph_without_sources() {
    let (root, repo) = repository();
    write(root.path(), "a.ts", "import './b';");
    write(root.path(), "b.ts", "export const b=1;");
    let oid = commit(&repo, &["a.ts", "b.ts"]);
    let db = tempdir().unwrap();
    let database = db.path().join("db.sqlite3");
    let state = storage::initialize(database.clone()).unwrap();
    let view = save(
        &state,
        git::capture_commit(
            root.path().to_str().unwrap(),
            &oid,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    assert!(view.project.latest_snapshot_id.is_none());
    fs::remove_file(root.path().join("a.ts")).unwrap();
    fs::remove_file(root.path().join("b.ts")).unwrap();
    let reopened = storage::initialize(database).unwrap();
    let restored = storage::load_project(&reopened, &view.project.id).unwrap();
    assert_eq!(restored.snapshot.id, view.snapshot.id);
    assert_eq!(restored.snapshot.basis.unwrap().commit_oid, Some(oid));
    let impact = crate::impact::ImpactState::default()
        .get(
            &reopened,
            crate::impact::ImpactRequest {
                snapshot_id: view.snapshot.id,
                roots: vec!["b.ts".into()],
                edge_kinds: crate::impact::EDGE_KINDS
                    .iter()
                    .map(|kind| (*kind).into())
                    .collect(),
                max_depth: None,
            },
        )
        .unwrap();
    assert_eq!(impact.known_affected_count, 1);
    assert_eq!(impact.first_page.items[0].node_id, "a.ts");
}

#[test]
#[ignore = "explicit Git oracle / end-to-end fixture export"]
fn export_git_ui_and_check_local_oracle() {
    let (root, repo) = repository();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diagnostics");
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(&source)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(&source)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        write(
            root.path(),
            &relative,
            &fs::read_to_string(entry.path()).unwrap(),
        );
        paths.push(relative);
    }
    if let Ok(extra) = std::env::var("DEPENDENCY_MIXED_FIXTURE") {
        for entry in walkdir::WalkDir::new(&extra)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            let relative = entry
                .path()
                .strip_prefix(&extra)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            write(
                root.path(),
                &relative,
                &fs::read_to_string(entry.path()).unwrap(),
            );
            paths.push(relative);
        }
    }
    let refs: Vec<_> = paths.iter().map(String::as_str).collect();
    let first = commit(&repo, &refs);
    write(root.path(), "src/main.ts", "import './shared';\n");
    let second = commit(&repo, &["src/main.ts"]);
    write(root.path(), "src/main.ts", "import './shared';\n\n");
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("src/main.ts")).unwrap();
    index.write().unwrap();
    write(root.path(), "src/main.ts", "import './shared';\n\n\n");
    write(root.path(), "untracked.txt", "fixture");
    let oracle = std::process::Command::new("git")
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-C",
            root.path().to_str().unwrap(),
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_CONFIG_COUNT")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .output()
        .unwrap();
    assert!(oracle.status.success());
    let oracle = String::from_utf8(oracle.stdout).unwrap();
    assert!(oracle.contains("MM src/main.ts"));
    assert!(oracle.contains("?? untracked.txt"));
    let db = tempdir().unwrap();
    let state = storage::initialize(db.path().join("db.sqlite3")).unwrap();
    let base = save(
        &state,
        git::capture_commit(
            root.path().to_str().unwrap(),
            &first,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    let commit_view = save(
        &state,
        git::capture_commit(
            root.path().to_str().unwrap(),
            &second,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    let target = save(
        &state,
        git::capture_worktree(
            root.path().to_str().unwrap(),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap(),
    );
    let comparisons = comparison::Comparisons::default();
    let summary = comparisons
        .compare(&state, &base.snapshot.id, &target.snapshot.id)
        .unwrap();
    let page = comparisons.page(&summary.id, 0, 200, "", "").unwrap();
    let status = git::status(root.path().to_str().unwrap(), 0, 200).unwrap();
    let refs = git::refs(root.path().to_str().unwrap(), 0, 100).unwrap();
    let commits = git::commits(root.path().to_str().unwrap(), "HEAD", 0, 100).unwrap();
    let snapshots = storage::snapshot_page(&state, &target.project.id, 0, 200).unwrap();
    let inventory = storage::inventory_page(&state, &target.snapshot.id, 0, 200).unwrap();
    let inventories = [&base, &commit_view, &target]
        .into_iter()
        .map(|view| {
            (
                view.snapshot.id.clone(),
                storage::inventory_page(&state, &view.snapshot.id, 0, 200).unwrap(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let bundle = serde_json::json!({"analysis":target.result,"snapshot":target.snapshot,"git":{"status":status,"refs":refs,"commits":commits,"snapshots":snapshots,"inventory":inventory,"inventories":inventories,"comparison":summary,"changes":page,"baseSnapshotId":base.snapshot.id,"targetSnapshotId":target.snapshot.id,"commitView":commit_view,"views":[base,target,commit_view]}});
    if let Ok(directory) = std::env::var("DEPENDENCY_DUMP_DIR") {
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            Path::new(&directory).join("git-ui.json"),
            serde_json::to_vec(&bundle).unwrap(),
        )
        .unwrap();
    }
    println!(
        "Git oracle checked; refs={}, snapshots={}, comparison rows={}, timestamp={}",
        repo.references().unwrap().count(),
        storage::list_snapshots(&state, &commit_view.project.id)
            .unwrap()
            .len(),
        summary.total,
        snapshot::timestamp()
    );
    if let Ok(roots) = std::env::var("DEPENDENCY_PROFILE_ROOTS") {
        for root in roots.split(';').filter(|root| !root.is_empty()) {
            let database = tempdir().unwrap();
            let state = storage::initialize(database.path().join("db.sqlite3")).unwrap();
            let started = std::time::Instant::now();
            let committed = save(
                &state,
                git::capture_commit(root, "HEAD", &AtomicBool::new(false), &|_| {}).unwrap(),
            );
            let commit_ms = started.elapsed().as_millis();
            let started = std::time::Instant::now();
            let worktree = save(
                &state,
                git::capture_worktree(root, &AtomicBool::new(false), &|_| {}).unwrap(),
            );
            let worktree_ms = started.elapsed().as_millis();
            let started = std::time::Instant::now();
            let comparison = comparisons
                .compare(&state, &committed.snapshot.id, &worktree.snapshot.id)
                .unwrap();
            println!(
                "REAL {root}: commit={commit_ms}ms worktree={worktree_ms}ms compare={}ms files={} imports={} parsed={} reused={} changes={}",
                started.elapsed().as_millis(),
                worktree.result.summary.source_file_count,
                worktree.result.summary.dependency_count,
                worktree.snapshot.parsed_file_count,
                worktree.snapshot.reused_file_count,
                comparison.total
            );
        }
    }
}
