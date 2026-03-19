use crate::{CreateVirtualStore, SymlinkDirectDependencies};
use pacquet_lockfile::{DependencyPath, PackageSnapshot, RootProjectSnapshot};
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_package_manifest::DependencyGroup;
use pacquet_tarball::{MemCache, NetworkMode};
use std::collections::HashMap;

/// This subroutine installs dependencies from a frozen lockfile.
///
/// **Brief overview:**
/// * Iterate over each package in [`Self::packages`].
/// * Fetch a tarball of each package.
/// * Extract each tarball into the store directory.
/// * Import (by reflink, hardlink, or copy) the files from the store dir to each `node_modules/.pacquet/{name}@{version}/node_modules/{name}/`.
/// * Create dependency symbolic links in each `node_modules/.pacquet/{name}@{version}/node_modules/`.
/// * Create a symbolic link at each `node_modules/{name}`.
#[must_use]
pub struct InstallFrozenLockfile<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub tarball_mem_cache: &'a MemCache,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Npmrc,
    pub project_snapshot: &'a RootProjectSnapshot,
    pub packages: Option<&'a HashMap<DependencyPath, PackageSnapshot>>,
    pub dependency_groups: DependencyGroupList,
    pub network_mode: NetworkMode,
}

impl<'a, DependencyGroupList> InstallFrozenLockfile<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Execute the subroutine.
    pub async fn run(self) {
        let InstallFrozenLockfile {
            tarball_mem_cache,
            http_client,
            config,
            project_snapshot,
            packages,
            dependency_groups,
            network_mode,
        } = self;

        CreateVirtualStore {
            tarball_mem_cache,
            http_client,
            config,
            packages,
            project_snapshot,
            network_mode,
        }
        .run()
        .await;

        SymlinkDirectDependencies { config, project_snapshot, dependency_groups }.run();
    }
}
