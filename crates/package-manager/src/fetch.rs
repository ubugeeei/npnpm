use crate::{InstallPackageBySnapshot, InstallPackageBySnapshotError};
use derive_more::{Display, Error};
use futures_util::future;
use miette::Diagnostic;
use pacquet_lockfile::{
    DependencyPath, Lockfile, LockfileResolution, PackageSnapshot, PackageSnapshotDependency,
    PkgName, PkgNameVerPeer, PkgVerPeer, ProjectSnapshot, RootProjectSnapshot,
};
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_package_manifest::DependencyGroup;
use pacquet_tarball::{MemCache, NetworkMode};
use std::collections::{HashMap, HashSet};

/// Fetch packages from a lockfile into the store and virtual store without creating root links.
#[must_use]
pub struct Fetch<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub tarball_mem_cache: &'a MemCache,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Npmrc,
    pub lockfile: &'a Lockfile,
    pub dependency_groups: DependencyGroupList,
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum FetchError {
    #[display("pnpm-lock.yaml does not contain any package snapshots")]
    MissingPackageSnapshots,

    #[display("Failed to resolve {package} from pnpm-lock.yaml")]
    ResolvePackage {
        package: String,
    },

    #[display("Package snapshot for {dependency_path} is missing from pnpm-lock.yaml")]
    MissingSnapshot {
        dependency_path: String,
    },

    #[display("Failed to parse optional dependency {name}@{reference}: {error}")]
    ParseOptionalDependency {
        name: String,
        reference: String,
        error: String,
    },

    #[display("Lockfile resolution is not supported by fetch yet: {dependency_path}")]
    UnsupportedResolution {
        dependency_path: String,
    },

    InstallPackage(#[error(source)] InstallPackageBySnapshotError),
}

impl<'a, DependencyGroupList> Fetch<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub async fn run(self) -> Result<(), FetchError> {
        let Fetch { tarball_mem_cache, http_client, config, lockfile, dependency_groups } = self;
        let packages = lockfile.packages.as_ref().ok_or(FetchError::MissingPackageSnapshots)?;
        let dependency_groups = dependency_groups.into_iter().collect::<Vec<_>>();
        let index = LockfilePackageIndex::new(packages);
        let selected = collect_selected_packages(
            &lockfile.project_snapshot,
            packages,
            &index,
            &dependency_groups,
        )?;

        let results = future::join_all(selected.iter().map(|dependency_path| {
            let package_snapshot =
                packages.get(dependency_path).expect("selected package snapshot exists");
            async move {
                InstallPackageBySnapshot {
                    tarball_mem_cache,
                    http_client,
                    config,
                    dependency_path,
                    package_snapshot,
                    network_mode: NetworkMode::Online,
                }
                .run()
                .await
            }
        }))
        .await;

        for result in results {
            result.map_err(FetchError::InstallPackage)?;
        }

        Ok(())
    }
}

struct LockfilePackageIndex<'a> {
    by_specifier: HashMap<PkgNameVerPeer, Vec<&'a DependencyPath>>,
    by_dependency_path: HashMap<String, &'a DependencyPath>,
}

impl<'a> LockfilePackageIndex<'a> {
    fn new(packages: &'a HashMap<DependencyPath, PackageSnapshot>) -> Self {
        let mut by_specifier = HashMap::<PkgNameVerPeer, Vec<&DependencyPath>>::new();
        let mut by_dependency_path =
            HashMap::<String, &'a DependencyPath>::with_capacity(packages.len());
        for dependency_path in packages.keys() {
            by_specifier
                .entry(dependency_path.package_specifier.clone())
                .or_default()
                .push(dependency_path);
            by_dependency_path.insert(dependency_path.to_string(), dependency_path);
        }
        Self { by_specifier, by_dependency_path }
    }

    fn resolve_by_specifier(
        &self,
        custom_registry: Option<&str>,
        package_specifier: &PkgNameVerPeer,
    ) -> Option<&'a DependencyPath> {
        let candidates = self.by_specifier.get(package_specifier)?;
        candidates
            .iter()
            .copied()
            .find(|candidate| candidate.custom_registry.as_deref() == custom_registry)
            .or_else(|| candidates.first().copied())
    }

    fn resolve_root_dependency(
        &self,
        name: &PkgName,
        version: &PkgVerPeer,
    ) -> Option<&'a DependencyPath> {
        self.resolve_by_specifier(None, &PkgNameVerPeer::new(name.clone(), version.clone()))
    }

    fn resolve_snapshot_dependency(
        &self,
        parent: &'a DependencyPath,
        name: &PkgName,
        dependency: &'a PackageSnapshotDependency,
    ) -> Option<&'a DependencyPath> {
        match dependency {
            PackageSnapshotDependency::PkgVerPeer(version) => self.resolve_by_specifier(
                parent.custom_registry.as_deref(),
                &PkgNameVerPeer::new(name.clone(), version.clone()),
            ),
            PackageSnapshotDependency::DependencyPath(path) => Some(path),
        }
    }

    fn resolve_optional_dependency(
        &self,
        parent: &'a DependencyPath,
        name: &str,
        reference: &str,
    ) -> Result<Option<&'a DependencyPath>, FetchError> {
        if reference.parse::<DependencyPath>().is_ok() {
            return Ok(self.by_dependency_path.get(reference).copied());
        }

        let version = reference.parse::<PkgVerPeer>().map_err(|error| {
            FetchError::ParseOptionalDependency {
                name: name.to_string(),
                reference: reference.to_string(),
                error: error.to_string(),
            }
        })?;
        let package_name =
            name.parse::<PkgName>().map_err(|error| FetchError::ParseOptionalDependency {
                name: name.to_string(),
                reference: reference.to_string(),
                error: error.to_string(),
            })?;
        Ok(self.resolve_by_specifier(
            parent.custom_registry.as_deref(),
            &PkgNameVerPeer::new(package_name, version),
        ))
    }
}

fn collect_selected_packages<'a>(
    project_snapshot: &'a RootProjectSnapshot,
    packages: &'a HashMap<DependencyPath, PackageSnapshot>,
    index: &'a LockfilePackageIndex<'a>,
    dependency_groups: &[DependencyGroup],
) -> Result<Vec<DependencyPath>, FetchError> {
    let mut stack = match project_snapshot {
        RootProjectSnapshot::Single(project) => {
            root_dependency_paths(project, index, dependency_groups)?
        }
        RootProjectSnapshot::Multi(projects) => projects
            .importers
            .values()
            .map(|project| root_dependency_paths(project, index, dependency_groups))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect(),
    };
    let mut visited = HashSet::with_capacity(stack.len());
    let mut selected = Vec::with_capacity(stack.len());

    while let Some(dependency_path) = stack.pop() {
        if !visited.insert(dependency_path.clone()) {
            continue;
        }

        let package_snapshot = packages.get(&dependency_path).ok_or_else(|| {
            FetchError::MissingSnapshot { dependency_path: dependency_path.to_string() }
        })?;

        match package_snapshot.resolution {
            LockfileResolution::Directory(_) => {}
            LockfileResolution::Git(_) => {
                return Err(FetchError::UnsupportedResolution {
                    dependency_path: dependency_path.to_string(),
                });
            }
            LockfileResolution::Registry(_) | LockfileResolution::Tarball(_) => {
                selected.push(dependency_path.clone());
            }
        }

        for (name, dependency) in package_snapshot.dependencies.iter().flatten() {
            let child = index
                .resolve_snapshot_dependency(&dependency_path, name, dependency)
                .ok_or_else(|| FetchError::ResolvePackage { package: name.to_string() })?;
            stack.push(child.clone());
        }

        for (name, reference) in package_snapshot.optional_dependencies.iter().flatten() {
            let Some(child) =
                index.resolve_optional_dependency(&dependency_path, name, reference)?
            else {
                return Err(FetchError::ResolvePackage { package: name.to_string() });
            };
            stack.push(child.clone());
        }
    }

    Ok(selected)
}

fn root_dependency_paths<'a>(
    project: &'a ProjectSnapshot,
    index: &'a LockfilePackageIndex<'a>,
    dependency_groups: &[DependencyGroup],
) -> Result<Vec<DependencyPath>, FetchError> {
    project
        .dependencies_by_groups(dependency_groups.iter().copied())
        .map(|(name, dependency)| {
            index
                .resolve_root_dependency(name, &dependency.version)
                .cloned()
                .ok_or_else(|| FetchError::ResolvePackage { package: name.to_string() })
        })
        .collect()
}
