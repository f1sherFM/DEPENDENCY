use crate::{
    analysis::AnalysisResult,
    investigations::{self, GraphState, Investigation},
    snapshot::FileFact,
    storage::{self, AppState, SnapshotRecord},
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, io::Read};
const LIMIT: u64 = 64 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Bundle {
    format: String,
    version: u32,
    state_version: i64,
    name: String,
    note: String,
    graph: GraphState,
    snapshot: SnapshotRecord,
    result: AnalysisResult,
    inventory: Vec<FileFact>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub fingerprint: String,
    pub name: String,
    pub note: String,
    pub snapshot_id: String,
    pub created_at: i64,
    pub analyzer_version: String,
    pub rules_version: String,
    pub nodes: usize,
    pub edges: usize,
    pub files: usize,
    pub origin: String,
}
fn validate(b: &Bundle) -> Result<(), String> {
    if b.format != "dependency-investigation"
        || b.version != 1
        || b.state_version != investigations::STATE_VERSION
    {
        return Err("Неподдерживаемая версия переносимого исследования".into());
    }
    if b.name.trim().is_empty() || b.name.chars().count() > 120 || b.note.chars().count() > 16000 {
        return Err("Недопустимый размер названия или заметки".into());
    }
    if b.result.nodes.len() > 100_000
        || b.result.edges.len() > 1_000_000
        || b.inventory.len() > 100_000
        || b.snapshot.parsed_file_count > 100_000
        || b.snapshot.reused_file_count > 100_000
    {
        return Err("Превышены лимиты снимка".into());
    }
    let ids: HashSet<_> = b.result.nodes.iter().map(|n| n.id.as_str()).collect();
    let edges: HashSet<_> = b.result.edges.iter().map(|e| e.id.as_str()).collect();
    let paths: HashSet<_> = b.inventory.iter().map(|f| f.path.as_str()).collect();
    if ids.len() != b.result.nodes.len()
        || edges.len() != b.result.edges.len()
        || paths.len() != b.inventory.len()
        || b.result
            .edges
            .iter()
            .any(|e| !ids.contains(e.source.as_str()) || !ids.contains(e.target.as_str()))
    {
        return Err("Снимок содержит дубликаты или разорванные связи".into());
    }
    b.graph.validate(&b.result)?;
    if serde_json::to_vec(&b.graph)
        .map_err(|e| e.to_string())?
        .len()
        > 65536
    {
        return Err("Состояние графа превышает 64 КиБ".into());
    }
    Ok(())
}
fn read(path: &str) -> Result<(Bundle, String), String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("Файл превышает 64 МиБ".into());
    }
    let b: Bundle = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Некорректный файл исследования: {e}"))?;
    validate(&b)?;
    Ok((b, format!("{:x}", Sha256::digest(&bytes))))
}
pub fn preview(path: &str) -> Result<Preview, String> {
    let (b, fingerprint) = read(path)?;
    Ok(Preview {
        fingerprint,
        name: b.name,
        note: b.note,
        snapshot_id: b.snapshot.id,
        created_at: b.snapshot.created_at,
        analyzer_version: b.snapshot.analyzer_version,
        rules_version: b.snapshot.rules_version,
        nodes: b.result.nodes.len(),
        edges: b.result.edges.len(),
        files: b.inventory.len(),
        origin: b.result.summary.root_path,
    })
}
pub fn export(state: &AppState, id: &str, path: &str) -> Result<(), String> {
    let record = investigations::load(state, id)?;
    let (snapshot, result) = storage::read_impact_snapshot(state, &record.metadata.snapshot_id)?;
    let inventory = storage::read_inventory(state, &snapshot.id)?;
    let b = Bundle {
        format: "dependency-investigation".into(),
        version: 1,
        state_version: investigations::STATE_VERSION,
        name: record.metadata.name,
        note: record.note,
        graph: record.graph,
        snapshot,
        result,
        inventory,
    };
    validate(&b)?;
    let bytes = serde_json::to_vec(&b).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("Экспорт превышает 64 МиБ".into());
    }
    // Never truncate an existing user file, including the application database.
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Выберите новое имя файла: {e}"))?;
    if let Err(e) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(e.to_string());
    }
    Ok(())
}
pub fn import(
    state: &AppState,
    path: &str,
    fingerprint: &str,
    project_id: &str,
) -> Result<Investigation, String> {
    let (b, actual) = read(path)?;
    if actual != fingerprint {
        return Err("Файл изменился после просмотра. Откройте его заново".into());
    }
    let mut conn = storage::connect(state)?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
            [project_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("Выбранный проект отсутствует".into());
    }
    let sid = uuid::Uuid::new_v4().to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let now = storage::now();
    tx.execute("INSERT INTO snapshots(id,project_id,created_at,analyzer_version,rules_version,parsed_file_count,reused_file_count,result_json,basis_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![sid,project_id,b.snapshot.created_at,b.snapshot.analyzer_version,b.snapshot.rules_version,b.snapshot.parsed_file_count as i64,b.snapshot.reused_file_count as i64,serde_json::to_string(&b.result).map_err(|e|e.to_string())?,b.result.basis.as_ref().map(serde_json::to_string).transpose().map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    for mut fact in b.inventory {
        let imports = serde_json::to_string(&fact.imports).map_err(|e| e.to_string())?;
        fact.imports.clear();
        let metadata = serde_json::to_string(&fact).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO snapshot_files(snapshot_id,relative_path,metadata_json,imports_json) VALUES(?1,?2,?3,?4)",params![sid,fact.path,metadata,imports]).map_err(|e|e.to_string())?;
    }
    tx.execute("INSERT INTO investigations(id,snapshot_id,name,note,state_version,revision,graph_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,1,?6,?7,?7)",params![id,sid,b.name.trim(),b.note,investigations::STATE_VERSION,serde_json::to_string(&b.graph).map_err(|e|e.to_string())?,now]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    investigations::load(state, &id)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, AppState, Investigation) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("source");
        fs::create_dir(&project).unwrap();
        fs::write(
            project.join("a.ts"),
            "import './b'; // PRIVATE_SOURCE_MARKER",
        )
        .unwrap();
        fs::write(project.join("b.ts"), "export const value=1;").unwrap();
        let state = storage::initialize(dir.path().join("data.sqlite")).unwrap();
        let out = crate::analysis::analyze_project(project.to_str().unwrap(), vec![]).unwrap();
        let view = storage::save_captured_snapshot(
            &state,
            out.result,
            out.file_cache,
            out.parsed_file_count,
            out.reused_file_count,
            vec![FileFact {
                path: "a.ts".into(),
                kind: "source".into(),
                mode: 0,
                content_hash: None,
                blob_oid: None,
                status: "captured".into(),
                reason: None,
                parser_version: Some("js-ts-parser-2".into()),
                imports: vec![],
                parse_error: Some(false),
            }],
        )
        .unwrap();
        let graph = GraphState {
            mode: "files".into(),
            directory: "".into(),
            query: "".into(),
            show_packages: false,
            edge_kind: "".into(),
            focus: None,
            direction: "incoming".into(),
            selected_node_id: Some("a.ts".into()),
            camera: investigations::Camera {
                center_x: 135.,
                center_y: 70.,
                zoom: 1.,
            },
            details_open: true,
        };
        let saved = investigations::save(
            &state,
            investigations::SaveRequest {
                id: None,
                expected_revision: None,
                snapshot_id: view.snapshot.id,
                name: "Research".into(),
                note: "Data only <script>".into(),
                graph,
            },
        )
        .unwrap();
        (dir, state, saved)
    }
    #[test]
    fn roundtrip_preserves_history_cache_and_inventory_without_sources() {
        let (dir, state, record) = fixture();
        let path = dir.path().join("export.json");
        let path = path.to_str().unwrap();
        let before = fs::read(&state.database_path).unwrap();
        export(&state, &record.metadata.id, path).unwrap();
        assert_eq!(before, fs::read(&state.database_path).unwrap());
        assert!(
            !fs::read_to_string(path)
                .unwrap()
                .contains("PRIVATE_SOURCE_MARKER")
        );
        assert!(export(&state, &record.metadata.id, path).is_err());
        let p = preview(path).unwrap();
        assert_eq!(p.files, 1);
        fs::remove_dir_all(dir.path().join("source")).unwrap();
        let copy = import(&state, path, &p.fingerprint, &record.metadata.project_id).unwrap();
        assert_ne!(copy.metadata.id, record.metadata.id);
        assert_ne!(copy.metadata.snapshot_id, record.metadata.snapshot_id);
        assert_eq!(copy.graph, record.graph);
        assert_eq!(copy.note, record.note);
        let project = storage::load_project(&state, &record.metadata.project_id).unwrap();
        assert_eq!(project.snapshot.id, record.metadata.snapshot_id);
        assert_eq!(
            storage::load_cache(&state, Some(&record.metadata.project_id))
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            storage::read_inventory(&state, &copy.metadata.snapshot_id)
                .unwrap()
                .len(),
            storage::read_inventory(&state, &record.metadata.snapshot_id)
                .unwrap()
                .len()
        );
    }
    #[test]
    fn invalid_changed_and_failed_imports_never_leave_partial_snapshot() {
        let (dir, state, record) = fixture();
        let path = dir.path().join("export.json");
        let path = path.to_str().unwrap();
        export(&state, &record.metadata.id, path).unwrap();
        let p = preview(path).unwrap();
        let original = fs::read(path).unwrap();
        let mut bytes = original.clone();
        bytes.push(b' ');
        fs::write(path, bytes).unwrap();
        assert!(import(&state, path, &p.fingerprint, &record.metadata.project_id).is_err());
        fs::write(path, &original).unwrap();
        let conn = storage::connect(&state).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_transfer BEFORE INSERT ON investigations BEGIN SELECT RAISE(ABORT,'test rollback'); END;").unwrap();
        assert!(import(&state, path, &p.fingerprint, &record.metadata.project_id).is_err());
        let count: i64 = conn
            .query_row("SELECT count(*) FROM snapshots", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value["version"] = 99.into();
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(preview(path).is_err());
        value["version"] = 1.into();
        value["result"]["edges"][0]["target"] = "missing".into();
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(preview(path).is_err());
    }
}
