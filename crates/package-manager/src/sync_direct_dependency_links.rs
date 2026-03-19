use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Display, Error, Diagnostic)]
pub enum SyncDirectDependencyLinksError {
    #[display("Failed to read node_modules directory at {dir:?}: {error}")]
    ReadModulesDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to read scoped directory at {dir:?}: {error}")]
    ReadScopeDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove stale dependency path at {path:?}: {error}")]
    RemoveDependencyPath {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

pub fn sync_direct_dependency_links(
    modules_dir: &Path,
    desired_packages: impl IntoIterator<Item = String>,
) -> Result<(), SyncDirectDependencyLinksError> {
    if !modules_dir.exists() {
        return Ok(());
    }

    let desired_packages = desired_packages.into_iter().collect::<HashSet<_>>();
    for (package_name, path) in current_direct_dependency_paths(modules_dir)? {
        if desired_packages.contains(&package_name) {
            continue;
        }

        remove_path(&path)?;

        if let Some(scope_dir) = path.parent() {
            if scope_dir != modules_dir
                && scope_dir.read_dir().is_ok_and(|mut dir| dir.next().is_none())
            {
                fs::remove_dir(scope_dir).map_err(|error| {
                    SyncDirectDependencyLinksError::RemoveDependencyPath {
                        path: scope_dir.to_path_buf(),
                        error,
                    }
                })?;
            }
        }
    }

    Ok(())
}

fn current_direct_dependency_paths(
    modules_dir: &Path,
) -> Result<Vec<(String, PathBuf)>, SyncDirectDependencyLinksError> {
    let mut packages = Vec::new();
    for entry in fs::read_dir(modules_dir).map_err(|error| {
        SyncDirectDependencyLinksError::ReadModulesDir { dir: modules_dir.to_path_buf(), error }
    })? {
        let entry = entry.map_err(|error| SyncDirectDependencyLinksError::ReadModulesDir {
            dir: modules_dir.to_path_buf(),
            error,
        })?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        if name.starts_with('@') && path.is_dir() {
            for scoped_entry in fs::read_dir(&path).map_err(|error| {
                SyncDirectDependencyLinksError::ReadScopeDir { dir: path.clone(), error }
            })? {
                let scoped_entry = scoped_entry.map_err(|error| {
                    SyncDirectDependencyLinksError::ReadScopeDir { dir: path.clone(), error }
                })?;
                let scoped_name = scoped_entry.file_name().to_string_lossy().to_string();
                packages.push((format!("{name}/{scoped_name}"), scoped_entry.path()));
            }
        } else {
            packages.push((name, path));
        }
    }

    Ok(packages)
}

fn remove_path(path: &Path) -> Result<(), SyncDirectDependencyLinksError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        SyncDirectDependencyLinksError::RemoveDependencyPath { path: path.to_path_buf(), error }
    })?;

    let result = if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)
    } else {
        fs::remove_dir_all(path)
    };

    result.map_err(|error| SyncDirectDependencyLinksError::RemoveDependencyPath {
        path: path.to_path_buf(),
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacquet_fs::symlink_dir;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn should_remove_stale_direct_dependency_links() {
        let dir = tempdir().unwrap();
        let modules_dir = dir.path().join("node_modules");
        let target_dir = dir.path().join("target");
        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(modules_dir.join("@scope")).unwrap();
        symlink_dir(&target_dir, &modules_dir.join("kept")).unwrap();
        symlink_dir(&target_dir, &modules_dir.join("stale")).unwrap();
        symlink_dir(&target_dir, &modules_dir.join("@scope/pkg")).unwrap();

        sync_direct_dependency_links(&modules_dir, ["kept".to_string()]).unwrap();

        assert!(modules_dir.join("kept").exists());
        assert!(!modules_dir.join("stale").exists());
        assert!(!modules_dir.join("@scope/pkg").exists());
        assert!(!modules_dir.join("@scope").exists());
    }
}
