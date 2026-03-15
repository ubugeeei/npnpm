use crate::{link_file, LinkFileError};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_npmrc::PackageImportMethod;
use rayon::prelude::*;
use std::{
    collections::{BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
};

/// Error type for [`create_cas_files`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CreateCasFilesError {
    #[display("cannot create directory at {dirname:?}: {error}")]
    CreateDir {
        dirname: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[diagnostic(transparent)]
    LinkFile(#[error(source)] LinkFileError),
}

/// If `dir_path` doesn't exist, create and populate it with files from `cas_paths`.
///
/// If `dir_path` already exists, do nothing.
pub fn create_cas_files(
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
) -> Result<(), CreateCasFilesError> {
    if dir_path.exists() {
        return Ok(());
    }

    fs::create_dir_all(dir_path).map_err(|error| CreateCasFilesError::CreateDir {
        dirname: dir_path.to_path_buf(),
        error,
    })?;

    let mut parent_dirs = BTreeSet::new();
    for cleaned_entry in cas_paths.keys() {
        let Some(parent) = Path::new(cleaned_entry).parent() else {
            continue;
        };
        if parent.as_os_str().is_empty() {
            continue;
        }
        parent_dirs.insert(dir_path.join(parent));
    }

    for parent_dir in parent_dirs {
        fs::create_dir_all(&parent_dir)
            .map_err(|error| CreateCasFilesError::CreateDir { dirname: parent_dir, error })?;
    }

    cas_paths
        .par_iter()
        .try_for_each(|(cleaned_entry, store_path)| {
            link_file(import_method, store_path, &dir_path.join(cleaned_entry))
        })
        .map_err(CreateCasFilesError::LinkFile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn should_hardlink_nested_files_when_requested() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempdir().unwrap();
        let source_file = dir.path().join("source.js");
        fs::write(&source_file, "console.log('hi')\n").unwrap();

        let mut cas_paths = HashMap::new();
        cas_paths.insert("dist/index.js".to_string(), source_file.clone());

        let target_dir = dir.path().join("node_modules/pkg");
        create_cas_files(PackageImportMethod::Hardlink, &target_dir, &cas_paths).unwrap();

        let linked_file = target_dir.join("dist/index.js");
        assert!(linked_file.is_file());
        assert_eq!(
            fs::metadata(&source_file).unwrap().ino(),
            fs::metadata(&linked_file).unwrap().ino()
        );
    }
}
