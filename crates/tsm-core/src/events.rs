use crate::model::{HostingState, LogLine, ServerState};
use serde::Serialize;

/// Events pushed from the engine to any listener (the Tauri UI bridge
/// forwards them to the webview as `tsm://event`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    ServerState { state: ServerState },
    Log { project_id: String, line: LogLine },
    Hosting { state: HostingState },
    RuntimeProgress { runtime: String, phase: String, percent: Option<f32>, message: String },
    Database { project_id: String, message: String },
    Notice { level: String, message: String },
}

pub type EventSender = tokio::sync::broadcast::Sender<EngineEvent>;

pub fn channel() -> EventSender {
    tokio::sync::broadcast::channel(4096).0
}
