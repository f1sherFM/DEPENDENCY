mod analysis;
mod storage;

use tauri::{Manager, State};

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let database_path = app.path().app_data_dir()?.join("dependency.sqlite3");
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
            analyze_project,
            list_projects,
            open_project,
            load_project,
            list_snapshots,
            load_snapshot
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
