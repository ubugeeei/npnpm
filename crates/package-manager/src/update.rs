use crate::{
    fetch_package_metadata, Install, ParsedPackageSpec, RegistryMetadataCache, ResolvedPackages,
};
use derive_more::{Display, Error};
use futures_util::future::join_all;
use miette::Diagnostic;
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_registry::RegistryError;
use pacquet_tarball::MemCache;

#[must_use]
pub struct Update<'a> {
    pub tarball_mem_cache: &'a MemCache,
    pub resolved_packages: &'a ResolvedPackages,
    pub registry_metadata_cache: &'a RegistryMetadataCache,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Npmrc,
    pub manifest: &'a mut PackageManifest,
    pub dependency_groups: Vec<DependencyGroup>,
    pub packages: &'a [String],
    pub latest: bool,
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum UpdateError {
    #[display("Failed to resolve package metadata: {_0}")]
    FetchFromRegistry(#[error(source)] RegistryError),
    #[display("Package is not present in package.json: {_0}")]
    MissingPackage(#[error(not(source))] String),
    #[display("Failed to update package in manifest: {_0}")]
    UpdateDependencyInManifest(#[error(source)] PackageManifestError),
    #[display("Failed save the manifest file: {_0}")]
    SaveManifest(#[error(source)] PackageManifestError),
}

impl<'a> Update<'a> {
    pub async fn run(self) -> Result<(), UpdateError> {
        let Update {
            tarball_mem_cache,
            resolved_packages,
            registry_metadata_cache,
            http_client,
            config,
            manifest,
            dependency_groups,
            packages,
            latest,
        } = self;

        if latest {
            let targets = if packages.is_empty() {
                manifest.dependency_entries(dependency_groups.clone())
            } else {
                collect_target_entries(manifest, &dependency_groups, packages)?
            };

            let updates = join_all(targets.iter().map(|(_, name, current_spec)| async move {
                let package = fetch_package_metadata(
                    registry_metadata_cache,
                    name,
                    http_client,
                    &config.registry,
                )
                .await
                .map_err(UpdateError::FetchFromRegistry)?;
                let latest_version = package
                    .version_by_specifier("latest")
                    .map_err(UpdateError::FetchFromRegistry)?;
                Ok::<_, UpdateError>((
                    name.clone(),
                    current_spec.clone(),
                    latest_version.serialize(is_exact_version(current_spec)),
                ))
            }))
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

            for (name, previous_spec, next_spec) in updates {
                for dependency_group in &dependency_groups {
                    if manifest.dependency_version(&name, *dependency_group)
                        == Some(previous_spec.as_str())
                    {
                        manifest
                            .add_dependency(&name, &next_spec, *dependency_group)
                            .map_err(UpdateError::UpdateDependencyInManifest)?;
                    }
                }
            }
        } else if !packages.is_empty() {
            ensure_target_packages_exist(manifest, &dependency_groups, packages)?;
            for package in packages {
                let ParsedPackageSpec { name, specifier } = ParsedPackageSpec::parse(package);
                let Some(specifier) = specifier else {
                    continue;
                };

                let mut updated = false;
                for dependency_group in &dependency_groups {
                    if manifest.dependency_version(name, *dependency_group).is_none() {
                        continue;
                    }

                    manifest
                        .add_dependency(name, specifier, *dependency_group)
                        .map_err(UpdateError::UpdateDependencyInManifest)?;
                    updated = true;
                }

                if !updated {
                    return Err(UpdateError::MissingPackage(name.to_string()));
                }
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
            dependency_groups,
            frozen_lockfile: false,
            offline: false,
        }
        .run()
        .await;

        manifest.save().map_err(UpdateError::SaveManifest)?;
        Ok(())
    }
}

fn collect_target_entries(
    manifest: &PackageManifest,
    dependency_groups: &[DependencyGroup],
    packages: &[String],
) -> Result<Vec<(DependencyGroup, String, String)>, UpdateError> {
    let mut targets = Vec::new();
    for package in packages {
        let ParsedPackageSpec { name, .. } = ParsedPackageSpec::parse(package);
        let mut found = false;
        for dependency_group in dependency_groups {
            if let Some(specifier) = manifest.dependency_version(name, *dependency_group) {
                targets.push((*dependency_group, name.to_string(), specifier.to_string()));
                found = true;
            }
        }

        if !found {
            return Err(UpdateError::MissingPackage(name.to_string()));
        }
    }
    Ok(targets)
}

fn ensure_target_packages_exist(
    manifest: &PackageManifest,
    dependency_groups: &[DependencyGroup],
    packages: &[String],
) -> Result<(), UpdateError> {
    for package in packages {
        let ParsedPackageSpec { name, .. } = ParsedPackageSpec::parse(package);
        if dependency_groups
            .iter()
            .all(|dependency_group| manifest.dependency_version(name, *dependency_group).is_none())
        {
            return Err(UpdateError::MissingPackage(name.to_string()));
        }
    }

    Ok(())
}

fn is_exact_version(specifier: &str) -> bool {
    specifier.parse::<node_semver::Version>().is_ok()
}
