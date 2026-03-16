use crate::{
    CreateBins, GenerateLockfile, InstallFrozenLockfile, InstallWithoutLockfile,
    RegistryMetadataCache, ResolvedPackages,
};
use pacquet_lockfile::{Lockfile, RootProjectSnapshot};
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_tarball::MemCache;
use std::{collections::HashMap, path::Path};

/// This subroutine does everything `pacquet install` is supposed to do.
#[must_use]
pub struct Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub tarball_mem_cache: &'a MemCache,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Npmrc,
    pub manifest: &'a PackageManifest,
    pub lockfile: Option<&'a Lockfile>,
    pub dependency_groups: DependencyGroupList,
    pub frozen_lockfile: bool,
    pub registry_metadata_cache: &'a RegistryMetadataCache,
}

impl<'a, DependencyGroupList> Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    fn lockfile_matches_manifest(
        manifest: &PackageManifest,
        lockfile: &Lockfile,
        dependency_groups: &[DependencyGroup],
    ) -> bool {
        let RootProjectSnapshot::Single(project_snapshot) = &lockfile.project_snapshot else {
            return false;
        };

        dependency_groups.iter().all(|group| {
            let manifest_dependencies: HashMap<_, _> = manifest.dependencies([*group]).collect();
            let snapshot_dependencies = project_snapshot
                .get_map_by_group(*group)
                .map(|dependencies| {
                    dependencies
                        .iter()
                        .map(|(name, spec)| (name.to_string(), spec.specifier.as_str()))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();

            manifest_dependencies.len() == snapshot_dependencies.len()
                && manifest_dependencies.iter().all(|(name, specifier)| {
                    snapshot_dependencies.get(*name).is_some_and(|value| value == specifier)
                })
        })
    }

    async fn install_from_lockfile(
        tarball_mem_cache: &MemCache,
        http_client: &ThrottledClient,
        config: &'static Npmrc,
        lockfile: &Lockfile,
        dependency_groups: Vec<DependencyGroup>,
    ) {
        let Lockfile { lockfile_version, project_snapshot, packages, .. } = lockfile;
        assert_eq!(lockfile_version.major, 6); // compatibility check already happens at serde, but this still helps preventing programmer mistakes.

        InstallFrozenLockfile {
            tarball_mem_cache,
            http_client,
            config,
            project_snapshot,
            packages: packages.as_ref(),
            dependency_groups,
        }
        .run()
        .await;
    }

    /// Execute the subroutine.
    pub async fn run(self) {
        let Install {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            config,
            manifest,
            lockfile,
            dependency_groups,
            frozen_lockfile,
            registry_metadata_cache,
        } = self;
        let dependency_groups = dependency_groups.into_iter().collect::<Vec<_>>();

        tracing::info!(target: "pacquet::install", "Start all");

        match (config.lockfile, frozen_lockfile, lockfile) {
            (false, _, _) => {
                InstallWithoutLockfile {
                    tarball_mem_cache,
                    resolved_packages,
                    http_client,
                    config,
                    manifest,
                    dependency_groups,
                    registry_metadata_cache,
                }
                .run()
                .await;
            }
            (true, true, None) => {
                panic!("--frozen-lockfile requires an existing pnpm-lock.yaml");
            }
            (true, true, Some(lockfile)) => {
                Self::install_from_lockfile(
                    tarball_mem_cache,
                    http_client,
                    config,
                    lockfile,
                    dependency_groups,
                )
                .await;
            }
            (true, false, Some(lockfile))
                if config.prefer_frozen_lockfile
                    && Self::lockfile_matches_manifest(manifest, lockfile, &dependency_groups) =>
            {
                Self::install_from_lockfile(
                    tarball_mem_cache,
                    http_client,
                    config,
                    lockfile,
                    dependency_groups,
                )
                .await;
            }
            (true, false, _) => {
                let generated_lockfile = GenerateLockfile {
                    http_client,
                    config,
                    manifest,
                    dependency_groups: &dependency_groups,
                    registry_metadata_cache,
                }
                .run()
                .await
                .expect("generate pnpm lockfile");

                let lockfile_dir = manifest.path().parent().unwrap_or(Path::new("."));
                generated_lockfile
                    .save_to_dir(lockfile_dir)
                    .expect("save pnpm lockfile to workspace");

                Self::install_from_lockfile(
                    tarball_mem_cache,
                    http_client,
                    config,
                    &generated_lockfile,
                    dependency_groups,
                )
                .await;
            }
        }

        CreateBins {
            modules_dir: &config.modules_dir,
            virtual_store_dir: &config.virtual_store_dir,
        }
        .run()
        .expect("create node_modules/.bin layout");

        tracing::info!(target: "pacquet::install", "Complete all");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacquet_lockfile::Lockfile;
    use pacquet_npmrc::Npmrc;
    use pacquet_package_manifest::{DependencyGroup, PackageManifest};
    use pacquet_registry_mock::AutoMockInstance;
    use pacquet_testing_utils::fs::{get_all_folders, is_symlink_or_junction};
    use std::{env, fs};
    use tempfile::tempdir;

    #[tokio::test]
    async fn should_install_dependencies() {
        let mock_instance = tokio::task::spawn_blocking(AutoMockInstance::load_or_init)
            .await
            .expect("load mocked registry");

        let dir = tempdir().unwrap();
        let store_dir = dir.path().join("pacquet-store");
        let project_root = dir.path().join("project");
        let modules_dir = project_root.join("node_modules"); // TODO: we shouldn't have to define this
        let virtual_store_dir = modules_dir.join(".pacquet"); // TODO: we shouldn't have to define this

        let manifest_path = dir.path().join("package.json");
        let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();

        manifest
            .add_dependency("@pnpm.e2e/hello-world-js-bin", "1.0.0", DependencyGroup::Prod)
            .unwrap();
        manifest.add_dependency("@pnpm/xyz", "1.0.0", DependencyGroup::Dev).unwrap();

        manifest.save().unwrap();

        let mut config = Npmrc::new();
        config.store_dir = store_dir.into();
        config.modules_dir = modules_dir.to_path_buf();
        config.virtual_store_dir = virtual_store_dir.to_path_buf();
        config.registry = mock_instance.url();
        let config = config.leak();
        let registry_metadata_cache = RegistryMetadataCache::new();

        Install {
            tarball_mem_cache: &Default::default(),
            http_client: &Default::default(),
            config,
            manifest: &manifest,
            lockfile: None,
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ],
            frozen_lockfile: false,
            resolved_packages: &Default::default(),
            registry_metadata_cache: &registry_metadata_cache,
        }
        .run()
        .await;

        // Make sure the package is installed
        let path = project_root.join("node_modules/@pnpm.e2e/hello-world-js-bin");
        assert!(is_symlink_or_junction(&path).unwrap());
        let path = project_root.join("node_modules/.pacquet/@pnpm.e2e+hello-world-js-bin@1.0.0");
        assert!(path.exists());
        // Make sure we install dev-dependencies as well
        let path = project_root.join("node_modules/@pnpm/xyz");
        assert!(is_symlink_or_junction(&path).unwrap());
        let path = project_root.join("node_modules/.pacquet/@pnpm+xyz@1.0.0");
        assert!(path.is_dir());

        insta::assert_debug_snapshot!(get_all_folders(&project_root));

        drop((dir, mock_instance)); // cleanup
    }

    #[tokio::test]
    async fn should_generate_and_reuse_lockfile() {
        let dir = tempdir().unwrap();
        let project_root = dir.path().join("project");
        fs::create_dir_all(&project_root).unwrap();

        let store_dir = dir.path().join("pacquet-store");
        let modules_dir = project_root.join("node_modules");
        let virtual_store_dir = modules_dir.join(".pacquet");
        let manifest_path = project_root.join("package.json");
        let mut manifest = PackageManifest::create_if_needed(manifest_path.clone()).unwrap();

        manifest.add_dependency("fast-querystring", "1.0.0", DependencyGroup::Prod).unwrap();
        manifest.save().unwrap();

        let mut config = Npmrc::new();
        config.store_dir = store_dir.into();
        config.modules_dir = modules_dir.clone();
        config.virtual_store_dir = virtual_store_dir;
        config.lockfile = true;
        config.registry = "https://registry.npmjs.org/".to_string();
        let config = config.leak();

        let tarball_mem_cache = MemCache::new();
        let resolved_packages = ResolvedPackages::new();
        let registry_metadata_cache = RegistryMetadataCache::new();
        let http_client = ThrottledClient::new_from_cpu_count();

        Install {
            tarball_mem_cache: &tarball_mem_cache,
            http_client: &http_client,
            config,
            manifest: &manifest,
            lockfile: None,
            dependency_groups: [DependencyGroup::Prod],
            frozen_lockfile: false,
            resolved_packages: &resolved_packages,
            registry_metadata_cache: &registry_metadata_cache,
        }
        .run()
        .await;

        let lockfile_path = project_root.join("pnpm-lock.yaml");
        assert!(lockfile_path.exists());

        let lockfile = Lockfile::load_from_dir(&project_root).unwrap().unwrap();
        assert!(modules_dir.join("fast-querystring").exists());

        fs::remove_dir_all(&modules_dir).unwrap();

        Install {
            tarball_mem_cache: &tarball_mem_cache,
            http_client: &http_client,
            config,
            manifest: &manifest,
            lockfile: Some(&lockfile),
            dependency_groups: [DependencyGroup::Prod],
            frozen_lockfile: true,
            resolved_packages: &resolved_packages,
            registry_metadata_cache: &registry_metadata_cache,
        }
        .run()
        .await;

        assert!(modules_dir.join("fast-querystring").exists());
        drop(dir);
    }
}
