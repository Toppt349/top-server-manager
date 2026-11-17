//! Project (server profile) persistence and validation.

use crate::error::{CoreError, CoreResult};
use crate::model::*;
use crate::security;
use crate::util::{now_iso, read_json_or_default, write_json_atomic};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProjectsFile {
    version: u32,
    projects: Vec<Project>,
}

pub struct ProjectManager {
    path: PathBuf,
    projects: RwLock<Vec<Project>>,
}

/// Exported profile format (`.tsmprofile.json`). Contains no secret values.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportedProfile {
    pub format: String,
    pub exported_at: String,
    pub project: Project,
}

impl ProjectManager {
    pub fn load(path: PathBuf) -> CoreResult<Self> {
        let file: ProjectsFile = read_json_or_default(&path)?;
        Ok(ProjectManager { path, projects: RwLock::new(file.projects) })
    }

    fn persist(&self, list: &[Project]) -> CoreResult<()> {
        write_json_atomic(&self.path, &ProjectsFile { version: 2, projects: list.to_vec() })
    }

    pub fn list(&self) -> Vec<Project> {
        self.projects.read().unwrap().clone()
    }

    pub fn get(&self, id: &str) -> CoreResult<Project> {
        self.projects
            .read()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| CoreError::ProjectNotFound(id.to_string()))
    }

    pub fn validate(&self, p: &Project, existing_id: Option<&str>) -> CoreResult<()> {
        let name = p.name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(CoreError::InvalidConfig("server name must be 1-80 characters".into()));
        }
        if !p.folder.is_absolute() {
            return Err(CoreError::InvalidConfig("project folder must be an absolute path".into()));
        }
        if p.port == 0 {
            return Err(CoreError::InvalidConfig("port must be between 1 and 65535".into()));
        }
        if !p.internal {
            if let Some(other) = self
                .projects
                .read()
                .unwrap()
                .iter()
                .find(|o| o.port == p.port && Some(o.id.as_str()) != existing_id && !o.internal)
            {
                return Err(CoreError::InvalidConfig(format!("port {} is already assigned to '{}'", p.port, other.name)));
            }
        }
        for k in p.env.keys() {
            security::validate_env_key(k)?;
        }
        security::validate_args(&p.args)?;
        if p.web_root.contains("..") {
            return Err(CoreError::InvalidConfig("web root must stay inside the project folder".into()));
        }
        if p.database.driver != DbDriver::None {
            security::validate_db_identifier(&p.database.name)?;
            if p.database.driver.is_server() && !p.database.user.is_empty() {
                security::validate_db_identifier(&p.database.user)?;
            }
        }
        if let Some(h) = &p.hosting.hostname {
            if !h.is_empty() {
                security::validate_hostname(h)?;
            }
        }
        if let Some(l) = p.limits.cpu_percent {
            if l == 0 || l > 100 {
                return Err(CoreError::InvalidConfig("CPU limit must be 1-100%".into()));
            }
        }
        if let Some(sqlite) = &p.database.sqlite_path {
            if security::path_is_within(sqlite, &p.web_root_path()) {
                return Err(CoreError::InvalidConfig(
                    "the SQLite database file must not be inside the public web root".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn create(&self, mut p: Project) -> CoreResult<Project> {
        p.name = p.name.trim().to_string();
        if p.id.is_empty() {
            p.id = format!("{}-{}", crate::util::slug(&p.name), &uuid::Uuid::new_v4().simple().to_string()[..6]);
        }
        self.validate(&p, None)?;
        p.created_at = now_iso();
        p.updated_at = p.created_at.clone();
        let mut list = self.projects.write().unwrap();
        if list.iter().any(|o| o.id == p.id) {
            return Err(CoreError::InvalidConfig(format!("project id '{}' already exists", p.id)));
        }
        list.push(p.clone());
        self.persist(&list)?;
        Ok(p)
    }

    pub fn update(&self, mut p: Project) -> CoreResult<Project> {
        self.validate(&p, Some(&p.id.clone()))?;
        let mut list = self.projects.write().unwrap();
        let slot = list.iter_mut().find(|o| o.id == p.id).ok_or_else(|| CoreError::ProjectNotFound(p.id.clone()))?;
        p.created_at = slot.created_at.clone();
        p.updated_at = now_iso();
        *slot = p.clone();
        self.persist(&list)?;
        Ok(p)
    }

    /// Removes the profile only. Project files and databases are never deleted.
    pub fn delete(&self, id: &str) -> CoreResult<()> {
        let mut list = self.projects.write().unwrap();
        let before = list.len();
        list.retain(|p| p.id != id);
        if list.len() == before {
            return Err(CoreError::ProjectNotFound(id.to_string()));
        }
        self.persist(&list)
    }

    pub fn next_free_port(&self, start: u16) -> u16 {
        let used: Vec<u16> = self.projects.read().unwrap().iter().map(|p| p.port).collect();
        (start..u16::MAX).find(|p| !used.contains(p) && !crate::ports::tcp_in_use(*p)).unwrap_or(start)
    }

    pub fn duplicate(&self, id: &str) -> CoreResult<Project> {
        let mut p = self.get(id)?;
        p.id = String::new();
        p.name = format!("{} (copy)", p.name);
        p.port = self.next_free_port(p.port.saturating_add(1));
        p.auto_start = false;
        p.local_domain = None;
        p.hosting = HostingConfig { provider: ProviderId::Local, ..Default::default() };
        if p.database.driver != DbDriver::None {
            p.database.name = format!("{}_copy", p.database.name).chars().take(64).collect();
            p.database.sqlite_path = None;
        }
        self.create(p)
    }

    pub fn export(&self, id: &str, path: &Path) -> CoreResult<()> {
        let mut project = self.get(id)?;
        project.hosting.exposure_consent = false;
        let prof = ExportedProfile { format: "tsm-profile/2".into(), exported_at: now_iso(), project };
        write_json_atomic(path, &prof)
    }

    pub fn import(&self, path: &Path) -> CoreResult<Project> {
        let data = std::fs::read(path)?;
        let prof: ExportedProfile = serde_json::from_slice(&data)?;
        if !prof.format.starts_with("tsm-profile/") {
            return Err(CoreError::InvalidConfig("not a TOP Server Manager profile".into()));
        }
        let mut p = prof.project;
        p.id = String::new();
        p.auto_start = false;
        p.hosting.exposure_consent = false;
        p.hosting.token_secret = None;
        if self.list().iter().any(|o| o.port == p.port) {
            p.port = self.next_free_port(p.port);
        }
        self.create(p)
    }
}
