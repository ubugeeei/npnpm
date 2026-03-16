use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_fs::symlink_file;
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

/// Generate `node_modules/.bin` entries for the root project and virtual store packages.
#[must_use]
pub struct CreateBins<'a> {
    pub modules_dir: &'a Path,
    pub virtual_store_dir: &'a Path,
}

/// Error type of [`CreateBins`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CreateBinsError {
    #[display("Failed to read directory at {dir:?}: {error}")]
    ReadDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to read an entry inside {dir:?}: {error}")]
    ReadDirEntry {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to create bin directory at {dir:?}: {error}")]
    CreateBinDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove bin directory at {dir:?}: {error}")]
    RemoveBinDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to load package manifest at {path:?}: {error}")]
    LoadManifest {
        path: PathBuf,
        #[error(source)]
        error: PackageManifestError,
    },

    #[display("Failed to create bin link at {link:?} to {target:?}: {error}")]
    CreateBinLink {
        target: PathBuf,
        link: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

impl<'a> CreateBins<'a> {
    /// Execute the subroutine.
    pub fn run(self) -> Result<(), CreateBinsError> {
        let CreateBins { modules_dir, virtual_store_dir } = self;

        if modules_dir.exists() {
            create_bin_links_for_node_modules(modules_dir)?;
        }

        if virtual_store_dir.exists() {
            for node_modules_dir in collect_virtual_store_node_modules_dirs(virtual_store_dir)? {
                create_bin_links_for_node_modules(&node_modules_dir)?;
            }
        }

        Ok(())
    }
}

fn collect_virtual_store_node_modules_dirs(root: &Path) -> Result<Vec<PathBuf>, CreateBinsError> {
    let dir = root.to_path_buf();
    let mut entries = fs::read_dir(root)
        .map_err(|error| CreateBinsError::ReadDir { dir: dir.clone(), error })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CreateBinsError::ReadDirEntry { dir: dir.clone(), error })?;
    entries.sort_by_key(|entry| entry.file_name());

    Ok(entries
        .into_iter()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            file_type.is_dir().then_some(entry.path().join("node_modules"))
        })
        .filter(|node_modules_dir| node_modules_dir.exists())
        .collect())
}

fn create_bin_links_for_node_modules(node_modules_dir: &Path) -> Result<(), CreateBinsError> {
    let package_dirs = immediate_package_dirs(node_modules_dir)?;
    let mut bin_links = BTreeMap::new();

    for package_dir in package_dirs {
        let manifest_path = package_dir.join("package.json");
        if !manifest_path.exists() {
            continue;
        }

        let manifest = PackageManifest::from_path(manifest_path.clone()).map_err(|error| {
            CreateBinsError::LoadManifest { path: manifest_path.clone(), error }
        })?;

        let mut bin_entries = manifest.bin_entries().map_err(|error| {
            CreateBinsError::LoadManifest { path: manifest_path.clone(), error }
        })?;
        bin_entries.sort();

        if bin_entries.is_empty() {
            continue;
        }

        for (bin_name, relative_target) in bin_entries {
            bin_links.entry(bin_name).or_insert_with(|| package_dir.join(relative_target));
        }
    }

    let bin_dir = node_modules_dir.join(".bin");
    if bin_dir.exists() {
        fs::remove_dir_all(&bin_dir)
            .map_err(|error| CreateBinsError::RemoveBinDir { dir: bin_dir.clone(), error })?;
    }

    if bin_links.is_empty() {
        return Ok(());
    }

    fs::create_dir_all(&bin_dir)
        .map_err(|error| CreateBinsError::CreateBinDir { dir: bin_dir.clone(), error })?;

    for (bin_name, target) in bin_links {
        let link = bin_dir.join(bin_name);
        if let Err(error) = symlink_file(&target, &link) {
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(CreateBinsError::CreateBinLink { target, link, error });
            }
        }
    }

    Ok(())
}

fn immediate_package_dirs(node_modules_dir: &Path) -> Result<Vec<PathBuf>, CreateBinsError> {
    let dir = node_modules_dir.to_path_buf();
    let mut entries = fs::read_dir(node_modules_dir)
        .map_err(|error| CreateBinsError::ReadDir { dir: dir.clone(), error })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CreateBinsError::ReadDirEntry { dir: dir.clone(), error })?;
    entries.sort_by_key(|entry| entry.file_name());

    let mut package_dirs = Vec::new();
    for entry in entries {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        let file_type = entry
            .file_type()
            .map_err(|error| CreateBinsError::ReadDirEntry { dir: dir.clone(), error })?;

        if file_name == ".bin" || file_name.starts_with('.') {
            continue;
        }

        if file_name.starts_with('@') && file_type.is_dir() {
            let mut scoped_entries = fs::read_dir(&path)
                .map_err(|error| CreateBinsError::ReadDir { dir: path.clone(), error })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| CreateBinsError::ReadDirEntry { dir: path.clone(), error })?;
            scoped_entries.sort_by_key(|entry| entry.file_name());

            package_dirs.extend(scoped_entries.into_iter().filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(|file_type| file_type.is_dir() || file_type.is_symlink())?;
                Some(entry.path())
            }));
            continue;
        }

        if file_type.is_dir() || file_type.is_symlink() {
            package_dirs.push(path);
        }
    }

    Ok(package_dirs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacquet_fs::symlink_dir;
    use pacquet_testing_utils::fs::get_filenames_in_folder;
    use std::fs;
    use tempfile::tempdir;

    fn create_package(
        node_modules_dir: &Path,
        package_name: &str,
        bin_field: serde_json::Value,
    ) -> PathBuf {
        let package_dir = if let Some((scope, bare_name)) = package_name.split_once('/') {
            let scoped_dir = node_modules_dir.join(scope);
            fs::create_dir_all(&scoped_dir).unwrap();
            scoped_dir.join(bare_name)
        } else {
            node_modules_dir.join(package_name)
        };

        fs::create_dir_all(package_dir.join("bin")).unwrap();
        fs::write(package_dir.join("bin").join("cli.js"), "#!/usr/bin/env node\n").unwrap();
        fs::write(
            package_dir.join("package.json"),
            serde_json::json!({
                "name": package_name,
                "bin": bin_field,
            })
            .to_string(),
        )
        .unwrap();

        package_dir
    }

    #[test]
    fn should_create_root_bin_links() {
        let dir = tempdir().unwrap();
        let modules_dir = dir.path().join("node_modules");
        let virtual_store_dir = modules_dir.join(".pnpm");
        fs::create_dir_all(&modules_dir).unwrap();

        create_package(&modules_dir, "@scope/example", serde_json::json!("bin/cli.js"));

        CreateBins { modules_dir: &modules_dir, virtual_store_dir: &virtual_store_dir }
            .run()
            .unwrap();

        assert_eq!(get_filenames_in_folder(&modules_dir.join(".bin")), ["example"]);
        assert!(modules_dir.join(".bin/example").exists());
    }

    #[test]
    fn should_create_virtual_store_bin_links() {
        let dir = tempdir().unwrap();
        let modules_dir = dir.path().join("node_modules");
        let virtual_store_dir = modules_dir.join(".pnpm");
        let package_node_modules_dir = virtual_store_dir.join("example@1.0.0").join("node_modules");
        let dependency_store_dir = virtual_store_dir.join("dependency@1.0.0").join("node_modules");

        fs::create_dir_all(&package_node_modules_dir).unwrap();
        fs::create_dir_all(&dependency_store_dir).unwrap();

        create_package(&package_node_modules_dir, "example", serde_json::json!("bin/cli.js"));
        let dependency_dir =
            create_package(&dependency_store_dir, "dependency", serde_json::json!("bin/cli.js"));
        symlink_dir(&dependency_dir, &package_node_modules_dir.join("dependency")).unwrap();

        CreateBins { modules_dir: &modules_dir, virtual_store_dir: &virtual_store_dir }
            .run()
            .unwrap();

        let bin_dir = package_node_modules_dir.join(".bin");
        assert_eq!(get_filenames_in_folder(&bin_dir), ["dependency", "example"]);
        assert!(bin_dir.join("dependency").exists());
        assert!(bin_dir.join("example").exists());
    }

    #[test]
    fn should_only_scan_top_level_virtual_store_entries() {
        let dir = tempdir().unwrap();
        let virtual_store_dir = dir.path().join("node_modules/.pnpm");
        let package_node_modules_dir = virtual_store_dir.join("example@1.0.0").join("node_modules");
        let nested_node_modules_dir =
            package_node_modules_dir.join("example").join("node_modules").join("nested");

        fs::create_dir_all(&package_node_modules_dir).unwrap();
        fs::create_dir_all(&nested_node_modules_dir).unwrap();

        let node_modules_dirs =
            collect_virtual_store_node_modules_dirs(&virtual_store_dir).unwrap();
        assert_eq!(node_modules_dirs, vec![package_node_modules_dir]);
    }
}
