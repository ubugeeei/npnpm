use crate::Lockfile;
use derive_more::{Display, Error};
use pacquet_diagnostics::miette::{self, Diagnostic};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

/// Error when writing the lockfile to the filesystem.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum SaveLockfileError {
    #[display("Failed to get current_dir: {_0}")]
    #[diagnostic(code(pacquet_lockfile::current_dir))]
    CurrentDir(io::Error),

    #[display("Failed to serialize lockfile as YAML: {_0}")]
    #[diagnostic(code(pacquet_lockfile::serialize_yaml))]
    SerializeYaml(serde_yaml::Error),

    #[display("Failed to write lockfile content to {path:?}: {error}")]
    #[diagnostic(code(pacquet_lockfile::write_file))]
    WriteFile {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

impl Lockfile {
    /// Save lockfile into the given directory.
    pub fn save_to_dir(&self, dir: impl AsRef<Path>) -> Result<PathBuf, SaveLockfileError> {
        let path = dir.as_ref().join(Lockfile::FILE_NAME);
        let contents = serde_yaml::to_string(self).map_err(SaveLockfileError::SerializeYaml)?;
        fs::write(&path, contents)
            .map_err(|error| SaveLockfileError::WriteFile { path: path.clone(), error })?;
        Ok(path)
    }

    /// Save lockfile into the current directory.
    pub fn save_to_current_dir(&self) -> Result<PathBuf, SaveLockfileError> {
        env::current_dir()
            .map_err(SaveLockfileError::CurrentDir)
            .and_then(|dir| self.save_to_dir(dir))
    }
}
