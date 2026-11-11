use crate::snapshot::{self, Basis, FileFact};
use git2::{Repository, StatusOptions};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

const ENTRY_LIMIT: usize = 100_000;
const FILE_LIMIT: u64 = 10 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 512 * 1024 * 1024;
pub fn discover(root: &str) -> Result<bool, String> {
    match Repository::discover(root) {
        Ok(_) => {
            Context::open(root)?;
            Ok(true)
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(false),
        Err(e) => Err(error(e)),
    }
}
fn error(error: impl std::fmt::Display) -> String {
    format!("Git: {error}")
}
pub struct Context {
    pub repository: Repository,
    pub logical_root: PathBuf,
    pub workdir: PathBuf,
    pub scope: String,
}
impl Context {
    pub fn open(root: &str) -> Result<Self, String> {
        let logical_root = fs::canonicalize(root).map_err(error)?;
        let repository = Repository::discover(&logical_root).map_err(|e| {
            format!("Локальный Git-репозиторий не найден или не поддерживается: {e}")
        })?;
        let workdir = repository
            .workdir()
            .ok_or("Выберите рабочий каталог, bare repository пока не поддерживается")?;
        let workdir = fs::canonicalize(workdir).map_err(error)?;
        let scope = logical_root
            .strip_prefix(&workdir)
            .map_err(error)?
            .to_string_lossy()
            .replace('\\', "/");
        Ok(Self {
            repository,
            logical_root,
            workdir,
            scope,
        })
    }
    fn relative(&self, path: &str) -> Option<String> {
        if self.scope.is_empty() {
            Some(path.into())
        } else {
            path.strip_prefix(&format!("{}/", self.scope))
                .map(str::to_string)
        }
    }
    fn head(&self) -> Option<String> {
        self.repository
            .head()
            .ok()
            .and_then(|head| head.target())
            .map(|oid| oid.to_string())
    }
    fn branch(&self) -> Option<String> {
        self.repository
            .head()
            .ok()
            .filter(|head| head.is_branch())
            .and_then(|head| head.shorthand().ok().map(str::to_string))
    }
    fn index_fingerprint(&self) -> Result<String, String> {
        let mut index = self.repository.index().map_err(error)?;
        index.read(true).map_err(error)?;
        let mut bytes = Vec::new();
        for entry in index.iter() {
            bytes.extend_from_slice(&entry.path);
            bytes.push(0);
            bytes.extend_from_slice(entry.id.as_bytes());
            bytes.extend_from_slice(&entry.mode.to_le_bytes());
            bytes.extend_from_slice(&entry.flags.to_le_bytes());
            bytes.extend_from_slice(&entry.flags_extended.to_le_bytes());
        }
        Ok(snapshot::hash(&bytes))
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusRow {
    pub path: String,
    pub staged: Vec<String>,
    pub unstaged: Vec<String>,
    pub untracked: bool,
    pub conflicted: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub root: String,
    pub repository_root: String,
    pub scope: String,
    pub head_oid: Option<String>,
    pub branch: Option<String>,
    pub observed_at: u64,
    pub items: Vec<StatusRow>,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub total: usize,
}
fn status_rows(context: &Context) -> Result<Vec<StatusRow>, String> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_unreadable(true)
        .exclude_submodules(true)
        .no_refresh(true)
        .update_index(false)
        .renames_head_to_index(false)
        .renames_index_to_workdir(false);
    let statuses = context
        .repository
        .statuses(Some(&mut options))
        .map_err(error)?;
    if statuses.len() > ENTRY_LIMIT {
        return Err("Git status превышает 100 000 записей".into());
    }
    let mut rows = Vec::new();
    for entry in statuses.iter() {
        let path = entry.path().map_err(error)?;
        let Some(path) = context.relative(path) else {
            continue;
        };
        let status = entry.status();
        let staged = [
            (git2::Status::INDEX_NEW, "added"),
            (git2::Status::INDEX_MODIFIED, "modified"),
            (git2::Status::INDEX_DELETED, "deleted"),
            (git2::Status::INDEX_RENAMED, "renamed"),
            (git2::Status::INDEX_TYPECHANGE, "type-change"),
        ]
        .into_iter()
        .filter(|(flag, _)| status.contains(*flag))
        .map(|(_, label)| label.into())
        .collect();
        let unstaged = [
            (git2::Status::WT_MODIFIED, "modified"),
            (git2::Status::WT_DELETED, "deleted"),
            (git2::Status::WT_RENAMED, "renamed"),
            (git2::Status::WT_TYPECHANGE, "type-change"),
            (git2::Status::WT_UNREADABLE, "unreadable"),
        ]
        .into_iter()
        .filter(|(flag, _)| status.contains(*flag))
        .map(|(_, label)| label.into())
        .collect();
        rows.push(StatusRow {
            path,
            staged,
            unstaged,
            untracked: status.is_wt_new(),
            conflicted: status.is_conflicted(),
        });
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(rows)
}
pub fn status(root: &str, offset: usize, limit: usize) -> Result<GitStatus, String> {
    page_limit(limit)?;
    let context = Context::open(root)?;
    let rows = status_rows(&context)?;
    if offset > rows.len() {
        return Err("Страница Git status вне диапазона".into());
    }
    let total = rows.len();
    let items = rows
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let next = offset + items.len();
    Ok(GitStatus {
        root: display(&context.logical_root),
        repository_root: display(&context.workdir),
        scope: context.scope.clone(),
        head_oid: context.head(),
        branch: context.branch(),
        observed_at: snapshot::timestamp(),
        items,
        offset,
        next_offset: (next < total).then_some(next),
        total,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefRow {
    pub name: String,
    pub oid: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRow {
    pub oid: String,
    pub summary: String,
    pub timestamp: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_offset: Option<usize>,
}
pub fn refs(root: &str, offset: usize, limit: usize) -> Result<Page<RefRow>, String> {
    page_limit(limit)?;
    let context = Context::open(root)?;
    let mut rows = Vec::new();
    for reference in context.repository.references().map_err(error)? {
        let reference = reference.map_err(error)?;
        if let (Ok(name), Ok(commit)) = (reference.name(), reference.peel_to_commit()) {
            rows.push(RefRow {
                name: name.into(),
                oid: commit.id().to_string(),
            });
        }
        if rows.len() > ENTRY_LIMIT {
            return Err("Слишком много Git refs".into());
        }
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    let total = rows.len();
    let items = rows
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let next = offset + items.len();
    Ok(Page {
        items,
        next_offset: (next < total).then_some(next),
    })
}
pub fn commits(
    root: &str,
    reference: &str,
    offset: usize,
    limit: usize,
) -> Result<Page<CommitRow>, String> {
    page_limit(limit)?;
    if offset > ENTRY_LIMIT {
        return Err("История ограничена 100 000 коммитов от выбранной ссылки".into());
    }
    let context = Context::open(root)?;
    let commit = resolve_commit(&context, reference)?;
    let mut walk = context.repository.revwalk().map_err(error)?;
    walk.push(commit.id()).map_err(error)?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .map_err(error)?;
    let mut rows = Vec::new();
    for oid in walk.skip(offset).take(limit + 1) {
        let commit = context
            .repository
            .find_commit(oid.map_err(error)?)
            .map_err(error)?;
        rows.push(CommitRow {
            oid: commit.id().to_string(),
            summary: commit
                .summary()
                .ok()
                .flatten()
                .unwrap_or("(без сообщения)")
                .into(),
            timestamp: commit.time().seconds(),
        });
    }
    let more = rows.len() > limit;
    rows.truncate(limit);
    let next = offset + rows.len();
    Ok(Page {
        items: rows,
        next_offset: more.then_some(next),
    })
}
fn resolve_commit<'a>(context: &'a Context, reference: &str) -> Result<git2::Commit<'a>, String> {
    if reference.is_empty() || reference.len() > 4096 {
        return Err("Укажите локальную Git-ссылку или commit OID".into());
    }
    context
        .repository
        .revparse_single(reference)
        .and_then(|object| object.peel_to_commit())
        .map_err(|e| format!("Коммит не найден в локальных объектах (fetch не выполняется): {e}"))
}
pub fn page_limit(limit: usize) -> Result<(), String> {
    if !(1..=200).contains(&limit) {
        Err("Размер страницы должен быть 1..200".into())
    } else {
        Ok(())
    }
}
pub fn display(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string()
}

pub struct Capture {
    pub directory: tempfile::TempDir,
    pub logical_root: PathBuf,
    pub basis: Basis,
    pub files: Vec<FileFact>,
}
struct Writer {
    capture: Capture,
    names: HashMap<String, String>,
    total: u64,
}
impl Writer {
    fn new(context: &Context, reference: Option<&str>) -> Result<Self, String> {
        let basis = Basis {
            kind: if reference.is_some() {
                "git-commit"
            } else {
                "git-worktree"
            }
            .into(),
            repository_identity: Some(display(
                &fs::canonicalize(context.repository.commondir()).map_err(error)?,
            )),
            project_scope: context.scope.clone(),
            requested_ref: reference.map(str::to_string),
            commit_oid: None,
            tree_oid: None,
            observed_head_oid: context.head(),
            branch_label: if reference.is_none() {
                context.branch()
            } else {
                None
            },
            index_fingerprint: Some(context.index_fingerprint()?),
            capture_fingerprint: String::new(),
            started_at: snapshot::timestamp(),
            completed_at: 0,
            capture_consistency: "capturing".into(),
            resolution_profile: snapshot::PROFILE.into(),
            capture_policy_hash: snapshot::hash(snapshot::POLICY.as_bytes()),
            omitted_count: 0,
        };
        Ok(Self {
            capture: Capture {
                directory: tempfile::tempdir().map_err(error)?,
                logical_root: context.logical_root.clone(),
                basis,
                files: Vec::new(),
            },
            names: HashMap::new(),
            total: 0,
        })
    }
    fn add(
        &mut self,
        path: &str,
        mode: u32,
        oid: Option<String>,
        bytes: Option<&[u8]>,
        reason: Option<String>,
    ) -> Result<(), String> {
        validate_path(path)?;
        let mut prefix = String::new();
        for part in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            let key = prefix.to_lowercase();
            if let Some(previous) = self.names.insert(key, prefix.clone())
                && previous != prefix
            {
                return Err(format!("Windows case collision: `{previous}` / `{prefix}`"));
            }
        }
        if self.capture.files.len() >= ENTRY_LIMIT {
            return Err("Capture превышает 100 000 entries".into());
        }
        let excluded = path
            .split('/')
            .any(|part| snapshot::EXCLUDED.contains(&part));
        let unsupported = reason
            .or_else(|| {
                (!matches!(mode, 0o100644 | 0o100755))
                    .then(|| "Symlink/gitlink/тип записи не анализируется".into())
            })
            .or_else(|| {
                bytes
                    .is_some_and(|bytes| {
                        bytes.starts_with(b"version https://git-lfs.github.com/spec/v1\n")
                            || bytes.starts_with(b"version https://git-lfs.github.com/spec/v1\r\n")
                    })
                    .then(|| "LFS pointer: содержимое не загружается".into())
            });
        let status = if excluded {
            "excluded"
        } else if unsupported.is_some() {
            "unsupported"
        } else if bytes.is_none() {
            "absent"
        } else {
            "included"
        };
        if status == "included" {
            let bytes = bytes.unwrap();
            self.total += bytes.len() as u64;
            if bytes.len() as u64 > FILE_LIMIT || self.total > TOTAL_LIMIT {
                return Err("Capture превышает лимит 10 МиБ/файл или 512 МиБ суммарно".into());
            }
            let target = self.capture.directory.path().join(path);
            fs::create_dir_all(target.parent().ok_or("Некорректный capture path")?)
                .map_err(error)?;
            // Private staging has no symlinks. create_new also catches file/directory and OS name collisions.
            use std::io::Write;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)
                .and_then(|mut file| file.write_all(bytes))
                .map_err(error)?;
        }
        if status == "unsupported" {
            self.capture.basis.omitted_count += 1;
        }
        self.capture.files.push(FileFact {
            path: path.into(),
            kind: if mode == 0o160000 {
                "gitlink"
            } else if mode == 0o120000 {
                "symlink"
            } else {
                "file"
            }
            .into(),
            mode,
            content_hash: bytes.map(snapshot::hash),
            blob_oid: oid,
            status: status.into(),
            reason: if excluded {
                Some("Generated/dependency capture policy".into())
            } else {
                unsupported
            },
            parser_version: None,
            imports: Vec::new(),
            parse_error: None,
        });
        Ok(())
    }
    fn finish(mut self, consistency: &str) -> Capture {
        self.capture.files.sort_by(|a, b| a.path.cmp(&b.path));
        self.capture.basis.capture_fingerprint =
            snapshot::hash(&serde_json::to_vec(&self.capture.files).expect("inventory serializes"));
        self.capture.basis.capture_consistency = consistency.into();
        self.capture.basis.completed_at = snapshot::timestamp();
        self.capture
    }
}
pub fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 4096 || path.starts_with('/') {
        return Err(format!("Непредставимый путь Git: `{path}`"));
    }
    for part in path.split('/') {
        let base = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
            || (base.len() == 4
                && (base.starts_with("COM") || base.starts_with("LPT"))
                && matches!(base.as_bytes()[3], b'1'..=b'9'));
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with([' ', '.'])
            || part.chars().any(|c| c < ' ' || r#"\:<>"|?*"#.contains(c))
            || reserved
        {
            return Err(format!(
                "Небезопасный/непредставимый Windows путь Git: `{path}`"
            ));
        }
    }
    Ok(())
}
pub fn capture_commit(
    root: &str,
    reference: &str,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize),
) -> Result<Capture, String> {
    let context = Context::open(root)?;
    let commit = resolve_commit(&context, reference)?;
    let mut writer = Writer::new(&context, Some(reference))?;
    writer.capture.basis.commit_oid = Some(commit.id().to_string());
    writer.capture.basis.tree_oid = Some(commit.tree_id().to_string());
    let mut tree = commit.tree().map_err(error)?;
    if !context.scope.is_empty() {
        let entry = tree
            .get_path(Path::new(&context.scope))
            .map_err(|e| format!("Project scope отсутствует в коммите: {e}"))?;
        tree = context.repository.find_tree(entry.id()).map_err(error)?;
    }
    let mut pending = vec![(tree, String::new())];
    let mut count = 0usize;
    while let Some((tree, prefix)) = pending.pop() {
        for entry in tree.iter() {
            snapshot::check_cancel(cancel)?;
            count += 1;
            if count > ENTRY_LIMIT {
                return Err("Tree превышает 100 000 entries".into());
            }
            let name = entry.name().map_err(error)?;
            let path = format!("{prefix}{name}");
            validate_path(&path)?;
            if entry.kind() == Some(git2::ObjectType::Tree) {
                pending.push((
                    context.repository.find_tree(entry.id()).map_err(error)?,
                    format!("{path}/"),
                ));
                continue;
            }
            let excluded = path
                .split('/')
                .any(|part| snapshot::EXCLUDED.contains(&part));
            if entry.filemode() == 0o160000 || excluded {
                writer.add(
                    &path,
                    entry.filemode() as u32,
                    Some(entry.id().to_string()),
                    None,
                    None,
                )?;
            } else {
                let blob = context
                    .repository
                    .find_blob(entry.id())
                    .map_err(|e| format!("Недоступен blob `{path}`: {e}"))?;
                if blob.size() as u64 > FILE_LIMIT {
                    return Err(format!("Blob `{path}` превышает 10 МиБ"));
                }
                writer.add(
                    &path,
                    entry.filemode() as u32,
                    Some(entry.id().to_string()),
                    Some(blob.content()),
                    None,
                )?;
            }
            progress(count);
        }
    }
    snapshot::check_cancel(cancel)?;
    Ok(writer.finish("immutable-tree"))
}
fn worktree_candidates(
    context: &Context,
) -> Result<BTreeMap<String, (u32, Option<String>)>, String> {
    let mut candidates = BTreeMap::new();
    let mut index = context.repository.index().map_err(error)?;
    index.read(true).map_err(error)?;
    for entry in index.iter() {
        let path = std::str::from_utf8(&entry.path).map_err(error)?;
        if let Some(path) = context.relative(path) {
            candidates.insert(path, (entry.mode, Some(entry.id.to_string())));
        }
        if candidates.len() > ENTRY_LIMIT {
            return Err("Index превышает 100 000 entries".into());
        }
    }
    for row in status_rows(context)? {
        if row.untracked {
            if row.path.ends_with('/') {
                candidates
                    .entry(row.path.trim_end_matches('/').into())
                    .or_insert((0o160000, None));
                continue;
            }
            candidates.entry(row.path).or_insert((0o100644, None));
        }
    }
    if candidates.len() > ENTRY_LIMIT {
        return Err("Worktree превышает 100 000 entries".into());
    }
    Ok(candidates)
}
fn regular_bytes(context: &Context, path: &str) -> Result<Option<Vec<u8>>, String> {
    validate_path(path)?;
    let mut source = context.logical_root.clone();
    for part in path.split('/') {
        source.push(part);
        let metadata = match fs::symlink_metadata(&source) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(error(e)),
        };
        if metadata.file_type().is_symlink() || is_reparse(&metadata) {
            return Err(format!("Не поддерживается symlink/reparse path: {path}"));
        }
    }
    let canonical = fs::canonicalize(&source).map_err(error)?;
    if !canonical.starts_with(&context.logical_root) {
        return Err(format!("Путь вышел за project scope: {path}"));
    }
    let file = fs::File::open(&source).map_err(error)?;
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err(format!("Файл `{path}` превышает 10 МиБ"));
    }
    Ok(Some(bytes))
}
fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}
pub fn capture_worktree(
    root: &str,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize),
) -> Result<Capture, String> {
    let context = Context::open(root)?;
    let mut writer = Writer::new(&context, None)?;
    let candidates = worktree_candidates(&context)?;
    for (index, (path, (mode, oid))) in candidates.iter().enumerate() {
        snapshot::check_cancel(cancel)?;
        if path
            .split('/')
            .any(|part| snapshot::EXCLUDED.contains(&part))
            || matches!(*mode, 0o120000 | 0o160000)
        {
            writer.add(path, *mode, oid.clone(), None, None)?;
        } else {
            let bytes = regular_bytes(&context, path)?;
            // An index OID is metadata, not the identity of dirty worktree bytes.
            writer.add(path, *mode, None, bytes.as_deref(), None)?;
        }
        progress(index + 1);
    }
    snapshot::check_cancel(cancel)?;
    if candidates != worktree_candidates(&context)?
        || writer.capture.basis.observed_head_oid != context.head()
        || writer.capture.basis.index_fingerprint.as_ref() != Some(&context.index_fingerprint()?)
    {
        return Err("Git/worktree изменился во время capture. Повторите анализ".into());
    }
    for fact in writer
        .capture
        .files
        .iter()
        .filter(|fact| fact.status == "included" || fact.status == "absent")
    {
        snapshot::check_cancel(cancel)?;
        let bytes = regular_bytes(&context, &fact.path)?;
        if bytes.as_deref().map(snapshot::hash) != fact.content_hash {
            return Err(format!(
                "`{}` изменился во время capture. Повторите анализ",
                fact.path
            ));
        }
    }
    Ok(writer.finish("verified-capture-not-atomic"))
}
