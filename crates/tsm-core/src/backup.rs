//! Project backups (zip of project files, excluding dependency folders) with
//! retention. Database backups are handled by the DatabaseManager.

use crate::error::{CoreError, CoreResult};
use crate::model::Project;
use crate::paths::AppPaths;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

const EXCLUDE_DIRS: &[&str] = &["node_modules", "vendor", ".git", "cache", "logs", "crash-reports"];
const MAX_TOTAL: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct BackupEntry {
    pub project_id: String,
    pub kind: String,
    pub path: PathBuf,
    pub size: u64,
    pub created: String,
}

pub fn list(paths: &AppPaths, project_id: Option<&str>) -> Vec<BackupEntry> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(paths.backups_dir()) else { return out };
    for proj in rd.flatten() {
        let pid = proj.file_name().to_string_lossy().to_string();
        if project_id.map(|p| p != pid).unwrap_or(false) {
            continue;
        }
        for kind in ["files", "db"] {
            if let Ok(files) = std::fs::read_dir(proj.path().join(kind)) {
                for f in files.flatten() {
                    if let Ok(m) = f.metadata() {
                        if m.is_file() {
                            out.push(BackupEntry {
                                project_id: pid.clone(),
                                kind: kind.into(),
                                path: f.path(),
                                size: m.len(),
                                created: m.modified().ok().map(|t| chrono::DateTime::<chrono::Local>::from(t).to_rfc3339()).unwrap_or_default(),
                            });
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

fn walk(dir: &Path, base: &Path, zip: &mut zip::ZipWriter<std::fs::File>, total: &mut u64, skip: &Path) -> CoreResult<()> {
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for e in std::fs::read_dir(dir)?.flatten() {
        let p = e.path();
        if p == skip {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let ft = e.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            if EXCLUDE_DIRS.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
                continue;
            }
            walk(&p, base, zip, total, skip)?;
        } else if ft.is_file() {
            let len = e.metadata()?.len();
            *total += len;
            if *total > MAX_TOTAL {
                return Err(CoreError::Other("project is larger than 4 GB – back it up manually".into()));
            }
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            zip.start_file(rel, opts).map_err(|e| CoreError::Other(e.to_string()))?;
            let mut f = std::fs::File::open(&p)?;
            std::io::copy(&mut f, zip)?;
        }
    }
    Ok(())
}

pub fn backup_project_files(paths: &AppPaths, project: &Project) -> CoreResult<PathBuf> {
    if !project.folder.is_dir() {
        return Err(CoreError::InvalidConfig("project folder does not exist".into()));
    }
    let dir = paths.project_backup_dir(&project.id).join("files");
    std::fs::create_dir_all(&dir)?;
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dest = dir.join(format!("{}-{ts}.zip", crate::util::slug(&project.name)));
    let file = std::fs::File::create(&dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let mut total = 0;
    let res = walk(&project.folder, &project.folder, &mut zip, &mut total, &paths.data_dir);
    match res.and_then(|_| zip.finish().map(|mut f| f.flush()).map_err(|e| CoreError::Other(e.to_string()))?.map_err(CoreError::from)) {
        Ok(()) => Ok(dest),
        Err(e) => {
            let _ = std::fs::remove_file(&dest);
            Err(e)
        }
    }
}

/// Keep the newest `keep` backups of each kind for a project.
pub fn apply_retention(paths: &AppPaths, project_id: &str, keep: u32) -> usize {
    let mut removed = 0;
    for kind in ["files", "db"] {
        let mut v: Vec<BackupEntry> = list(paths, Some(project_id)).into_iter().filter(|b| b.kind == kind).collect();
        v.sort_by(|a, b| b.created.cmp(&a.created));
        for old in v.iter().skip(keep.max(1) as usize) {
            // only touch files inside our backup folder
            if crate::security::path_is_within(&old.path, &paths.backups_dir()) && std::fs::remove_file(&old.path).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}
