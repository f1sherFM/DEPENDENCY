use crate::analysis::{AnalysisResult, ParsedFileCache};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

// Keep these independent of the app release: bump only when parser output or
// diagnostic semantics change, so UI and storage changes do not invalidate AST cache.
pub const ANALYZER_VERSION: &str = "js-ts-parser-2";
pub const RULES_VERSION: &str = "diagnostics-3+resolver-3";

#[derive(Clone)]
pub struct AppState {
    pub database_path: PathBuf,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub last_opened_at: i64,
    pub latest_snapshot_id: Option<String>,
    pub latest_snapshot_at: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRecord {
    pub id: String,
    pub project_id: String,
    pub created_at: i64,
    pub analyzer_version: String,
    pub rules_version: String,
    pub parsed_file_count: usize,
    pub reused_file_count: usize,
    #[serde(default)]
    pub basis: Option<crate::snapshot::Basis>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub project: ProjectRecord,
    pub snapshot: SnapshotRecord,
    pub result: AnalysisResult,
}

pub fn initialize(path: PathBuf) -> Result<AppState, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Не удалось создать каталог данных: {e}"))?;
    }
    let state = AppState {
        database_path: path,
    };
    let connection = connect(&state)?;
    let schema_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| format!("Не удалось проверить версию базы данных: {e}"))?;
    if schema_version > 2 {
        return Err(format!(
            "База DEPENDENCY имеет более новую версию схемы ({schema_version}) и не может быть открыта этой версией приложения"
        ));
    }
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects (
           id TEXT PRIMARY KEY, root_key TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
           root_path TEXT NOT NULL, created_at INTEGER NOT NULL, last_opened_at INTEGER NOT NULL,
           latest_snapshot_id TEXT
         );
         CREATE TABLE IF NOT EXISTS snapshots (
           id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           created_at INTEGER NOT NULL, analyzer_version TEXT NOT NULL, rules_version TEXT NOT NULL,
           parsed_file_count INTEGER NOT NULL, reused_file_count INTEGER NOT NULL,
           result_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS snapshots_project_created ON snapshots(project_id, created_at DESC);
         CREATE TABLE IF NOT EXISTS parsed_files (
           project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           relative_path TEXT NOT NULL, content_hash TEXT NOT NULL, analyzer_version TEXT NOT NULL,
           cache_json TEXT NOT NULL, PRIMARY KEY(project_id, relative_path)
         );
         "
    ).map_err(|e| format!("Не удалось создать схему SQLite: {e}"))?;
    if schema_version < 2 {
        transaction
            .execute("ALTER TABLE snapshots ADD COLUMN basis_json TEXT", [])
            .map_err(|e| e.to_string())?;
    }
    transaction.execute_batch("CREATE TABLE IF NOT EXISTS snapshot_files(snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE, relative_path TEXT NOT NULL, metadata_json TEXT NOT NULL, imports_json TEXT NOT NULL, PRIMARY KEY(snapshot_id,relative_path)); PRAGMA user_version=2;").map_err(|e| e.to_string())?;
    transaction
        .commit()
        .map_err(|e| format!("Не удалось мигрировать SQLite: {e}"))?;
    Ok(state)
}

fn connect(state: &AppState) -> Result<Connection, String> {
    let connection = Connection::open(&state.database_path)
        .map_err(|e| format!("Не удалось открыть базу проектов: {e}"))?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;",
        )
        .map_err(|e| format!("Не удалось настроить SQLite: {e}"))?;
    Ok(connection)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectRecord> {
    Ok(ProjectRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        root_path: row.get(2)?,
        last_opened_at: row.get(3)?,
        latest_snapshot_id: row.get(4)?,
        latest_snapshot_at: row.get(5)?,
    })
}

pub fn list_projects(state: &AppState) -> Result<Vec<ProjectRecord>, String> {
    let connection = connect(state)?;
    let mut statement = connection.prepare("SELECT p.id,p.name,p.root_path,p.last_opened_at,p.latest_snapshot_id,s.created_at FROM projects p LEFT JOIN snapshots s ON s.id=p.latest_snapshot_id ORDER BY p.last_opened_at DESC")
        .map_err(|e| e.to_string())?;
    statement
        .query_map([], project_from_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn get_project(connection: &Connection, id: &str) -> Result<ProjectRecord, String> {
    connection.query_row("SELECT p.id,p.name,p.root_path,p.last_opened_at,p.latest_snapshot_id,s.created_at FROM projects p LEFT JOIN snapshots s ON s.id=p.latest_snapshot_id WHERE p.id=?1", [id], project_from_row)
        .optional().map_err(|e| e.to_string())?.ok_or_else(|| "Проект не найден в хранилище".to_string())
}

fn snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotRecord> {
    Ok(SnapshotRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        created_at: row.get(2)?,
        analyzer_version: row.get(3)?,
        rules_version: row.get(4)?,
        parsed_file_count: row.get::<_, i64>(5)? as usize,
        reused_file_count: row.get::<_, i64>(6)? as usize,
        basis: None,
    })
}

fn get_view(
    connection: &Connection,
    project: ProjectRecord,
    snapshot_id: &str,
) -> Result<ProjectView, String> {
    let (mut snapshot, json) = connection.query_row(
        "SELECT id,project_id,created_at,analyzer_version,rules_version,parsed_file_count,reused_file_count,result_json FROM snapshots WHERE id=?1 AND project_id=?2",
        params![snapshot_id, project.id], |row| Ok((snapshot_from_row(row)?, row.get::<_, String>(7)?))
    ).optional().map_err(|e| e.to_string())?.ok_or_else(|| "Снимок не найден".to_string())?;
    let result: AnalysisResult =
        serde_json::from_str(&json).map_err(|e| format!("Не удалось прочитать снимок: {e}"))?;
    snapshot.basis = result.basis.clone();
    Ok(ProjectView {
        project,
        snapshot,
        result,
    })
}

pub fn open_project(state: &AppState, root: &str) -> Result<Option<ProjectView>, String> {
    let root =
        fs::canonicalize(root).map_err(|e| format!("Не удалось открыть каталог проекта: {e}"))?;
    let root_path = root.to_string_lossy().to_string();
    let connection = connect(state)?;
    let project = connection.query_row("SELECT id,name,root_path,last_opened_at,latest_snapshot_id,(SELECT created_at FROM snapshots WHERE id=projects.latest_snapshot_id) FROM projects WHERE root_key=?1", [root_path.to_lowercase()], project_from_row)
        .optional().map_err(|e| e.to_string())?;
    if let Some(project) = project {
        let _ = connection.execute(
            "UPDATE projects SET last_opened_at=?1 WHERE id=?2",
            params![now(), project.id],
        );
        let preferred = match project.latest_snapshot_id.clone() { Some(id) => Some(id), None => connection.query_row("SELECT id FROM snapshots WHERE project_id=?1 ORDER BY created_at DESC,id DESC LIMIT 1", [&project.id], |row| row.get(0)).optional().map_err(|e| e.to_string())? };
        return preferred
            .map(|id| get_view(&connection, project, &id))
            .transpose();
    }
    Ok(None)
}

pub fn load_project(state: &AppState, id: &str) -> Result<ProjectView, String> {
    let connection = connect(state)?;
    let project = get_project(&connection, id)?;
    let snapshot_id = match project.latest_snapshot_id.clone() {
        Some(id) => id,
        None => connection.query_row("SELECT id FROM snapshots WHERE project_id=?1 ORDER BY created_at DESC,id DESC LIMIT 1", [id], |row| row.get(0)).map_err(|_| "У проекта ещё нет готового снимка")?,
    };
    connection
        .execute(
            "UPDATE projects SET last_opened_at=?1 WHERE id=?2",
            params![now(), id],
        )
        .map_err(|e| e.to_string())?;
    get_view(&connection, project, &snapshot_id)
}

pub fn load_snapshot(state: &AppState, id: &str) -> Result<ProjectView, String> {
    let connection = connect(state)?;
    let project_id: String = connection
        .query_row("SELECT project_id FROM snapshots WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Снимок не найден".to_string())?;
    get_view(&connection, get_project(&connection, &project_id)?, id)
}

/// Impact queries only read immutable snapshot data, never project paths or last-opened state.
pub fn read_impact_snapshot(
    state: &AppState,
    id: &str,
) -> Result<(SnapshotRecord, AnalysisResult), String> {
    let connection = Connection::open_with_flags(
        &state.database_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("Не удалось прочитать хранилище: {error}"))?;
    let (mut snapshot, json) = connection.query_row(
        "SELECT id,project_id,created_at,analyzer_version,rules_version,parsed_file_count,reused_file_count,CASE WHEN length(CAST(result_json AS BLOB)) <= 67108864 THEN result_json ELSE NULL END FROM snapshots WHERE id=?1",
        [id], |row| Ok((snapshot_from_row(row)?, row.get::<_, Option<String>>(7)?)),
    ).optional().map_err(|error| error.to_string())?.ok_or("Снимок не найден".to_string())?;
    let json =
        json.ok_or("Снимок превышает лимит чтения Impact Analysis (64 МиБ); данные не усечены")?;
    let result: AnalysisResult = serde_json::from_str(&json)
        .map_err(|error| format!("Не удалось прочитать снимок: {error}"))?;
    snapshot.basis = result.basis.clone();
    Ok((snapshot, result))
}

pub fn list_snapshots(state: &AppState, project_id: &str) -> Result<Vec<SnapshotRecord>, String> {
    let connection = connect(state)?;
    let mut statement = connection.prepare("SELECT id,project_id,created_at,analyzer_version,rules_version,parsed_file_count,reused_file_count,basis_json FROM snapshots WHERE project_id=?1 ORDER BY created_at DESC,id DESC").map_err(|e| e.to_string())?;
    statement
        .query_map([project_id], |row| {
            let mut snapshot = snapshot_from_row(row)?;
            snapshot.basis = row
                .get::<_, Option<String>>(7)?
                .map(|json| {
                    serde_json::from_str(&json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            7,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })
                })
                .transpose()?;
            Ok(snapshot)
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn load_cache(
    state: &AppState,
    project_id: Option<&str>,
) -> Result<Vec<ParsedFileCache>, String> {
    let Some(project_id) = project_id else {
        return Ok(Vec::new());
    };
    let connection = connect(state)?;
    let mut statement = connection
        .prepare("SELECT cache_json FROM parsed_files WHERE project_id=?1 AND analyzer_version=?2")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(params![project_id, ANALYZER_VERSION], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?;
    rows.map(|json| {
        let json = json.map_err(|e| e.to_string())?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    })
    .collect()
}

pub fn save_snapshot(
    state: &AppState,
    result: AnalysisResult,
    cache: Vec<ParsedFileCache>,
    parsed: usize,
    reused: usize,
) -> Result<ProjectView, String> {
    save_captured_snapshot(state, result, cache, parsed, reused, Vec::new())
}
pub fn save_captured_snapshot(
    state: &AppState,
    result: AnalysisResult,
    cache: Vec<ParsedFileCache>,
    parsed: usize,
    reused: usize,
    inventory: Vec<crate::snapshot::FileFact>,
) -> Result<ProjectView, String> {
    let root_path = result.summary.root_path.clone();
    let root_key = root_path.to_lowercase();
    let name = result.summary.name.clone();
    let json = serde_json::to_string(&result)
        .map_err(|e| format!("Не удалось подготовить снимок: {e}"))?;
    let connection = connect(state)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| format!("Не удалось начать транзакцию снимка: {e}"))?;
    let project_id = transaction
        .query_row(
            "SELECT id FROM projects WHERE root_key=?1",
            [&root_key],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let time = now();
    transaction.execute("INSERT INTO projects(id,root_key,name,root_path,created_at,last_opened_at) VALUES(?1,?2,?3,?4,?5,?5) ON CONFLICT(root_key) DO UPDATE SET name=excluded.name,root_path=excluded.root_path,last_opened_at=excluded.last_opened_at", params![project_id, root_key, name, root_path, time]).map_err(|e| format!("Не удалось сохранить проект: {e}"))?;
    let snapshot_id = Uuid::new_v4().to_string();
    transaction.execute("INSERT INTO snapshots(id,project_id,created_at,analyzer_version,rules_version,parsed_file_count,reused_file_count,result_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![snapshot_id, project_id, time, ANALYZER_VERSION, RULES_VERSION, parsed as i64, reused as i64, json]).map_err(|e| format!("Не удалось сохранить снимок: {e}"))?;
    let basis_json = result
        .basis
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())?;
    transaction
        .execute(
            "UPDATE snapshots SET basis_json=?1 WHERE id=?2",
            params![basis_json, snapshot_id],
        )
        .map_err(|e| e.to_string())?;
    for fact in inventory {
        let imports = serde_json::to_string(&fact.imports).map_err(|e| e.to_string())?;
        let mut metadata = fact.clone();
        metadata.imports.clear();
        let metadata = serde_json::to_string(&metadata).map_err(|e| e.to_string())?;
        transaction.execute("INSERT INTO snapshot_files(snapshot_id,relative_path,metadata_json,imports_json) VALUES(?1,?2,?3,?4)",params![snapshot_id,fact.path,metadata,imports]).map_err(|e| e.to_string())?;
    }
    for file in &cache {
        let cache_json = serde_json::to_string(file).map_err(|e| e.to_string())?;
        transaction.execute("INSERT INTO parsed_files(project_id,relative_path,content_hash,analyzer_version,cache_json) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(project_id,relative_path) DO UPDATE SET content_hash=excluded.content_hash,analyzer_version=excluded.analyzer_version,cache_json=excluded.cache_json", params![project_id, file.relative_path, file.content_hash, ANALYZER_VERSION, cache_json]).map_err(|e| format!("Не удалось обновить кэш разбора: {e}"))?;
    }
    if result
        .basis
        .as_ref()
        .is_none_or(|basis| basis.kind != "git-commit")
    {
        transaction
            .execute(
                "UPDATE projects SET latest_snapshot_id=?1 WHERE id=?2",
                params![snapshot_id, project_id],
            )
            .map_err(|e| e.to_string())?;
    }
    transaction
        .commit()
        .map_err(|e| format!("Не удалось завершить транзакцию снимка: {e}"))?;
    let connection = connect(state)?;
    let project = get_project(&connection, &project_id)?;
    get_view(&connection, project, &snapshot_id)
}

pub fn read_inventory(
    state: &AppState,
    snapshot_id: &str,
) -> Result<Vec<crate::snapshot::FileFact>, String> {
    let connection = Connection::open_with_flags(
        &state.database_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    let (count,bytes):(i64,i64)=connection.query_row("SELECT count(*),coalesce(sum(length(CAST(metadata_json AS BLOB))+length(CAST(imports_json AS BLOB))),0) FROM snapshot_files WHERE snapshot_id=?1",[snapshot_id],|row| Ok((row.get(0)?,row.get(1)?))).map_err(|e| e.to_string())?;
    if count > 100_000 || bytes > 67_108_864 {
        return Err("Inventory превышает лимит сравнения (100 000 файлов / 64 МиБ)".into());
    }
    let mut statement=connection.prepare("SELECT metadata_json,imports_json FROM snapshot_files WHERE snapshot_id=?1 ORDER BY relative_path").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([snapshot_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    rows.map(|row| {
        let (metadata, imports) = row.map_err(|e| e.to_string())?;
        let mut fact: crate::snapshot::FileFact =
            serde_json::from_str(&metadata).map_err(|e| e.to_string())?;
        fact.imports = serde_json::from_str(&imports).map_err(|e| e.to_string())?;
        Ok(fact)
    })
    .collect()
}

pub fn snapshot_page(
    state: &AppState,
    project_id: &str,
    offset: usize,
    limit: usize,
) -> Result<crate::git::Page<SnapshotRecord>, String> {
    crate::git::page_limit(limit)?;
    if offset > i64::MAX as usize - limit {
        return Err("Смещение страницы снимков вне диапазона".into());
    }
    let connection = connect(state)?;
    let mut statement=connection.prepare("SELECT id,project_id,created_at,analyzer_version,rules_version,parsed_file_count,reused_file_count,basis_json FROM snapshots WHERE project_id=?1 ORDER BY created_at DESC,id DESC LIMIT ?2 OFFSET ?3").map_err(|e|e.to_string())?;
    let mut items = statement
        .query_map(
            params![project_id, (limit + 1) as i64, offset as i64],
            |row| {
                let mut snapshot = snapshot_from_row(row)?;
                snapshot.basis = row
                    .get::<_, Option<String>>(7)?
                    .map(|json| {
                        serde_json::from_str(&json).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                7,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    })
                    .transpose()?;
                Ok(snapshot)
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let more = items.len() > limit;
    items.truncate(limit);
    let next = offset + items.len();
    Ok(crate::git::Page {
        items,
        next_offset: more.then_some(next),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryRow {
    pub fact: crate::snapshot::FileFact,
    pub import_count: usize,
}
pub fn inventory_page(
    state: &AppState,
    id: &str,
    offset: usize,
    limit: usize,
) -> Result<crate::git::Page<InventoryRow>, String> {
    crate::git::page_limit(limit)?;
    let files = read_inventory(state, id)?;
    let total = files.len();
    if offset > total {
        return Err("Страница inventory вне диапазона".into());
    }
    let items = files
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|mut fact| {
            let import_count = fact.imports.len();
            fact.imports.clear();
            InventoryRow { fact, import_count }
        })
        .collect::<Vec<_>>();
    let next = offset + items.len();
    Ok(crate::git::Page {
        items,
        next_offset: (next < total).then_some(next),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initializes_and_reopens_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dependency.sqlite3");
        initialize(path.clone()).unwrap();
        initialize(path).unwrap();
    }
}
