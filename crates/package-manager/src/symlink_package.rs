use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_fs::symlink_dir;
use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

/// Error type for [`symlink_package`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum SymlinkPackageError {
    #[display("Failed to create directory at {dir:?}: {error}")]
    CreateParentDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to create symlink at {symlink_path:?} to {symlink_target:?}: {error}")]
    SymlinkDir {
        symlink_target: PathBuf,
        symlink_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove stale path at {path:?}: {error}")]
    RemoveStalePath {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// Create symlink for a package.
///
/// * If ancestors of `symlink_path` don't exist, they will be created recursively.
/// * If `symlink_path` already exists, skip.
/// * If `symlink_path` doesn't exist, a symlink pointing to `symlink_target` will be created.
pub fn symlink_package(
    symlink_target: &Path,
    symlink_path: &Path,
) -> Result<(), SymlinkPackageError> {
    // NOTE: symlink target in pacquet is absolute yet in pnpm is relative
    // TODO: change symlink target to relative
    if let Some(parent) = symlink_path.parent() {
        fs::create_dir_all(parent).map_err(|error| SymlinkPackageError::CreateParentDir {
            dir: parent.to_path_buf(),
            error,
        })?;
    }
    if symlink_path.exists() {
        let same_target =
            fs::canonicalize(symlink_path).ok() == fs::canonicalize(symlink_target).ok();
        if same_target {
            return Ok(());
        }

        let metadata = fs::symlink_metadata(symlink_path).map_err(|error| {
            SymlinkPackageError::RemoveStalePath { path: symlink_path.to_path_buf(), error }
        })?;
        let remove = if metadata.file_type().is_symlink() || metadata.is_file() {
            fs::remove_file(symlink_path)
        } else {
            fs::remove_dir_all(symlink_path)
        };
        remove.map_err(|error| SymlinkPackageError::RemoveStalePath {
            path: symlink_path.to_path_buf(),
            error,
        })?;
    }

    if let Err(error) = symlink_dir(symlink_target, symlink_path) {
        if error.kind() != ErrorKind::AlreadyExists {
            return Err(SymlinkPackageError::SymlinkDir {
                symlink_target: symlink_target.to_path_buf(),
                symlink_path: symlink_path.to_path_buf(),
                error,
            });
        }
    }
    Ok(())
}
