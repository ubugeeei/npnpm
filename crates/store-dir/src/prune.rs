use crate::{PackageFilesIndex, StoreDir};
use derive_more::{Display, Error};
use miette::Diagnostic;
use ssri::Integrity;
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

/// Error type of [`StoreDir::prune`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PruneError {
    #[display("Failed to read directory {dir:?}: {error}")]
    ReadDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to parse store index file {path:?}: {error}")]
    ParseIndexFile {
        path: PathBuf,
        #[error(source)]
        error: serde_json::Error,
    },

    #[display("Failed to parse integrity in {path:?}: {error}")]
    ParseIntegrity {
        path: PathBuf,
        #[error(source)]
        error: ssri::Error,
    },

    #[display("Failed to remove path {path:?}: {error}")]
    RemovePath {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

impl StoreDir {
    /// Remove store files that are not referenced by any tarball index and clear tmp files.
    pub fn prune(&self) -> Result<(), PruneError> {
        let referenced_files = self.referenced_store_files()?;
        self.remove_orphan_cas_files(&referenced_files)?;
        self.clear_tmp_dir()?;
        Ok(())
    }

    fn referenced_store_files(&self) -> Result<HashSet<PathBuf>, PruneError> {
        let mut referenced = HashSet::new();

        for index_path in self.prune_index_file_paths()? {
            let contents = fs::read_to_string(&index_path)
                .map_err(|error| PruneError::ReadDir { dir: index_path.clone(), error })?;
            let index = serde_json::from_str::<PackageFilesIndex>(&contents)
                .map_err(|error| PruneError::ParseIndexFile { path: index_path.clone(), error })?;

            for file_info in index.files.values() {
                let integrity = Integrity::from_str(&file_info.integrity).map_err(|error| {
                    PruneError::ParseIntegrity { path: index_path.clone(), error }
                })?;
                let is_executable = (file_info.mode & 0o111) == 0o111;
                referenced.insert(self.cas_file_path_by_integrity(&integrity, is_executable));
            }
        }

        Ok(referenced)
    }

    fn remove_orphan_cas_files(
        &self,
        referenced_files: &HashSet<PathBuf>,
    ) -> Result<(), PruneError> {
        let files_dir = self.files();
        if !files_dir.exists() {
            return Ok(());
        }

        for head_entry in read_dir(&files_dir)? {
            let head_entry = head_entry
                .map_err(|error| PruneError::ReadDir { dir: files_dir.clone(), error })?;
            let head_path = head_entry.path();
            if !head_path.is_dir() {
                continue;
            }

            for entry in read_dir(&head_path)? {
                let entry =
                    entry.map_err(|error| PruneError::ReadDir { dir: head_path.clone(), error })?;
                let path = entry.path();
                if !path.is_file()
                    || path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().ends_with("-index.json"))
                    || referenced_files.contains(&path)
                {
                    continue;
                }

                fs::remove_file(&path)
                    .map_err(|error| PruneError::RemovePath { path: path.clone(), error })?;
            }
        }

        Ok(())
    }

    fn clear_tmp_dir(&self) -> Result<(), PruneError> {
        let tmp_dir = self.tmp();
        if !tmp_dir.exists() {
            return Ok(());
        }

        for entry in read_dir(&tmp_dir)? {
            let entry =
                entry.map_err(|error| PruneError::ReadDir { dir: tmp_dir.clone(), error })?;
            let path = entry.path();
            let remove =
                if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            remove.map_err(|error| PruneError::RemovePath { path, error })?;
        }

        Ok(())
    }

    fn prune_index_file_paths(&self) -> Result<Vec<PathBuf>, PruneError> {
        let mut paths = Vec::new();
        let files_dir = self.files();
        if !files_dir.exists() {
            return Ok(paths);
        }

        for head_entry in read_dir(&files_dir)? {
            let head_entry = head_entry
                .map_err(|error| PruneError::ReadDir { dir: files_dir.clone(), error })?;
            let head_path = head_entry.path();
            if !head_path.is_dir() {
                continue;
            }

            for entry in read_dir(&head_path)? {
                let entry =
                    entry.map_err(|error| PruneError::ReadDir { dir: head_path.clone(), error })?;
                let path = entry.path();
                if path.is_file()
                    && path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().ends_with("-index.json"))
                {
                    paths.push(path);
                }
            }
        }

        Ok(paths)
    }
}

fn read_dir(path: &Path) -> Result<fs::ReadDir, PruneError> {
    fs::read_dir(path).map_err(|error| PruneError::ReadDir { dir: path.to_path_buf(), error })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PackageFileInfo, PackageFilesIndex};
    use ssri::{Algorithm, IntegrityOpts};
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn prune_should_remove_orphaned_cas_files_and_tmp_entries() {
        let dir = tempdir().unwrap();
        let store_dir = StoreDir::new(dir.path());
        let referenced_contents = b"console.log('kept')\n";
        let orphan_contents = b"console.log('remove')\n";
        let referenced_integrity =
            IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(referenced_contents).result();
        let tarball_integrity =
            IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(b"tarball").result();

        let (referenced_path, _) = store_dir.write_cas_file(referenced_contents, false).unwrap();
        let (orphan_path, _) = store_dir.write_cas_file(orphan_contents, false).unwrap();

        let index = PackageFilesIndex {
            files: HashMap::from([(
                "index.js".to_string(),
                PackageFileInfo {
                    checked_at: None,
                    integrity: referenced_integrity.to_string(),
                    mode: 0o644,
                    size: Some(referenced_contents.len() as u64),
                },
            )]),
        };
        store_dir.write_index_file(&tarball_integrity, &index).unwrap();

        let tmp_file = store_dir.tmp().join("stale.txt");
        fs::create_dir_all(store_dir.tmp()).unwrap();
        fs::write(&tmp_file, b"stale").unwrap();

        store_dir.prune().unwrap();

        assert!(referenced_path.exists());
        assert!(!orphan_path.exists());
        assert!(!tmp_file.exists());
    }
}
