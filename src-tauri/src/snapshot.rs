use crate::analysis::CachedImport;
use serde::{Deserialize, Serialize};

pub const PROFILE: &str = "repository-only-v1";
pub const POLICY: &str = "capture-v2:.venv,venv,__pycache__,.tox,.mypy_cache,.pytest_cache,.git,node_modules,target,dist,build,coverage,.next,.turbo,vendor,out,.dependency-capture";
pub const EXCLUDED: [&str; 17] = [
    ".git",
    ".venv",
    "venv",
    "__pycache__",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    "node_modules",
    "target",
    "dist",
    "build",
    "coverage",
    ".next",
    ".turbo",
    "vendor",
    "out",
    ".dependency-capture",
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Basis {
    pub kind: String,
    pub repository_identity: Option<String>,
    pub project_scope: String,
    pub requested_ref: Option<String>,
    pub commit_oid: Option<String>,
    pub tree_oid: Option<String>,
    pub observed_head_oid: Option<String>,
    pub branch_label: Option<String>,
    pub index_fingerprint: Option<String>,
    pub capture_fingerprint: String,
    pub started_at: u64,
    pub completed_at: u64,
    pub capture_consistency: String,
    pub resolution_profile: String,
    pub capture_policy_hash: String,
    pub omitted_count: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFact {
    pub path: String,
    pub kind: String,
    pub mode: u32,
    pub content_hash: Option<String>,
    pub blob_oid: Option<String>,
    pub status: String,
    pub reason: Option<String>,
    pub parser_version: Option<String>,
    pub imports: Vec<CachedImport>,
    #[serde(default)]
    pub parse_error: Option<bool>,
}
pub fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
pub fn check_cancel(cancel: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        Err("Операция отменена".into())
    } else {
        Ok(())
    }
}
