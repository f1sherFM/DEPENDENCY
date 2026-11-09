mod analysis;

#[tauri::command]
async fn analyze_project(root: String) -> Result<analysis::AnalysisResult, String> {
    tauri::async_runtime::spawn_blocking(move || analysis::analyze_project(&root))
        .await
        .map_err(|error| format!("Ошибка задачи анализа: {error}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![analyze_project])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
