use crate::{
    fetch_package_metadata, Install, ParsedPackageSpec, RegistryMetadataCache,
    RegistryMetadataMode, ResolvedPackages,
};
use derive_more::{Display, Error};
use futures_util::future::join_all;
use miette::Diagnostic;
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_package_manifest::PackageManifestError;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_registry::RegistryError;
use pacquet_tarball::MemCache;

/// This subroutine does everything `pacquet add` is supposed to do.
#[must_use]
pub struct Add<'a, ListDependencyGroups, DependencyGroupList>
where
    ListDependencyGroups: Fn() -> DependencyGroupList,
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub tarball_mem_cache: &'a MemCache,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Npmrc,
    pub manifest: &'a mut PackageManifest,
    pub lockfile: Option<&'a Lockfile>,
    pub list_dependency_groups: ListDependencyGroups, // must be a function because it is called multiple times
    pub packages: &'a [String],
    pub save_exact: bool, // TODO: add `save-exact` to `.npmrc`, merge configs, and remove this
    pub registry_metadata_cache: &'a RegistryMetadataCache,
}

/// Error type of [`Add`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum AddError {
    #[display("Failed to add package to manifest: {_0}")]
    AddDependencyToManifest(#[error(source)] PackageManifestError),
    #[display("Failed save the manifest file: {_0}")]
    SaveManifest(#[error(source)] PackageManifestError),
    #[display("Failed to resolve package metadata: {_0}")]
    FetchFromRegistry(#[error(source)] RegistryError),
}

impl<'a, ListDependencyGroups, DependencyGroupList>
    Add<'a, ListDependencyGroups, DependencyGroupList>
where
    ListDependencyGroups: Fn() -> DependencyGroupList,
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub async fn run(self) -> Result<(), AddError> {
        let Add {
            tarball_mem_cache,
            http_client,
            config,
            manifest,
            lockfile,
            list_dependency_groups,
            packages,
            save_exact,
            resolved_packages,
            registry_metadata_cache,
        } = self;

        let resolved_specs = join_all(packages.iter().map(|package| async move {
            let ParsedPackageSpec { name, specifier } = ParsedPackageSpec::parse(package);
            let version_range = match specifier {
                Some(specifier) => specifier.to_string(),
                None => {
                    let package = fetch_package_metadata(
                        registry_metadata_cache,
                        name,
                        http_client,
                        &config.registry,
                        &config.store_dir,
                        RegistryMetadataMode::Online,
                    )
                    .await
                    .map_err(AddError::FetchFromRegistry)?;
                    package
                        .version_by_specifier("latest")
                        .map_err(AddError::FetchFromRegistry)?
                        .serialize(save_exact)
                }
            };
            Ok::<_, AddError>((name, version_range))
        }))
        .await;

        for (name, version_range) in resolved_specs.into_iter().collect::<Result<Vec<_>, _>>()? {
            for dependency_group in list_dependency_groups() {
                manifest
                    .add_dependency(name, &version_range, dependency_group)
                    .map_err(AddError::AddDependencyToManifest)?;
            }
        }

        Install {
            tarball_mem_cache,
            http_client,
            config,
            manifest,
            lockfile,
            dependency_groups: list_dependency_groups(),
            frozen_lockfile: false,
            resolved_packages,
            registry_metadata_cache,
            offline: false,
            prefer_offline: false,
            lockfile_only: false,
            resolution_only: false,
        }
        .run()
        .await;

        manifest.save().map_err(AddError::SaveManifest)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::ParsedPackageSpec;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_parse_unscoped_package_specs() {
        assert_eq!(
            ParsedPackageSpec::parse("react"),
            ParsedPackageSpec { name: "react", specifier: None },
        );
        assert_eq!(
            ParsedPackageSpec::parse("react@18.3.1"),
            ParsedPackageSpec { name: "react", specifier: Some("18.3.1") },
        );
        assert_eq!(
            ParsedPackageSpec::parse("react@latest"),
            ParsedPackageSpec { name: "react", specifier: Some("latest") },
        );
    }

    #[test]
    fn should_parse_scoped_package_specs() {
        assert_eq!(
            ParsedPackageSpec::parse("@scope/example"),
            ParsedPackageSpec { name: "@scope/example", specifier: None },
        );
        assert_eq!(
            ParsedPackageSpec::parse("@scope/example@1.2.3"),
            ParsedPackageSpec { name: "@scope/example", specifier: Some("1.2.3") },
        );
        assert_eq!(
            ParsedPackageSpec::parse("@scope/example@^1"),
            ParsedPackageSpec { name: "@scope/example", specifier: Some("^1") },
        );
    }
}
