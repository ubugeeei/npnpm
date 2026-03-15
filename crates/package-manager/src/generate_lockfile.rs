use async_recursion::async_recursion;
use pacquet_lockfile::{
    ComVer, DependencyPath, Lockfile, LockfileResolution, LockfileSettings, PackageSnapshot,
    PackageSnapshotDependency, PkgName, PkgNameVerPeer, PkgVerPeer, ProjectSnapshot,
    RegistryResolution, ResolvedDependencyMap, ResolvedDependencySpec, RootProjectSnapshot,
    TarballResolution,
};
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_registry::{Package, PackageTag, PackageVersion, RegistryError};
use std::{collections::HashMap, str::FromStr};

/// Resolve a [`PackageManifest`] into a pnpm-compatible lockfile.
#[must_use]
pub struct GenerateLockfile<'a> {
    pub http_client: &'a ThrottledClient,
    pub config: &'static Npmrc,
    pub manifest: &'a PackageManifest,
    pub dependency_groups: &'a [DependencyGroup],
}

impl<'a> GenerateLockfile<'a> {
    pub async fn run(self) -> Result<Lockfile, RegistryError> {
        let GenerateLockfile { http_client, config, manifest, dependency_groups } = self;

        let mut builder = LockfileBuilder {
            http_client,
            config,
            package_cache: HashMap::new(),
            packages: HashMap::new(),
        };

        let dependencies =
            builder.build_root_group(manifest, dependency_groups, DependencyGroup::Prod).await?;
        let optional_dependencies = builder
            .build_root_group(manifest, dependency_groups, DependencyGroup::Optional)
            .await?;
        let dev_dependencies =
            builder.build_root_group(manifest, dependency_groups, DependencyGroup::Dev).await?;

        let project_snapshot = ProjectSnapshot {
            specifiers: None,
            dependencies,
            optional_dependencies,
            dev_dependencies,
            dependencies_meta: None,
            publish_directory: None,
        };

        Ok(Lockfile {
            lockfile_version: ComVer::new(6, 0).try_into().expect("valid lockfile version"),
            settings: Some(LockfileSettings::new(config.auto_install_peers, false)),
            never_built_dependencies: None,
            overrides: None,
            project_snapshot: RootProjectSnapshot::Single(project_snapshot),
            packages: (!builder.packages.is_empty()).then_some(builder.packages),
        })
    }
}

struct LockfileBuilder<'a> {
    http_client: &'a ThrottledClient,
    config: &'static Npmrc,
    package_cache: HashMap<String, Package>,
    packages: HashMap<DependencyPath, PackageSnapshot>,
}

impl<'a> LockfileBuilder<'a> {
    async fn build_root_group(
        &mut self,
        manifest: &PackageManifest,
        enabled_groups: &[DependencyGroup],
        group: DependencyGroup,
    ) -> Result<Option<ResolvedDependencyMap>, RegistryError> {
        if !enabled_groups.contains(&group) {
            return Ok(None);
        }

        let mut dependencies = ResolvedDependencyMap::new();
        for (name, specifier) in manifest.dependencies([group]) {
            let package = self.resolve_package_version(name, specifier).await?;
            let resolved_version = self.package_specifier(&package).suffix.clone();

            dependencies.insert(
                name.parse::<PkgName>().expect("package name from manifest is valid"),
                ResolvedDependencySpec {
                    specifier: specifier.to_string(),
                    version: resolved_version.clone(),
                },
            );

            self.build_package_snapshot(
                package,
                matches!(group, DependencyGroup::Dev),
                matches!(group, DependencyGroup::Optional),
            )
            .await?;
        }

        Ok((!dependencies.is_empty()).then_some(dependencies))
    }

    async fn resolve_package_version(
        &mut self,
        name: &str,
        version_range: &str,
    ) -> Result<PackageVersion, RegistryError> {
        if version_range == "latest" {
            return PackageVersion::fetch_from_registry(
                name,
                PackageTag::Latest,
                self.http_client,
                &self.config.registry,
            )
            .await;
        }

        if let Ok(version) = version_range.parse() {
            return PackageVersion::fetch_from_registry(
                name,
                PackageTag::Version(version),
                self.http_client,
                &self.config.registry,
            )
            .await;
        }

        let package = self.fetch_package(name).await?;
        package.pinned_version(version_range).cloned().ok_or_else(|| {
            RegistryError::MissingVersionRelease(version_range.to_string(), name.to_string())
        })
    }

    async fn fetch_package(&mut self, name: &str) -> Result<Package, RegistryError> {
        if let Some(package) = self.package_cache.get(name) {
            return Ok(package.clone());
        }

        let package =
            Package::fetch_from_registry(name, self.http_client, &self.config.registry).await?;
        self.package_cache.insert(name.to_string(), package.clone());
        Ok(package)
    }

    fn package_specifier(&self, package: &PackageVersion) -> PkgNameVerPeer {
        let name = package.name.parse::<PkgName>().expect("registry package name is valid");
        let version = package.version.to_string();
        let version = PkgVerPeer::from_str(&version).expect("registry version is valid");
        PkgNameVerPeer::new(name, version)
    }

    fn dependency_path(&self, package: &PackageVersion) -> DependencyPath {
        DependencyPath { custom_registry: None, package_specifier: self.package_specifier(package) }
    }

    fn resolution(&self, package: &PackageVersion) -> LockfileResolution {
        let integrity =
            package.dist.integrity.clone().expect("registry metadata should include integrity");

        if self.config.lockfile_include_tarball_url {
            TarballResolution { tarball: package.dist.tarball.clone(), integrity: Some(integrity) }
                .into()
        } else {
            RegistryResolution { integrity }.into()
        }
    }

    fn merge_package_flags(snapshot: &mut PackageSnapshot, dev: bool, optional: bool) {
        snapshot.dev = Some(snapshot.dev.unwrap_or(true) && dev);
        snapshot.optional = Some(snapshot.optional.unwrap_or(true) && optional);
    }

    #[async_recursion(?Send)]
    async fn build_package_snapshot(
        &mut self,
        package: PackageVersion,
        dev: bool,
        optional: bool,
    ) -> Result<(), RegistryError> {
        let dependency_path = self.dependency_path(&package);
        if let Some(snapshot) = self.packages.get_mut(&dependency_path) {
            Self::merge_package_flags(snapshot, dev, optional);
            return Ok(());
        }

        self.packages.insert(
            dependency_path.clone(),
            PackageSnapshot {
                resolution: self.resolution(&package),
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
                dev: Some(dev),
                optional: Some(optional),
            },
        );

        let mut dependencies = HashMap::new();
        for (name, version_range) in package.dependencies(self.config.auto_install_peers) {
            let child_package = self.resolve_package_version(name, version_range).await?;
            let child_optional = optional
                || package
                    .optional_dependencies
                    .as_ref()
                    .is_some_and(|optional_dependencies| optional_dependencies.contains_key(name));

            dependencies.insert(
                name.parse::<PkgName>().expect("registry dependency name is valid"),
                PackageSnapshotDependency::PkgVerPeer(
                    self.package_specifier(&child_package).suffix,
                ),
            );

            self.build_package_snapshot(child_package, dev, child_optional).await?;
        }

        let snapshot = self
            .packages
            .get_mut(&dependency_path)
            .expect("package snapshot exists after placeholder insert");
        snapshot.dependencies = (!dependencies.is_empty()).then_some(dependencies);
        Self::merge_package_flags(snapshot, dev, optional);

        Ok(())
    }
}
