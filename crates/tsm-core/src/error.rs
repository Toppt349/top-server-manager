use serde::Serialize;

/// Error type returned by every core operation. The `code` is stable and is
/// used by the UI to show a localized explanation / fix-it hint.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Project not found: {0}")]
    ProjectNotFound(String),
    #[error("Port {port} is already in use{}", owner.as_ref().map(|o| format!(" by {o}")).unwrap_or_default())]
    PortConflict { port: u16, owner: Option<String> },
    #[error("Runtime not installed: {0}")]
    RuntimeMissing(String),
    #[error("Executable not found: {0}")]
    MissingExecutable(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Hosting provider error: {0}")]
    Provider(String),
    #[error("Approval required: {0}")]
    ApprovalRequired(String),
    #[error("Operation already in progress: {0}")]
    Busy(String),
    #[error("Verification failed: {0}")]
    Verification(String),
    #[error("Not supported: {0}")]
    Unsupported(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl CoreError {
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::InvalidConfig(_) => "invalid_config",
            CoreError::ProjectNotFound(_) => "project_not_found",
            CoreError::PortConflict { .. } => "port_conflict",
            CoreError::RuntimeMissing(_) => "runtime_missing",
            CoreError::MissingExecutable(_) => "missing_executable",
            CoreError::PermissionDenied(_) => "permission_denied",
            CoreError::Database(_) => "database_error",
            CoreError::Network(_) => "network_error",
            CoreError::Provider(_) => "provider_error",
            CoreError::ApprovalRequired(_) => "approval_required",
            CoreError::Busy(_) => "busy",
            CoreError::Verification(_) => "verification_failed",
            CoreError::Unsupported(_) => "unsupported",
            CoreError::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => "permission_denied",
            CoreError::Io(_) => "io_error",
            CoreError::Other(_) => "error",
        }
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(e: rusqlite::Error) -> Self {
        CoreError::Database(e.to_string())
    }
}
impl From<serde_json::Error> for CoreError {
    fn from(e: serde_json::Error) -> Self {
        CoreError::InvalidConfig(e.to_string())
    }
}
impl From<reqwest::Error> for CoreError {
    fn from(e: reqwest::Error) -> Self {
        CoreError::Network(e.to_string())
    }
}
impl From<anyhow::Error> for CoreError {
    fn from(e: anyhow::Error) -> Self {
        CoreError::Other(e.to_string())
    }
}

/// Serializable error representation sent over IPC.
#[derive(Debug, Serialize, Clone)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

impl From<&CoreError> for ErrorPayload {
    fn from(e: &CoreError) -> Self {
        ErrorPayload { code: e.code().to_string(), message: e.to_string() }
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
