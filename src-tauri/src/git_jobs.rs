use crate::{analysis, git, snapshot, storage};
use serde::Serialize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub id: String,
    pub root: String,
    pub phase: String,
    pub file_count: usize,
    pub done: bool,
    pub error: Option<String>,
    pub snapshot_id: Option<String>,
}
struct Job {
    status: Mutex<JobStatus>,
    cancel: AtomicBool,
}
#[derive(Clone, Default)]
pub struct GitJobs {
    current: Arc<Mutex<Option<Arc<Job>>>>,
}
impl GitJobs {
    pub fn start(
        &self,
        storage: storage::AppState,
        root: String,
        reference: Option<String>,
    ) -> Result<String, String> {
        let mut current = self.current.lock().map_err(|_| "Git worker недоступен")?;
        if let Some(job) = current.as_ref()
            && !job.status.lock().map_err(|_| "Git job недоступен")?.done
        {
            return Err("Git анализ уже выполняется. Дождитесь завершения или отмените его".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let job = Arc::new(Job {
            status: Mutex::new(JobStatus {
                id: id.clone(),
                root: root.clone(),
                phase: "capturing".into(),
                file_count: 0,
                done: false,
                error: None,
                snapshot_id: None,
            }),
            cancel: AtomicBool::new(false),
        });
        *current = Some(job.clone());
        tauri::async_runtime::spawn_blocking(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (|| {
                    let progress = |count| {
                        let mut state = job.status.lock().unwrap_or_else(|poisoned| {
                            job.status.clear_poison();
                            poisoned.into_inner()
                        });
                        state.file_count = count;
                    };
                    let capture = match reference {
                        Some(reference) => {
                            git::capture_commit(&root, &reference, &job.cancel, &progress)?
                        }
                        None => git::capture_worktree(&root, &job.cancel, &progress)?,
                    };
                    job.status.lock().map_err(|_| "Git job недоступен")?.phase = "analyzing".into();
                    let (output, inventory) = analyze_capture(&storage, capture, &job.cancel)?;
                    let mut state = job.status.lock().map_err(|_| "Git job недоступен")?;
                    snapshot::check_cancel(&job.cancel)?;
                    state.phase = "saving".into();
                    // The commit point is serialized with cancel. A committed snapshot is never reported as cancelled.
                    let view = storage::save_captured_snapshot(
                        &storage,
                        output.result,
                        output.file_cache,
                        output.parsed_file_count,
                        output.reused_file_count,
                        inventory,
                    )?;
                    state.snapshot_id = Some(view.snapshot.id);
                    Ok::<(), String>(())
                })()
            }))
            .unwrap_or_else(|_| {
                Err(
                    "Неожиданный сбой Git analysis. Проверьте историю снимков перед повтором"
                        .into(),
                )
            });
            {
                let mut state = job.status.lock().unwrap_or_else(|poisoned| {
                    job.status.clear_poison();
                    poisoned.into_inner()
                });
                state.done = true;
                state.error = result.err();
                state.phase = if state.error.is_some() {
                    if job.cancel.load(Ordering::Relaxed) {
                        "cancelled"
                    } else {
                        "failed"
                    }
                } else {
                    "complete"
                }
                .into();
            }
        });
        Ok(id)
    }
    fn job(&self, id: &str) -> Result<Arc<Job>, String> {
        self.current
            .lock()
            .map_err(|_| "Git worker недоступен")?
            .as_ref()
            .filter(|job| job.status.lock().is_ok_and(|state| state.id == id))
            .cloned()
            .ok_or("Git job истёк".into())
    }
    pub fn status(&self, id: &str) -> Result<JobStatus, String> {
        self.job(id)?
            .status
            .lock()
            .map(|state| state.clone())
            .map_err(|_| "Git job недоступен".into())
    }
    pub fn current_status(&self) -> Result<Option<JobStatus>, String> {
        self.current
            .lock()
            .map_err(|_| "Git worker недоступен")?
            .as_ref()
            .map(|job| {
                job.status
                    .lock()
                    .map(|state| state.clone())
                    .map_err(|_| "Git job недоступен".into())
            })
            .transpose()
    }
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let job = self.job(id)?;
        let state = job.status.lock().map_err(|_| "Git job недоступен")?;
        if state.done {
            return Err("Git job уже завершён".into());
        }
        if state.snapshot_id.is_some() || state.phase == "saving" {
            return Err("Снимок уже фиксируется/зафиксирован; отмена недоступна".into());
        }
        job.cancel.store(true, Ordering::Relaxed);
        Ok(())
    }
}

pub fn analyze_capture(
    storage: &storage::AppState,
    mut capture: git::Capture,
    cancel: &AtomicBool,
) -> Result<(analysis::AnalysisOutput, Vec<snapshot::FileFact>), String> {
    let logical = git::display(&capture.logical_root);
    let project = storage::list_projects(storage)?
        .into_iter()
        .find(|project| project.root_path.to_lowercase() == logical.to_lowercase());
    let cache = storage::load_cache(storage, project.as_ref().map(|project| project.id.as_str()))?;
    snapshot::check_cancel(cancel)?;
    let physical = std::fs::canonicalize(capture.directory.path()).map_err(|e| e.to_string())?;
    let mut output = analysis::analyze_input(
        physical.to_str().ok_or("Capture path не UTF-8")?,
        cache,
        true,
        cancel,
    )?;
    snapshot::check_cancel(cancel)?;
    let parsed_by_path: std::collections::HashMap<_, _> = output
        .file_cache
        .iter()
        .map(|parsed| (parsed.relative_path.as_str(), parsed))
        .collect();
    for file in &mut capture.files {
        if let Some(parsed) = parsed_by_path.get(file.path.as_str()) {
            file.parser_version = Some(parsed.analyzer_version.clone());
            file.imports = parsed.imports.clone();
            file.parse_error = Some(parsed.parse_error);
        }
    }
    if capture.basis.omitted_count > 0 {
        if let Some(run) = output.result.rules_run.as_mut() {
            run.coverage_issue_count += capture.basis.omitted_count;
            run.completed_rules
                .retain(|rule| rule != "no-known-consumers");
            run.skipped_rules.push(crate::diagnostics::SkippedRule {
                rule_id: "no-known-consumers".into(),
                reason: "Capture содержит неподдерживаемые symlinks/gitlinks/LFS".into(),
            });
        }
        output
            .result
            .diagnostics
            .retain(|diagnostic| diagnostic.rule_id != "no-known-consumers");
    }
    // Sanitize any resolver evidence before persistence, not only the summary root.
    let mut value = serde_json::to_value(&output.result).map_err(|e| e.to_string())?;
    let patterns = [
        physical.to_string_lossy().into_owned(),
        git::display(&physical),
        physical.to_string_lossy().replace('\\', "/"),
        git::display(&physical).replace('\\', "/"),
    ];
    fn sanitize(value: &mut serde_json::Value, patterns: &[String], logical: &str) {
        match value {
            serde_json::Value::String(text) => {
                for pattern in patterns {
                    *text = text.replace(pattern, logical);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    sanitize(item, patterns, logical);
                }
            }
            serde_json::Value::Object(map) => {
                for item in map.values_mut() {
                    sanitize(item, patterns, logical);
                }
            }
            _ => {}
        }
    }
    sanitize(&mut value, &patterns, &logical);
    output.result = serde_json::from_value(value).map_err(|e| e.to_string())?;
    output.result.summary.root_path = logical;
    output.result.summary.name = capture
        .logical_root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    output.result.basis = Some(capture.basis);
    Ok((output, capture.files))
}
