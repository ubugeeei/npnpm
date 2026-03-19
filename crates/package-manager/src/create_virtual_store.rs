use crate::InstallPackageBySnapshot;
use futures_util::future;
use pacquet_lockfile::{DependencyPath, PackageSnapshot, RootProjectSnapshot};
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_tarball::{MemCache, NetworkMode};
use pipe_trait::Pipe;
use std::collections::HashMap;

/// This subroutine generates filesystem layout for the virtual store at `node_modules/.pacquet`.
#[must_use]
pub struct CreateVirtualStore<'a> {
    pub tarball_mem_cache: &'a MemCache,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Npmrc,
    pub packages: Option<&'a HashMap<DependencyPath, PackageSnapshot>>,
    pub project_snapshot: &'a RootProjectSnapshot,
    pub network_mode: NetworkMode,
}

impl<'a> CreateVirtualStore<'a> {
    /// Execute the subroutine.
    pub async fn run(self) {
        let CreateVirtualStore {
            tarball_mem_cache,
            http_client,
            config,
            packages,
            project_snapshot,
            network_mode,
        } = self;

        let packages = packages.unwrap_or_else(|| {
            dbg!(project_snapshot);
            todo!("check project_snapshot, error if it's not empty, do nothing if empty");
        });

        packages
            .iter()
            .map(|(dependency_path, package_snapshot)| async move {
                InstallPackageBySnapshot {
                    tarball_mem_cache,
                    http_client,
                    config,
                    dependency_path,
                    package_snapshot,
                    network_mode,
                }
                .run()
                .await
                .unwrap(); // TODO: properly propagate this error
            })
            .pipe(future::join_all)
            .await;
    }
}
