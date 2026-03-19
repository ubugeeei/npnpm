use crate::{CreateVirtualDirBySnapshot, CreateVirtualDirError};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_lockfile::{DependencyPath, LockfileResolution, PackageSnapshot, PkgNameVerPeer};
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_tarball::{DownloadTarballToStore, MemCache, NetworkMode, TarballError};
use pipe_trait::Pipe;
use std::borrow::Cow;

/// This subroutine downloads a package tarball, extracts it, installs it to a virtual dir,
/// then creates the symlink layout for the package.
#[must_use]
pub struct InstallPackageBySnapshot<'a> {
    pub tarball_mem_cache: &'a MemCache,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Npmrc,
    pub dependency_path: &'a DependencyPath,
    pub package_snapshot: &'a PackageSnapshot,
    pub network_mode: NetworkMode,
}

/// Error type of [`InstallPackageBySnapshot`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallPackageBySnapshotError {
    DownloadTarball(TarballError),
    CreateVirtualDir(CreateVirtualDirError),
}

impl<'a> InstallPackageBySnapshot<'a> {
    /// Execute the subroutine.
    pub async fn run(self) -> Result<(), InstallPackageBySnapshotError> {
        let InstallPackageBySnapshot {
            tarball_mem_cache,
            http_client,
            config,
            dependency_path,
            package_snapshot,
            network_mode,
        } = self;
        let PackageSnapshot { resolution, .. } = package_snapshot;
        let DependencyPath { custom_registry, package_specifier } = dependency_path;

        let (tarball_url, integrity) = match resolution {
            LockfileResolution::Tarball(tarball_resolution) => {
                let integrity = tarball_resolution.integrity.as_ref().unwrap_or_else(|| {
                    // TODO: how to handle the absent of integrity field?
                    panic!("Current implementation requires integrity, but {dependency_path} doesn't have it");
                });
                (tarball_resolution.tarball.as_str().pipe(Cow::Borrowed), integrity)
            }
            LockfileResolution::Registry(registry_resolution) => {
                let registry = custom_registry.as_ref().unwrap_or(&config.registry);
                let registry = registry.strip_suffix('/').unwrap_or(registry);
                let PkgNameVerPeer { name, suffix: ver_peer } = package_specifier;
                let version = ver_peer.version();
                let bare_name = name.bare.as_str();
                let tarball_url = format!("{registry}/{name}/-/{bare_name}-{version}.tgz");
                let integrity = &registry_resolution.integrity;
                (Cow::Owned(tarball_url), integrity)
            }
            LockfileResolution::Directory(_) | LockfileResolution::Git(_) => {
                panic!("Only TarballResolution and RegistryResolution is supported at the moment, but {dependency_path} requires {resolution:?}");
            }
        };

        // TODO: skip when already exists in store?
        let cas_paths = DownloadTarballToStore {
            http_client,
            store_dir: &config.store_dir,
            package_integrity: integrity,
            package_unpacked_size: None,
            package_url: &tarball_url,
            network_mode,
        }
        .run_with_mem_cache(tarball_mem_cache)
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)?;

        CreateVirtualDirBySnapshot {
            virtual_store_dir: &config.virtual_store_dir,
            cas_paths: cas_paths.as_ref(),
            import_method: config.package_import_method,
            dependency_path,
            package_snapshot,
        }
        .run()
        .map_err(InstallPackageBySnapshotError::CreateVirtualDir)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use pacquet_lockfile::{
        LockfileResolution, PackageSnapshot, PkgName, PkgNameVerPeer, PkgVerPeer, TarballResolution,
    };
    use pacquet_npmrc::{Npmrc, PackageImportMethod};
    use pacquet_store_dir::StoreDir;
    use ssri::Integrity;
    use std::{fs, path::PathBuf, str::FromStr};
    use tempfile::tempdir;

    fn fixture_tarball() -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tasks/micro-benchmark/fixtures/@fastify+error-3.3.0.tgz");
        fs::read(path).expect("read tarball fixture")
    }

    fn create_config(
        store_dir: &std::path::Path,
        modules_dir: &std::path::Path,
        virtual_store_dir: &std::path::Path,
    ) -> Npmrc {
        Npmrc {
            hoist: false,
            hoist_pattern: vec![],
            public_hoist_pattern: vec![],
            shamefully_hoist: false,
            store_dir: StoreDir::new(store_dir),
            modules_dir: modules_dir.to_path_buf(),
            node_linker: Default::default(),
            symlink: true,
            virtual_store_dir: virtual_store_dir.to_path_buf(),
            package_import_method: PackageImportMethod::Auto,
            modules_cache_max_age: 0,
            lockfile: true,
            prefer_frozen_lockfile: true,
            lockfile_include_tarball_url: false,
            registry: "http://127.0.0.1:9/".to_string(),
            auto_install_peers: false,
            dedupe_peer_dependents: false,
            strict_peer_dependencies: false,
            resolve_peers_from_workspace_root: false,
        }
    }

    #[tokio::test]
    async fn should_reuse_store_for_snapshot_install_without_network() {
        let dir = tempdir().unwrap();
        let store_dir = dir.path().join("store");
        let modules_dir = dir.path().join("node_modules");
        let virtual_store_dir = modules_dir.join(".pnpm");

        let tarball = fixture_tarball();
        let integrity: Integrity =
            "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="
                .parse()
                .expect("parse tarball integrity");
        let prepopulate_store_dir = Box::leak(Box::new(StoreDir::new(&store_dir)));

        let mut server = Server::new();
        server
            .mock("GET", "/@fastify+error-3.3.0.tgz")
            .with_status(200)
            .with_body(tarball)
            .create();

        let http_client = ThrottledClient::new_from_cpu_count();
        DownloadTarballToStore {
            http_client: &http_client,
            store_dir: prepopulate_store_dir,
            package_integrity: &integrity,
            package_unpacked_size: Some(16697),
            package_url: &format!("{}/@fastify+error-3.3.0.tgz", server.url()),
            network_mode: NetworkMode::Online,
        }
        .run_without_mem_cache()
        .await
        .unwrap();

        let config =
            create_config(&store_dir, &modules_dir, &virtual_store_dir).leak() as &'static Npmrc;
        let dependency_path = DependencyPath {
            custom_registry: None,
            package_specifier: PkgNameVerPeer::new(
                PkgName::from_str("@fastify/error").unwrap(),
                PkgVerPeer::from_str("3.3.0").unwrap(),
            ),
        };
        let package_snapshot = PackageSnapshot {
            resolution: LockfileResolution::Tarball(TarballResolution {
                tarball: "http://127.0.0.1:9/not-used.tgz".to_string(),
                integrity: Some(integrity),
            }),
            id: None,
            name: None,
            version: None,
            engines: None,
            cpu: None,
            os: None,
            libc: None,
            deprecated: None,
            has_bin: None,
            prepare: None,
            requires_build: None,
            bundled_dependencies: None,
            peer_dependencies: None,
            peer_dependencies_meta: None,
            dependencies: None,
            optional_dependencies: None,
            transitive_peer_dependencies: None,
            dev: Some(false),
            optional: Some(false),
        };
        let tarball_mem_cache = Default::default();

        InstallPackageBySnapshot {
            tarball_mem_cache: &tarball_mem_cache,
            http_client: &http_client,
            config,
            dependency_path: &dependency_path,
            package_snapshot: &package_snapshot,
            network_mode: NetworkMode::Online,
        }
        .run()
        .await
        .unwrap();

        let installed_path = virtual_store_dir
            .join("@fastify+error@3.3.0")
            .join("node_modules")
            .join("@fastify")
            .join("error");
        assert!(installed_path.is_dir());
    }
}
