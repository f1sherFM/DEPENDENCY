//! Opt-in observation files; no listener, code execution or database writes.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct AgentAccess {
    directory: PathBuf,
    session_id: String,
}
impl AgentAccess {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            session_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    pub fn enabled(&self) -> bool {
        std::fs::File::open(self.directory.join("agent-access.json"))
            .ok()
            .and_then(|file| {
                let mut bytes = Vec::new();
                file.take(4097).read_to_end(&mut bytes).ok()?;
                (bytes.len() <= 4096).then_some(bytes)
            })
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|config| config.get("enabled") == Some(&Value::Bool(true)))
    }
    pub fn publish(&self, state: Value) -> Result<(), String> {
        if !self.enabled() {
            return Err("Доступ агента выключен".into());
        }
        if !state.is_object()
            || serde_json::to_vec(&state).map_err(|e| e.to_string())?.len() > 131072
        {
            return Err("Состояние интерфейса превышает лимит 128 КиБ".into());
        }
        let folder = self.directory.join("agent-sessions");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let observed_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis() as u64;
        let bytes = serde_json::to_vec(&json!({"schemaVersion":1,"sessionId":self.session_id,"pid":std::process::id(),"observedAt":observed_at,"state":state})).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(&folder).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.persist(folder.join(format!("{}.json", self.session_id)))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn access_is_opt_in_bounded_atomic_and_can_be_revoked_live() {
        let root = tempfile::tempdir().unwrap();
        let access = AgentAccess::new(root.path().into());
        assert!(access.publish(json!({"view":"graph"})).is_err());
        assert!(!root.path().join("agent-sessions").exists());
        std::fs::write(root.path().join("agent-access.json"), r#"{"enabled":true}"#).unwrap();
        access.publish(json!({"snapshotId":"old"})).unwrap();
        access.publish(json!({"snapshotId":"new"})).unwrap();
        let path = root
            .path()
            .join("agent-sessions")
            .join(format!("{}.json", access.session_id));
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["state"]["snapshotId"], "new");
        assert!(access.publish(json!({"text":"x".repeat(140000)})).is_err());
        assert_eq!(
            saved,
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap()
        );
        std::fs::write(
            root.path().join("agent-access.json"),
            r#"{"enabled":false}"#,
        )
        .unwrap();
        assert!(access.publish(json!({})).is_err());
    }
}
