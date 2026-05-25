use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{atomic_write, WorkspaceError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalMarker {
    pub issue_id: String,
    pub issue_id_path_key: String,
    pub workspace_key: String,
    pub terminal_since: DateTime<Utc>,
    pub state: String,
    pub gc_attempts: u32,
    pub last_attempt_at: Option<DateTime<Utc>>,
}

pub(crate) fn dir(root: &Path) -> PathBuf {
    root.join(".symphony").join("gc").join("terminal")
}

pub(crate) fn path(root: &Path, issue_id_path_key: &str) -> PathBuf {
    dir(root).join(format!("{issue_id_path_key}.json"))
}

pub(crate) async fn write(root: &Path, marker: &TerminalMarker) -> Result<(), WorkspaceError> {
    let bytes = serde_json::to_vec_pretty(marker).map_err(|e| WorkspaceError::MetadataInvalid {
        reason: e.to_string(),
    })?;
    atomic_write(&path(root, &marker.issue_id_path_key), &bytes).await
}

pub(crate) async fn read(path: &Path) -> Result<TerminalMarker, WorkspaceError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?;
    serde_json::from_slice(&bytes).map_err(|e| WorkspaceError::MetadataInvalid {
        reason: e.to_string(),
    })
}

pub(crate) async fn list(root: &Path) -> Result<Vec<(PathBuf, TerminalMarker)>, WorkspaceError> {
    let marker_dir = dir(root);
    let mut markers = Vec::new();
    let mut entries = match tokio::fs::read_dir(&marker_dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(markers),
        Err(e) => return Err(WorkspaceError::Io { source: e }),
    };

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| WorkspaceError::Io { source: e })?
    {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        markers.push((path.clone(), read(&path).await?));
    }

    Ok(markers)
}

pub(crate) async fn remove_if_same_terminal_since(
    path: &Path,
    expected: &TerminalMarker,
) -> Result<bool, WorkspaceError> {
    match read(path).await {
        Ok(current)
            if current.issue_id_path_key == expected.issue_id_path_key
                && current.terminal_since == expected.terminal_since =>
        {
            tokio::fs::remove_file(path)
                .await
                .map_err(|e| WorkspaceError::Io { source: e })?;
            Ok(true)
        }
        Ok(_) => Ok(false),
        Err(WorkspaceError::Io { source }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}
