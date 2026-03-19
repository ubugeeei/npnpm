use crate::{PackageFilesIndex, StoreDir};
use derive_more::{Display, Error};
use miette::Diagnostic;
use rayon::prelude::*;
use ssri::Integrity;
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreStatus {
    pub checked_packages: usize,
    pub checked_files: usize,
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum StoreStatusError {
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

    #[display("Failed to read store file {path:?}: {error}")]
    ReadStoreFile {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Integrity verification failed for {path:?}: {error}")]
    VerifyStoreFile {
        path: PathBuf,
        #[error(source)]
        error: ssri::Error,
    },
}

impl StoreDir {
    pub fn status(&self) -> Result<StoreStatus, StoreStatusError> {
        let index_paths = self.index_file_paths()?;
        let mut unique_files = HashMap::<PathBuf, Integrity>::new();

        for index_path in &index_paths {
            let index = fs::read_to_string(index_path)
                .map_err(|error| StoreStatusError::ReadDir { dir: index_path.to_path_buf(), error })
                .and_then(|contents| {
                    serde_json::from_str::<PackageFilesIndex>(&contents).map_err(|error| {
                        StoreStatusError::ParseIndexFile { path: index_path.to_path_buf(), error }
                    })
                })?;

            for file_info in index.files.values() {
                let integrity = Integrity::from_str(&file_info.integrity).map_err(|error| {
                    StoreStatusError::ParseIntegrity { path: index_path.to_path_buf(), error }
                })?;
                let is_executable = (file_info.mode & 0o111) == 0o111;
                let store_path = self.cas_file_path_by_integrity(&integrity, is_executable);
                unique_files.entry(store_path).or_insert(integrity);
            }
        }

        unique_files.par_iter().try_for_each(|(path, integrity)| {
            let contents = fs::read(path)
                .map_err(|error| StoreStatusError::ReadStoreFile { path: path.clone(), error })?;
            integrity
                .check(&contents)
                .map(|_| ())
                .map_err(|error| StoreStatusError::VerifyStoreFile { path: path.clone(), error })
        })?;

        Ok(StoreStatus { checked_packages: index_paths.len(), checked_files: unique_files.len() })
    }

    fn index_file_paths(&self) -> Result<Vec<PathBuf>, StoreStatusError> {
        let mut paths = Vec::new();
        let files_dir = self.files();
        if !files_dir.exists() {
            return Ok(paths);
        }

        for head_entry in read_dir(&files_dir)? {
            let head_entry = head_entry
                .map_err(|error| StoreStatusError::ReadDir { dir: files_dir.clone(), error })?;
            let head_path = head_entry.path();
            if !head_path.is_dir() {
                continue;
            }

            for entry in read_dir(&head_path)? {
                let entry = entry
                    .map_err(|error| StoreStatusError::ReadDir { dir: head_path.clone(), error })?;
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

fn read_dir(path: &Path) -> Result<fs::ReadDir, StoreStatusError> {
    fs::read_dir(path).map_err(|error| StoreStatusError::ReadDir { dir: path.to_path_buf(), error })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PackageFileInfo, PackageFilesIndex};
    use ssri::{Algorithm, IntegrityOpts};
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn status_should_verify_each_unique_cas_file_once() {
        let dir = tempdir().unwrap();
        let store_dir = StoreDir::new(dir.path());
        let file_integrity = IntegrityOpts::new()
            .algorithm(Algorithm::Sha512)
            .chain(b"module.exports = 1\n")
            .result();
        let tarball_one =
            IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(b"tarball-1").result();
        let tarball_two =
            IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(b"tarball-2").result();

        store_dir.write_cas_file(b"module.exports = 1\n", false).unwrap();
        let index = PackageFilesIndex {
            files: HashMap::from([(
                "index.js".to_string(),
                PackageFileInfo {
                    checked_at: None,
                    integrity: file_integrity.to_string(),
                    mode: 0o644,
                    size: Some(19),
                },
            )]),
        };
        store_dir.write_index_file(&tarball_one, &index).unwrap();
        store_dir.write_index_file(&tarball_two, &index).unwrap();

        let status = store_dir.status().unwrap();
        assert_eq!(status.checked_packages, 2);
        assert_eq!(status.checked_files, 1);
    }

    #[test]
    fn status_should_fail_when_a_cas_file_is_modified() {
        let dir = tempdir().unwrap();
        let store_dir = StoreDir::new(dir.path());
        let file_contents = b"console.log('ok')\n";
        let file_integrity =
            IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(file_contents).result();
        let tarball_integrity =
            IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(b"tarball").result();

        let (file_path, _) = store_dir.write_cas_file(file_contents, false).unwrap();
        let index = PackageFilesIndex {
            files: HashMap::from([(
                "index.js".to_string(),
                PackageFileInfo {
                    checked_at: None,
                    integrity: file_integrity.to_string(),
                    mode: 0o644,
                    size: Some(file_contents.len() as u64),
                },
            )]),
        };
        store_dir.write_index_file(&tarball_integrity, &index).unwrap();

        fs::write(&file_path, b"tampered\n").unwrap();

        assert!(matches!(store_dir.status(), Err(StoreStatusError::VerifyStoreFile { .. })));
    }
}
