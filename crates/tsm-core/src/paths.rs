use std::path::{Path, PathBuf};

pub const APP_IDENTIFIER: &str = "com.topserver.manager";

/// All storage locations used by the application. Application files, runtimes,
/// user config, databases and backups are deliberately kept in separate roots:
///
/// * `config_dir`   – `%APPDATA%\com.topserver.manager` (settings, projects, secrets)
/// * `runtimes_dir` – `%LOCALAPPDATA%\com.topserver.manager\runtimes`
/// * `state_dir`    – `%LOCALAPPDATA%\com.topserver.manager\state` (pids, logs, generated configs)
/// * `data_dir`     – `%LOCALAPPDATA%\TOP Server Manager Data` (databases + backups).
///   This folder is intentionally *outside* the bundle-identifier folders so the
///   uninstaller's "delete application data" option never removes user databases.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub runtimes_dir: PathBuf,
    pub state_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl AppPaths {
    pub fn system_default() -> Self {
        let roaming = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        let local = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
        AppPaths {
            config_dir: roaming.join(APP_IDENTIFIER),
            runtimes_dir: local.join(APP_IDENTIFIER).join("runtimes"),
            state_dir: local.join(APP_IDENTIFIER).join("state"),
            data_dir: local.join("TOP Server Manager Data"),
        }
    }

    /// Everything under one root – used by tests and portable mode.
    pub fn rooted(root: &Path) -> Self {
        AppPaths {
            config_dir: root.join("config"),
            runtimes_dir: root.join("runtimes"),
            state_dir: root.join("state"),
            data_dir: root.join("data"),
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for d in [&self.config_dir, &self.runtimes_dir, &self.state_dir, &self.data_dir] {
            std::fs::create_dir_all(d)?;
        }
        std::fs::create_dir_all(self.logs_dir())?;
        std::fs::create_dir_all(self.databases_dir())?;
        std::fs::create_dir_all(self.backups_dir())?;
        std::fs::create_dir_all(self.generated_dir())?;
        Ok(())
    }

    pub fn settings_file(&self) -> PathBuf { self.config_dir.join("settings.json") }
    pub fn projects_file(&self) -> PathBuf { self.config_dir.join("projects.json") }
    pub fn secrets_file(&self) -> PathBuf { self.config_dir.join("secrets.json") }
    pub fn logs_dir(&self) -> PathBuf { self.state_dir.join("logs") }
    pub fn generated_dir(&self) -> PathBuf { self.state_dir.join("generated") }
    pub fn running_file(&self) -> PathBuf { self.state_dir.join("running.json") }
    pub fn hosting_state_file(&self) -> PathBuf { self.state_dir.join("hosting.json") }
    pub fn databases_dir(&self) -> PathBuf { self.data_dir.join("databases") }
    pub fn backups_dir(&self) -> PathBuf { self.data_dir.join("backups") }
    pub fn project_db_dir(&self, project_id: &str) -> PathBuf { self.databases_dir().join(project_id) }
    pub fn project_backup_dir(&self, project_id: &str) -> PathBuf { self.backups_dir().join(project_id) }
    pub fn project_log_file(&self, project_id: &str, unit: &str) -> PathBuf {
        self.logs_dir().join(format!("{project_id}.{unit}.log"))
    }
}
