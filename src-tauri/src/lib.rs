mod agent_access;
mod analysis;
mod comparison;
mod diagnostics;
mod git;
mod git_jobs;
#[cfg(test)]
mod git_tests;
mod impact;
mod investigations;
mod js_ts;
mod language;
mod native_languages;
mod native_resolver;
#[cfg(test)]
mod native_tests;
mod parse_facts;
mod python_resolver;
#[cfg(test)]
mod python_tests;
mod resolver;
mod snapshot;
mod storage;
mod transfer;

use tauri::{Manager, State};

#[tauri::command]
async fn export_research(
    state: State<'_, storage::AppState>,
    id: String,
    path: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || transfer::export(&state, &id, &path))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn preview_research(path: String) -> Result<transfer::Preview, String> {
    tauri::async_runtime::spawn_blocking(move || transfer::preview(&path))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn import_research(
    state: State<'_, storage::AppState>,
    path: String,
    fingerprint: String,
    project_id: String,
) -> Result<investigations::Investigation, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        transfer::import(&state, &path, &fingerprint, &project_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn save_investigation(
    request: investigations::SaveRequest,
    state: State<'_, storage::AppState>,
) -> Result<investigations::Investigation, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || investigations::save(&state, request))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn load_investigation(
    id: String,
    state: State<'_, storage::AppState>,
) -> Result<investigations::Investigation, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || investigations::load(&state, &id))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn list_investigations(
    project_id: String,
    offset: usize,
    limit: usize,
    state: State<'_, storage::AppState>,
) -> Result<investigations::Page, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        investigations::list(&state, &project_id, offset, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn delete_investigation(
    id: String,
    expected_revision: i64,
    state: State<'_, storage::AppState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        investigations::delete(&state, &id, expected_revision)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn agent_access_enabled(access: State<'_, agent_access::AgentAccess>) -> bool {
    access.enabled()
}

#[tauri::command]
async fn publish_agent_state(
    state: serde_json::Value,
    access: State<'_, agent_access::AgentAccess>,
) -> Result<(), String> {
    let access = access.inner().clone();
    tauri::async_runtime::spawn_blocking(move || access.publish(state))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn list_projects(
    state: State<'_, storage::AppState>,
) -> Result<Vec<storage::ProjectRecord>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || storage::list_projects(&state))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_project(
    root: String,
    state: State<'_, storage::AppState>,
) -> Result<Option<storage::ProjectView>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || storage::open_project(&state, &root))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn analyze_project(
    root: String,
    state: State<'_, storage::AppState>,
) -> Result<storage::ProjectView, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root_path = std::fs::canonicalize(&root)
            .map_err(|e| format!("Не удалось открыть каталог проекта: {e}"))?
            .to_string_lossy()
            .to_lowercase();
        let project_id = storage::list_projects(&state)?
            .into_iter()
            .find(|p| p.root_path.to_lowercase() == root_path)
            .map(|p| p.id);
        let cache = storage::load_cache(&state, project_id.as_deref())?;
        let output = analysis::analyze_project(&root, cache)?;
        storage::save_snapshot(
            &state,
            output.result,
            output.file_cache,
            output.parsed_file_count,
            output.reused_file_count,
        )
    })
    .await
    .map_err(|error| format!("Ошибка задачи анализа: {error}"))?
}

#[tauri::command]
async fn load_project(
    project_id: String,
    state: State<'_, storage::AppState>,
) -> Result<storage::ProjectView, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || storage::load_project(&state, &project_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn list_snapshots(
    project_id: String,
    state: State<'_, storage::AppState>,
) -> Result<Vec<storage::SnapshotRecord>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || storage::list_snapshots(&state, &project_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn load_snapshot(
    snapshot_id: String,
    state: State<'_, storage::AppState>,
) -> Result<storage::ProjectView, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || storage::load_snapshot(&state, &snapshot_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_impact(
    request: impact::ImpactRequest,
    state: State<'_, storage::AppState>,
    impact: State<'_, impact::ImpactState>,
) -> Result<impact::ImpactSummary, String> {
    let state = state.inner().clone();
    let impact = impact.inner().clone();
    tauri::async_runtime::spawn_blocking(move || impact.get(&state, request))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn get_impact_page(
    query_id: String,
    offset: usize,
    limit: usize,
    text: String,
    tests_only: bool,
    impact: State<'_, impact::ImpactState>,
) -> Result<impact::ImpactPage, String> {
    let impact = impact.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        impact.page(&query_id, offset, limit, &text, tests_only)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn get_impact_path(
    query_id: String,
    node_id: String,
    offset: usize,
    limit: usize,
    impact: State<'_, impact::ImpactState>,
) -> Result<impact::ImpactPath, String> {
    let impact = impact.inner().clone();
    tauri::async_runtime::spawn_blocking(move || impact.path(&query_id, &node_id, offset, limit))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn discover_git(root: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || git::discover(&root))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn git_status(root: String, offset: usize, limit: usize) -> Result<git::GitStatus, String> {
    tauri::async_runtime::spawn_blocking(move || git::status(&root, offset, limit))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn git_refs(
    root: String,
    offset: usize,
    limit: usize,
) -> Result<git::Page<git::RefRow>, String> {
    tauri::async_runtime::spawn_blocking(move || git::refs(&root, offset, limit))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn git_commits(
    root: String,
    reference: String,
    offset: usize,
    limit: usize,
) -> Result<git::Page<git::CommitRow>, String> {
    tauri::async_runtime::spawn_blocking(move || git::commits(&root, &reference, offset, limit))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn start_git_analysis(
    root: String,
    reference: Option<String>,
    state: State<'_, storage::AppState>,
    jobs: State<'_, git_jobs::GitJobs>,
) -> Result<String, String> {
    jobs.start(state.inner().clone(), root, reference)
}
#[tauri::command]
async fn get_git_job(
    job_id: Option<String>,
    jobs: State<'_, git_jobs::GitJobs>,
) -> Result<Option<git_jobs::JobStatus>, String> {
    match job_id {
        Some(id) => jobs.status(&id).map(Some),
        None => jobs.current_status(),
    }
}
#[tauri::command]
async fn cancel_git_job(job_id: String, jobs: State<'_, git_jobs::GitJobs>) -> Result<(), String> {
    jobs.cancel(&job_id)
}
#[tauri::command]
async fn compare_snapshots(
    base_id: String,
    target_id: String,
    state: State<'_, storage::AppState>,
    comparisons: State<'_, comparison::Comparisons>,
) -> Result<comparison::Summary, String> {
    let state = state.inner().clone();
    let comparisons = comparisons.inner().clone();
    tauri::async_runtime::spawn_blocking(move || comparisons.compare(&state, &base_id, &target_id))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn comparison_page(
    id: String,
    offset: usize,
    limit: usize,
    category: String,
    text: String,
    comparisons: State<'_, comparison::Comparisons>,
) -> Result<comparison::Page, String> {
    let comparisons = comparisons.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        comparisons.page(&id, offset, limit, &category, &text)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn git_snapshot_page(
    project_id: String,
    offset: usize,
    limit: usize,
    state: State<'_, storage::AppState>,
) -> Result<git::Page<storage::SnapshotRecord>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        storage::snapshot_page(&state, &project_id, offset, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn snapshot_inventory(
    snapshot_id: String,
    offset: usize,
    limit: usize,
    state: State<'_, storage::AppState>,
) -> Result<git::Page<storage::InventoryRow>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        storage::inventory_page(&state, &snapshot_id, offset, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(impact::ImpactState::default())
        .manage(git_jobs::GitJobs::default())
        .manage(comparison::Comparisons::default())
        .setup(|app| {
            let database_path = app.path().app_data_dir()?.join("dependency.sqlite3");
            app.manage(agent_access::AgentAccess::new(app.path().app_data_dir()?));
            let storage = storage::initialize(database_path).map_err(std::io::Error::other)?;
            app.manage(storage);
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            export_research,
            preview_research,
            import_research,
            save_investigation,
            load_investigation,
            list_investigations,
            delete_investigation,
            agent_access_enabled,
            publish_agent_state,
            analyze_project,
            list_projects,
            open_project,
            load_project,
            list_snapshots,
            load_snapshot,
            get_impact,
            get_impact_page,
            get_impact_path,
            discover_git,
            git_status,
            git_refs,
            git_commits,
            start_git_analysis,
            get_git_job,
            cancel_git_job,
            compare_snapshots,
            comparison_page,
            git_snapshot_page,
            snapshot_inventory
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
