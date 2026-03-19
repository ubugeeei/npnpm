use crate::{fetch_package_metadata, ParsedPackageSpec, RegistryMetadataCache, ResolvedPackages};
use async_recursion::async_recursion;
use derive_more::{Display, Error};
use futures_util::future::join_all;
use miette::Diagnostic;
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_registry::{PackageVersion, RegistryError};
use pacquet_tarball::{DownloadTarballToStore, MemCache, TarballError};

/// Populate the store with packages and their transitive dependencies without creating node_modules.
#[must_use]
pub struct StoreAdd<'a> {
    pub tarball_mem_cache: &'a MemCache,
    pub registry_metadata_cache: &'a RegistryMetadataCache,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Npmrc,
    pub packages: &'a [String],
}

/// Error type of [`StoreAdd`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum StoreAddError {
    FetchFromRegistry(#[error(source)] RegistryError),
    DownloadTarball(#[error(source)] TarballError),
}

impl<'a> StoreAdd<'a> {
    pub async fn run(self) -> Result<(), StoreAddError> {
        let StoreAdd {
            tarball_mem_cache,
            registry_metadata_cache,
            resolved_packages,
            http_client,
            config,
            packages,
        } = self;

        let runner = StoreAdder {
            tarball_mem_cache,
            registry_metadata_cache,
            resolved_packages,
            http_client,
            config,
        };

        let root_packages = join_all(packages.iter().map(|package| async {
            let ParsedPackageSpec { name, specifier } = ParsedPackageSpec::parse(package);
            let package = fetch_package_metadata(
                registry_metadata_cache,
                name,
                http_client,
                &config.registry,
            )
            .await
            .map_err(StoreAddError::FetchFromRegistry)?;
            package
                .version_by_specifier(specifier.unwrap_or("latest"))
                .cloned()
                .map_err(StoreAddError::FetchFromRegistry)
        }))
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

        for package in root_packages {
            runner.store_package_and_dependencies(package).await?;
        }

        Ok(())
    }
}

struct StoreAdder<'a> {
    tarball_mem_cache: &'a MemCache,
    registry_metadata_cache: &'a RegistryMetadataCache,
    resolved_packages: &'a ResolvedPackages,
    http_client: &'a ThrottledClient,
    config: &'static Npmrc,
}

impl<'a> StoreAdder<'a> {
    #[async_recursion]
    async fn store_package_and_dependencies(
        &self,
        package: PackageVersion,
    ) -> Result<(), StoreAddError> {
        if !self.resolved_packages.insert(package.to_virtual_store_name()) {
            tracing::info!(
                target: "pacquet::store",
                package = ?package.to_virtual_store_name(),
                "Skip store subset"
            );
            return Ok(());
        }

        DownloadTarballToStore {
            http_client: self.http_client,
            store_dir: &self.config.store_dir,
            package_integrity: package
                .dist
                .integrity
                .as_ref()
                .expect("registry metadata should include integrity"),
            package_unpacked_size: package.dist.unpacked_size,
            package_url: package.as_tarball_url(),
        }
        .run_with_mem_cache(self.tarball_mem_cache)
        .await
        .map_err(StoreAddError::DownloadTarball)?;

        let dependencies = join_all(package.dependencies(self.config.auto_install_peers).map(
            |(name, version_range)| async move {
                let dependency = fetch_package_metadata(
                    self.registry_metadata_cache,
                    name,
                    self.http_client,
                    &self.config.registry,
                )
                .await
                .map_err(StoreAddError::FetchFromRegistry)?;
                dependency
                    .version_by_specifier(version_range)
                    .cloned()
                    .map_err(StoreAddError::FetchFromRegistry)
            },
        ))
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

        for dependency in dependencies {
            self.store_package_and_dependencies(dependency).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use pacquet_npmrc::PackageImportMethod;
    use pacquet_store_dir::StoreDir;
    use ssri::Integrity;
    use std::{fs, path::PathBuf};
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
        registry: String,
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
            lockfile: false,
            prefer_frozen_lockfile: false,
            lockfile_include_tarball_url: false,
            registry,
            auto_install_peers: false,
            dedupe_peer_dependents: false,
            strict_peer_dependencies: false,
            resolve_peers_from_workspace_root: false,
        }
    }

    #[tokio::test]
    async fn should_prefetch_root_and_transitive_dependencies_without_duplicate_downloads() {
        let dir = tempdir().unwrap();
        let store_dir = dir.path().join("store");
        let modules_dir = dir.path().join("node_modules");
        let virtual_store_dir = modules_dir.join(".pnpm");
        let tarball = fixture_tarball();
        let integrity: Integrity =
            "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="
                .parse()
                .expect("parse tarball integrity");

        let mut server = Server::new_async().await;
        let registry = format!("{}/", server.url());

        let root_package = serde_json::json!({
            "name": "root",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "root",
                    "version": "1.0.0",
                    "dist": {
                        "tarball": format!("{}/root/-/root-1.0.0.tgz", server.url()),
                        "integrity": integrity.to_string(),
                        "unpackedSize": 16697
                    },
                    "dependencies": {
                        "dep": "1.0.0"
                    }
                }
            }
        });
        let dep_package = serde_json::json!({
            "name": "dep",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "dep",
                    "version": "1.0.0",
                    "dist": {
                        "tarball": format!("{}/dep/-/dep-1.0.0.tgz", server.url()),
                        "integrity": integrity.to_string(),
                        "unpackedSize": 16697
                    }
                }
            }
        });

        let _root_metadata = server
            .mock("GET", "/root")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(root_package.to_string())
            .expect(1)
            .create_async()
            .await;
        let _dep_metadata = server
            .mock("GET", "/dep")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(dep_package.to_string())
            .expect(1)
            .create_async()
            .await;
        let _root_tarball = server
            .mock("GET", "/root/-/root-1.0.0.tgz")
            .with_status(200)
            .with_body(tarball)
            .expect(1)
            .create_async()
            .await;

        let config = create_config(&store_dir, &modules_dir, &virtual_store_dir, registry).leak();
        let tarball_mem_cache = MemCache::new();
        let registry_metadata_cache = RegistryMetadataCache::new();
        let resolved_packages = ResolvedPackages::new();
        let http_client = ThrottledClient::new_from_cpu_count();

        StoreAdd {
            tarball_mem_cache: &tarball_mem_cache,
            registry_metadata_cache: &registry_metadata_cache,
            resolved_packages: &resolved_packages,
            http_client: &http_client,
            config,
            packages: &["root".to_string()],
        }
        .run()
        .await
        .unwrap();

        assert!(config.store_dir.status().unwrap().checked_files > 0);
        assert!(config.store_dir.tmp().parent().unwrap().exists());
    }
}
