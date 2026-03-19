use crate::{Install, RegistryMetadataCache, ResolvedPackages};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_tarball::MemCache;

#[must_use]
pub struct Remove<'a, ListDependencyGroups, DependencyGroupList>
where
    ListDependencyGroups: Fn() -> DependencyGroupList,
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub tarball_mem_cache: &'a MemCache,
    pub resolved_packages: &'a ResolvedPackages,
    pub registry_metadata_cache: &'a RegistryMetadataCache,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Npmrc,
    pub manifest: &'a mut PackageManifest,
    pub list_dependency_groups: ListDependencyGroups,
    pub packages: &'a [String],
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum RemoveError {
    #[display("Failed to remove package from manifest: {_0}")]
    RemoveDependencyFromManifest(#[error(source)] PackageManifestError),
    #[display("Failed save the manifest file: {_0}")]
    SaveManifest(#[error(source)] PackageManifestError),
}

impl<'a, ListDependencyGroups, DependencyGroupList>
    Remove<'a, ListDependencyGroups, DependencyGroupList>
where
    ListDependencyGroups: Fn() -> DependencyGroupList,
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub async fn run(self) -> Result<(), RemoveError> {
        let Remove {
            tarball_mem_cache,
            resolved_packages,
            registry_metadata_cache,
            http_client,
            config,
            manifest,
            list_dependency_groups,
            packages,
        } = self;

        for package in packages {
            for dependency_group in list_dependency_groups() {
                manifest
                    .remove_dependency(package, dependency_group)
                    .map_err(RemoveError::RemoveDependencyFromManifest)?;
            }
        }

        Install {
            tarball_mem_cache,
            resolved_packages,
            registry_metadata_cache,
            http_client,
            config,
            manifest,
            lockfile: None,
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ],
            frozen_lockfile: false,
            offline: false,
            prefer_offline: false,
            lockfile_only: false,
            resolution_only: false,
        }
        .run()
        .await;

        manifest.save().map_err(RemoveError::SaveManifest)?;
        Ok(())
    }
}
