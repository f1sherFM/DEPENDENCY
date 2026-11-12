//! User annotations and graph views refer to immutable snapshots, never latest.
use crate::{analysis::AnalysisResult, storage};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub const STATE_VERSION: i64 = 1;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Camera {
    pub center_x: f64,
    pub center_y: f64,
    pub zoom: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphState {
    pub mode: String,
    pub directory: String,
    pub query: String,
    pub show_packages: bool,
    pub edge_kind: String,
    pub focus: Option<String>,
    pub direction: String,
    pub selected_node_id: Option<String>,
    pub camera: Camera,
    pub details_open: bool,
}
impl GraphState {
    fn validate(&self, result: &AnalysisResult) -> Result<(), String> {
        if !matches!(self.mode.as_str(), "files" | "directories")
            || !matches!(self.direction.as_str(), "both" | "incoming" | "outgoing")
        {
            return Err("Неподдерживаемый режим графа".into());
        }
        if self.query.chars().count() > 1000
            || self.directory.len() > 4096
            || self.edge_kind.len() > 128
        {
            return Err("Параметры графа превышают допустимый размер".into());
        }
        let camera = &self.camera;
        if !camera.zoom.is_finite()
            || !(0.02..=3.0).contains(&camera.zoom)
            || !camera.center_x.is_finite()
            || !camera.center_y.is_finite()
            || camera.center_x.abs() > 1e9
            || camera.center_y.abs() > 1e9
        {
            return Err("Некорректная камера графа".into());
        }
        if !self.directory.is_empty()
            && (self
                .directory
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
                || self.directory.contains(['\\', ':'])
                || !result.nodes.iter().any(|node| {
                    node.kind == "file" && node.path.starts_with(&format!("{}/", self.directory))
                }))
        {
            return Err("Каталог отсутствует в сохранённом снимке".into());
        }
        for id in [&self.focus, &self.selected_node_id].into_iter().flatten() {
            if id.len() > 4096 || !result.nodes.iter().any(|node| &node.id == id) {
                return Err("Узел отсутствует в сохранённом снимке".into());
            }
        }
        if !self.edge_kind.is_empty()
            && !result.edges.iter().any(|edge| edge.kind == self.edge_kind)
        {
            return Err("Тип связи отсутствует в сохранённом снимке".into());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveRequest {
    pub id: Option<String>,
    pub expected_revision: Option<i64>,
    pub snapshot_id: String,
    pub name: String,
    pub note: String,
    pub graph: GraphState,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub id: String,
    pub project_id: String,
    pub snapshot_id: String,
    pub name: String,
    pub note_preview: String,
    pub state_version: i64,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Investigation {
    #[serde(flatten)]
    pub metadata: Metadata,
    pub note: String,
    pub graph: GraphState,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub items: Vec<Metadata>,
    pub total: usize,
    pub next_offset: Option<usize>,
}

const COLUMNS: &str = "i.id,s.project_id,i.snapshot_id,i.name,substr(i.note,1,160),i.state_version,i.revision,i.created_at,i.updated_at";
fn metadata(row: &rusqlite::Row<'_>) -> rusqlite::Result<Metadata> {
    Ok(Metadata {
        id: row.get(0)?,
        project_id: row.get(1)?,
        snapshot_id: row.get(2)?,
        name: row.get(3)?,
        note_preview: row.get(4)?,
        state_version: row.get(5)?,
        revision: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}
fn read_record(connection: &Connection, id: &str) -> Result<Investigation, String> {
    let sql = format!(
        "SELECT {COLUMNS},i.note,CASE WHEN length(CAST(i.graph_json AS BLOB))<=65536 THEN i.graph_json ELSE NULL END FROM investigations i JOIN snapshots s ON s.id=i.snapshot_id WHERE i.id=?1"
    );
    let (metadata, note, graph): (Metadata, String, Option<String>) = connection
        .query_row(&sql, [id], |row| {
            Ok((metadata(row)?, row.get(9)?, row.get(10)?))
        })
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("Исследование не найдено")?;
    if metadata.state_version != STATE_VERSION {
        return Err(
            "Формат исследования создан другой версией приложения; данные сохранены".into(),
        );
    }
    let graph = serde_json::from_str(&graph.ok_or("Состояние исследования превышает 64 КиБ")?)
        .map_err(|e| format!("Не удалось прочитать состояние исследования: {e}"))?;
    Ok(Investigation {
        metadata,
        note,
        graph,
    })
}
pub fn load(state: &storage::AppState, id: &str) -> Result<Investigation, String> {
    let record = read_record(&storage::connect(state)?, id)?;
    let (_, result) = storage::read_impact_snapshot(state, &record.metadata.snapshot_id)?;
    record.graph.validate(&result)?;
    Ok(record)
}
pub fn list(
    state: &storage::AppState,
    project_id: &str,
    offset: usize,
    limit: usize,
) -> Result<Page, String> {
    if limit == 0 || limit > 50 || offset > 1_000_000 {
        return Err("Некорректная страница исследований".into());
    }
    let connection = storage::connect(state)?;
    let total = connection.query_row("SELECT count(*) FROM investigations i JOIN snapshots s ON s.id=i.snapshot_id WHERE s.project_id=?1",[project_id],|row|row.get::<_,usize>(0)).map_err(|e|e.to_string())?;
    let sql = format!(
        "SELECT {COLUMNS} FROM investigations i JOIN snapshots s ON s.id=i.snapshot_id WHERE s.project_id=?1 ORDER BY i.updated_at DESC,i.id LIMIT ?2 OFFSET ?3"
    );
    let items = connection
        .prepare(&sql)
        .map_err(|e| e.to_string())?
        .query_map(params![project_id, limit as i64, offset as i64], metadata)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(Page {
        items,
        total,
        next_offset: (offset + limit < total).then_some(offset + limit),
    })
}
pub fn save(state: &storage::AppState, request: SaveRequest) -> Result<Investigation, String> {
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > 120 || request.note.chars().count() > 16000 {
        return Err("Название должно содержать 1–120 символов, заметка — до 16000".into());
    }
    let (_, result) = storage::read_impact_snapshot(state, &request.snapshot_id)?;
    request.graph.validate(&result)?;
    let graph = serde_json::to_string(&request.graph).map_err(|e| e.to_string())?;
    if graph.len() > 65536 {
        return Err("Состояние исследования превышает 64 КиБ".into());
    }
    let mut connection = storage::connect(state)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let id = if let Some(id) = request.id {
        let old = read_record(&transaction, &id)?;
        if old.metadata.snapshot_id != request.snapshot_id {
            return Err(
                "Снимок исследования нельзя заменить неявно. Сохраните новое исследование".into(),
            );
        }
        if request.expected_revision != Some(old.metadata.revision) {
            return Err("Исследование уже изменилось в другом окне. Откройте его заново".into());
        }
        transaction.execute("UPDATE investigations SET name=?1,note=?2,graph_json=?3,updated_at=?4,revision=revision+1 WHERE id=?5",params![name,request.note,graph,storage::now(),id]).map_err(|e|e.to_string())?;
        id
    } else {
        if request.expected_revision.is_some() {
            return Err("У нового исследования не может быть предыдущей ревизии".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = storage::now();
        transaction.execute("INSERT INTO investigations(id,snapshot_id,name,note,state_version,revision,graph_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,1,?6,?7,?7)",params![id,request.snapshot_id,name,request.note,STATE_VERSION,graph,now]).map_err(|e|e.to_string())?;
        id
    };
    let saved = read_record(&transaction, &id)?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(saved)
}
pub fn delete(state: &storage::AppState, id: &str, expected_revision: i64) -> Result<(), String> {
    let connection = storage::connect(state)?;
    let changed = connection
        .execute(
            "DELETE FROM investigations WHERE id=?1 AND revision=?2 AND state_version=?3",
            params![id, expected_revision, STATE_VERSION],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("Исследование отсутствует, изменилось или имеет неподдерживаемый формат. Обновите список".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        tempfile::TempDir,
        storage::AppState,
        storage::ProjectView,
        GraphState,
    ) {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("src/main.ts"), "import './util';").unwrap();
        std::fs::write(project.join("src/util.ts"), "export const x=1;").unwrap();
        let state = storage::initialize(root.path().join("database.sqlite3")).unwrap();
        let out = crate::analysis::analyze_project(project.to_str().unwrap(), vec![]).unwrap();
        let view = storage::save_snapshot(
            &state,
            out.result,
            out.file_cache,
            out.parsed_file_count,
            out.reused_file_count,
        )
        .unwrap();
        let graph = GraphState {
            mode: "files".into(),
            directory: "src".into(),
            query: "main".into(),
            show_packages: false,
            edge_kind: "import".into(),
            focus: Some("src/main.ts".into()),
            direction: "outgoing".into(),
            selected_node_id: Some("src/util.ts".into()),
            camera: Camera {
                center_x: 250.0,
                center_y: 180.0,
                zoom: 0.4,
            },
            details_open: true,
        };
        (root, state, view, graph)
    }
    fn request(snapshot: &str, graph: GraphState) -> SaveRequest {
        SaveRequest {
            id: None,
            expected_revision: None,
            snapshot_id: snapshot.into(),
            name: "  Слои проекта  ".into(),
            note: "Заметка <script>data only</script>".into(),
            graph,
        }
    }
    fn snapshots_and_cache(state: &storage::AppState) -> (Vec<String>, Vec<String>) {
        let connection = storage::connect(state).unwrap();
        let snapshots = connection
            .prepare("SELECT result_json FROM snapshots ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let cache = connection
            .prepare("SELECT cache_json FROM parsed_files ORDER BY relative_path")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (snapshots, cache)
    }
    #[test]
    fn research_survives_restart_and_missing_sources_and_stays_on_historic_snapshot() {
        let (root, state, old, graph) = fixture();
        let saved = save(&state, request(&old.snapshot.id, graph.clone())).unwrap();
        std::fs::write(root.path().join("project/src/util.ts"), "export const x=2;").unwrap();
        let out = crate::analysis::analyze_project(
            root.path().join("project").to_str().unwrap(),
            storage::load_cache(&state, Some(&old.project.id)).unwrap(),
        )
        .unwrap();
        let new = storage::save_snapshot(
            &state,
            out.result,
            out.file_cache,
            out.parsed_file_count,
            out.reused_file_count,
        )
        .unwrap();
        assert_ne!(old.snapshot.id, new.snapshot.id);
        let before = snapshots_and_cache(&state);
        std::fs::remove_dir_all(root.path().join("project")).unwrap();
        let reopened = storage::initialize(state.database_path.clone()).unwrap();
        let restored = load(&reopened, &saved.metadata.id).unwrap();
        assert_eq!(restored.metadata.snapshot_id, old.snapshot.id);
        assert_eq!(restored.metadata.name, "Слои проекта");
        assert_eq!(restored.graph, graph);
        assert!(restored.note.contains("<script>"));
        assert_eq!(list(&reopened, &old.project.id, 0, 20).unwrap().total, 1);
        assert_eq!(snapshots_and_cache(&reopened), before);
        assert_eq!(
            storage::load_project(&reopened, &old.project.id)
                .unwrap()
                .snapshot
                .id,
            new.snapshot.id
        );
    }
    #[test]
    fn revision_conflicts_write_failure_and_delete_do_not_damage_saved_state_or_snapshots() {
        let (_root, state, view, graph) = fixture();
        let saved = save(&state, request(&view.snapshot.id, graph.clone())).unwrap();
        let before = snapshots_and_cache(&state);
        let mut update = request(&view.snapshot.id, graph.clone());
        update.id = Some(saved.metadata.id.clone());
        update.expected_revision = Some(1);
        update.note = "Новая заметка".into();
        let updated = save(&state, update).unwrap();
        assert_eq!(updated.metadata.revision, 2);
        let mut stale = request(&view.snapshot.id, graph.clone());
        stale.id = Some(saved.metadata.id.clone());
        stale.expected_revision = Some(1);
        assert!(save(&state, stale).unwrap_err().contains("другом окне"));
        storage::connect(&state).unwrap().execute_batch("CREATE TRIGGER deny_research_write BEFORE UPDATE ON investigations BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;").unwrap();
        let mut failed = request(&view.snapshot.id, graph);
        failed.id = Some(saved.metadata.id.clone());
        failed.expected_revision = Some(2);
        assert!(save(&state, failed).is_err());
        assert_eq!(
            load(&state, &saved.metadata.id).unwrap().note,
            "Новая заметка"
        );
        assert!(delete(&state, &saved.metadata.id, 1).is_err());
        delete(&state, &saved.metadata.id, 2).unwrap();
        assert_eq!(list(&state, &view.project.id, 0, 20).unwrap().total, 0);
        assert_eq!(snapshots_and_cache(&state), before);
    }
    #[test]
    fn concurrent_edits_have_one_winner_and_snapshot_rebinding_requires_a_new_record() {
        let (root, state, view, graph) = fixture();
        let saved = save(&state, request(&view.snapshot.id, graph.clone())).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut workers = Vec::new();
        for index in 0..2 {
            let state = state.clone();
            let graph = graph.clone();
            let snapshot_id = view.snapshot.id.clone();
            let id = saved.metadata.id.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                let mut input = request(&snapshot_id, graph);
                input.id = Some(id);
                input.expected_revision = Some(1);
                input.name = format!("Правка {index}");
                barrier.wait();
                save(&state, input)
            }));
        }
        let outcomes: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            load(&state, &saved.metadata.id).unwrap().metadata.revision,
            2
        );
        let out = crate::analysis::analyze_project(
            root.path().join("project").to_str().unwrap(),
            storage::load_cache(&state, Some(&view.project.id)).unwrap(),
        )
        .unwrap();
        let latest = storage::save_snapshot(
            &state,
            out.result,
            out.file_cache,
            out.parsed_file_count,
            out.reused_file_count,
        )
        .unwrap();
        let mut update = request(&latest.snapshot.id, graph.clone());
        update.id = Some(saved.metadata.id.clone());
        update.expected_revision = Some(2);
        assert!(save(&state, update).unwrap_err().contains("Снимок"));
        assert_eq!(
            load(&state, &saved.metadata.id)
                .unwrap()
                .metadata
                .snapshot_id,
            view.snapshot.id
        );
        save(&state, request(&latest.snapshot.id, graph)).unwrap();
        let page = list(&state, &view.project.id, 0, 1).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.next_offset, Some(1));
    }
    #[test]
    fn migration_invalid_graph_foreign_keys_and_future_format_are_explicit() {
        let (_root, state, view, graph) = fixture();
        let before = snapshots_and_cache(&state);
        storage::connect(&state)
            .unwrap()
            .execute_batch("DROP TABLE investigations; PRAGMA user_version=2;")
            .unwrap();
        let reopened = storage::initialize(state.database_path.clone()).unwrap();
        assert_eq!(snapshots_and_cache(&reopened), before);
        assert_eq!(
            storage::connect(&reopened)
                .unwrap()
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            3
        );
        let mut invalid = graph.clone();
        invalid.camera.zoom = f64::NAN;
        assert!(save(&state, request(&view.snapshot.id, invalid)).is_err());
        let mut invalid = graph.clone();
        invalid.selected_node_id = Some("missing.ts".into());
        assert!(save(&state, request(&view.snapshot.id, invalid)).is_err());
        let mut invalid = graph.clone();
        invalid.directory = "../outside".into();
        assert!(save(&state, request(&view.snapshot.id, invalid)).is_err());
        assert!(save(&state, request("absent-snapshot", graph.clone())).is_err());
        let saved = save(&state, request(&view.snapshot.id, graph)).unwrap();
        let connection = storage::connect(&state).unwrap();
        assert!(
            connection
                .execute(
                    "UPDATE investigations SET snapshot_id='absent' WHERE id=?1",
                    [&saved.metadata.id]
                )
                .is_err()
        );
        connection
            .execute(
                "UPDATE investigations SET state_version=99 WHERE id=?1",
                [&saved.metadata.id],
            )
            .unwrap();
        assert_eq!(
            list(&state, &view.project.id, 0, 1).unwrap().items[0].state_version,
            99
        );
        assert!(load(&state, &saved.metadata.id).is_err());
        assert!(delete(&state, &saved.metadata.id, 1).is_err());
        assert_eq!(snapshots_and_cache(&state), before);
    }
}
