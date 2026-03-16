use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use pacquet_network::ThrottledClient;
use pipe_trait::Pipe;
use serde::{Deserialize, Serialize};

use crate::{package_version::PackageVersion, NetworkError, PackageTag, RegistryError};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Package {
    pub name: String,
    #[serde(rename = "dist-tags")]
    dist_tags: HashMap<String, String>,
    pub versions: HashMap<String, PackageVersion>,

    #[serde(skip_serializing, skip_deserializing)]
    pub mutex: Arc<Mutex<u8>>,
}

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Package {
    pub async fn fetch_from_registry(
        name: &str,
        http_client: &ThrottledClient,
        registry: &str,
    ) -> Result<Self, RegistryError> {
        let url = || format!("{registry}{name}"); // TODO: use reqwest URL directly
        let network_error = |error| NetworkError { error, url: url() };
        http_client
            .run_with_permit(|client| {
                client
                    .get(url())
                    .header(
                        "accept",
                        "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
                    )
                    .send()
            })
            .await
            .map_err(network_error)?
            .json::<Package>()
            .await
            .map_err(network_error)?
            .pipe(Ok)
    }

    pub fn pinned_version(&self, version_range: &str) -> Option<&PackageVersion> {
        let range: node_semver::Range = version_range.parse().unwrap(); // TODO: this step should have happened in PackageManifest
        self.versions
            .values()
            .filter(|version| version.version.satisfies(&range))
            .max_by(|left, right| left.version.partial_cmp(&right.version).unwrap())
    }

    pub fn version_by_tag(&self, tag: PackageTag) -> Result<&PackageVersion, RegistryError> {
        match tag {
            PackageTag::Latest => {
                let version = self
                    .dist_tags
                    .get("latest")
                    .ok_or_else(|| RegistryError::MissingLatestTag(self.name.clone()))?;
                self.versions.get(version).ok_or_else(|| {
                    RegistryError::MissingVersionRelease(version.clone(), self.name.clone())
                })
            }
            PackageTag::Version(version) => {
                let version = version.to_string();
                self.versions
                    .get(&version)
                    .ok_or_else(|| RegistryError::MissingVersionRelease(version, self.name.clone()))
            }
        }
    }

    pub fn version_by_specifier(
        &self,
        version_range: &str,
    ) -> Result<&PackageVersion, RegistryError> {
        if let Ok(tag) = version_range.parse::<PackageTag>() {
            return self.version_by_tag(tag);
        }

        self.pinned_version(version_range).ok_or_else(|| {
            RegistryError::MissingVersionRelease(version_range.to_string(), self.name.clone())
        })
    }

    pub fn latest(&self) -> &PackageVersion {
        let version =
            self.dist_tags.get("latest").expect("latest tag is expected but not found for package");
        self.versions.get(version).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use node_semver::Version;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::package_distribution::PackageDistribution;

    #[test]
    pub fn package_version_should_include_peers() {
        let mut dependencies = HashMap::<String, String>::new();
        dependencies.insert("fastify".to_string(), "1.0.0".to_string());
        let mut peer_dependencies = HashMap::<String, String>::new();
        peer_dependencies.insert("fast-querystring".to_string(), "1.0.0".to_string());
        let version = PackageVersion {
            name: "".to_string(),
            version: Version::parse("1.0.0").unwrap(),
            dist: PackageDistribution::default(),
            dependencies: Some(dependencies),
            optional_dependencies: None,
            dev_dependencies: None,
            peer_dependencies: Some(peer_dependencies),
        };

        let dependencies = |peer| version.dependencies(peer).collect::<HashMap<_, _>>();
        assert!(dependencies(false).contains_key("fastify"));
        assert!(!dependencies(false).contains_key("fast-querystring"));
        assert!(dependencies(true).contains_key("fastify"));
        assert!(dependencies(true).contains_key("fast-querystring"));
        assert!(!dependencies(true).contains_key("hello-world"));
    }

    #[test]
    pub fn serialized_according_to_params() {
        let version = PackageVersion {
            name: "".to_string(),
            version: Version { major: 3, minor: 2, patch: 1, build: vec![], pre_release: vec![] },
            dist: PackageDistribution::default(),
            dependencies: None,
            optional_dependencies: None,
            dev_dependencies: None,
            peer_dependencies: None,
        };

        assert_eq!(version.serialize(true), "3.2.1");
        assert_eq!(version.serialize(false), "^3.2.1");
    }

    #[test]
    fn version_by_specifier_prefers_direct_lookups_before_range_resolution() {
        let package = Package {
            name: "demo".to_string(),
            dist_tags: HashMap::from([("latest".to_string(), "1.2.0".to_string())]),
            versions: HashMap::from([
                (
                    "1.0.0".to_string(),
                    PackageVersion {
                        name: "demo".to_string(),
                        version: Version::parse("1.0.0").unwrap(),
                        dist: PackageDistribution::default(),
                        dependencies: None,
                        optional_dependencies: None,
                        dev_dependencies: None,
                        peer_dependencies: None,
                    },
                ),
                (
                    "1.2.0".to_string(),
                    PackageVersion {
                        name: "demo".to_string(),
                        version: Version::parse("1.2.0").unwrap(),
                        dist: PackageDistribution::default(),
                        dependencies: None,
                        optional_dependencies: None,
                        dev_dependencies: None,
                        peer_dependencies: None,
                    },
                ),
            ]),
            mutex: Default::default(),
        };

        assert_eq!(
            package.version_by_specifier("latest").unwrap().version,
            Version::parse("1.2.0").unwrap()
        );
        assert_eq!(
            package.version_by_specifier("1.0.0").unwrap().version,
            Version::parse("1.0.0").unwrap()
        );
        assert_eq!(
            package.version_by_specifier("^1.0.0").unwrap().version,
            Version::parse("1.2.0").unwrap()
        );
    }
}
