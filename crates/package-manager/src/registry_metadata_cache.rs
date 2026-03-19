use dashmap::{mapref::entry::Entry, DashMap};
use pacquet_fs::ensure_file;
use pacquet_network::ThrottledClient;
use pacquet_registry::{Package, RegistryError};
use pacquet_store_dir::StoreDir;
use std::{fs, future::Future, io::ErrorKind, sync::Arc};
use tokio::sync::OnceCell;

pub type RegistryMetadataCache = DashMap<String, Arc<OnceCell<Arc<Package>>>>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RegistryMetadataMode {
    #[default]
    Online,
    PreferOffline,
}

fn cache_key(registry: &str, name: &str) -> String {
    format!("{registry}\n{name}")
}

fn load_package_metadata_from_disk(
    store_dir: &StoreDir,
    registry: &str,
    name: &str,
) -> Result<Option<Package>, RegistryError> {
    let path = store_dir.registry_metadata_file_path(registry, name);
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(RegistryError::Io(error)),
    };

    serde_json::from_str(&contents)
        .map(Some)
        .map_err(|error| RegistryError::Serialization(error.to_string()))
}

fn save_package_metadata_to_disk(
    store_dir: &StoreDir,
    registry: &str,
    name: &str,
    package: &Package,
) {
    let path = store_dir.registry_metadata_file_path(registry, name);
    let contents = match serde_json::to_vec(package) {
        Ok(contents) => contents,
        Err(error) => {
            tracing::warn!(
                target: "pacquet::registry",
                package = name,
                registry,
                error = %error,
                "Skip persisting registry metadata cache"
            );
            return;
        }
    };

    if let Err(error) = ensure_file(&path, &contents, Some(0o666)) {
        tracing::warn!(
            target: "pacquet::registry",
            package = name,
            registry,
            error = %error,
            "Skip writing registry metadata cache"
        );
    }
}

pub fn fetch_package_metadata<'a>(
    cache: &RegistryMetadataCache,
    name: &'a str,
    http_client: &'a ThrottledClient,
    registry: &'a str,
    store_dir: &'a StoreDir,
    mode: RegistryMetadataMode,
) -> impl Future<Output = Result<Arc<Package>, RegistryError>> + Send + 'a {
    let cache_key = cache_key(registry, name);
    let cell = match cache.entry(cache_key) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
            let cell = Arc::new(OnceCell::new());
            entry.insert(cell.clone());
            cell
        }
    };

    async move {
        let package = cell
            .get_or_try_init(|| async move {
                if matches!(mode, RegistryMetadataMode::PreferOffline) {
                    if let Some(package) =
                        load_package_metadata_from_disk(store_dir, registry, name)?
                    {
                        tracing::info!(
                            target: "pacquet::registry",
                            package = name,
                            registry,
                            "Reuse on-disk registry metadata cache"
                        );
                        return Ok::<Arc<Package>, RegistryError>(Arc::new(package));
                    }
                }

                let package = Package::fetch_from_registry(name, http_client, registry).await?;
                save_package_metadata_to_disk(store_dir, registry, name, &package);
                Ok::<Arc<Package>, RegistryError>(Arc::new(package))
            })
            .await?;

        Ok(package.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future::join_all;
    use mockito::Server;
    use tempfile::tempdir;

    #[tokio::test]
    async fn should_fetch_package_metadata_only_once_for_concurrent_requests() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/left-pad")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                  "name": "left-pad",
                  "dist-tags": { "latest": "1.3.0" },
                  "versions": {
                    "1.3.0": {
                      "name": "left-pad",
                      "version": "1.3.0",
                      "dist": {
                        "tarball": "https://example.invalid/left-pad-1.3.0.tgz",
                        "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
                      }
                    }
                  }
                }"#,
            )
            .expect(1)
            .create_async()
            .await;

        let registry = format!("{}/", server.url());
        let cache = RegistryMetadataCache::new();
        let http_client = ThrottledClient::new_from_cpu_count();
        let tempdir = tempdir().unwrap();
        let store_dir = StoreDir::new(tempdir.path());

        let results = join_all((0..8).map(|_| async {
            fetch_package_metadata(
                &cache,
                "left-pad",
                &http_client,
                &registry,
                &store_dir,
                RegistryMetadataMode::Online,
            )
            .await
        }))
        .await;

        for result in results {
            assert_eq!(result.unwrap().name, "left-pad");
        }
    }

    #[tokio::test]
    async fn should_reuse_on_disk_metadata_cache_in_prefer_offline_mode() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/left-pad")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                  "name": "left-pad",
                  "dist-tags": { "latest": "1.3.0" },
                  "versions": {
                    "1.3.0": {
                      "name": "left-pad",
                      "version": "1.3.0",
                      "dist": {
                        "tarball": "https://example.invalid/left-pad-1.3.0.tgz",
                        "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
                      }
                    }
                  }
                }"#,
            )
            .expect(1)
            .create_async()
            .await;

        let cache = RegistryMetadataCache::new();
        let http_client = ThrottledClient::new_from_cpu_count();
        let tempdir = tempdir().unwrap();
        let store_dir = StoreDir::new(tempdir.path());
        let online_registry = format!("{}/", server.url());

        let package = fetch_package_metadata(
            &cache,
            "left-pad",
            &http_client,
            &online_registry,
            &store_dir,
            RegistryMetadataMode::Online,
        )
        .await
        .unwrap();
        assert_eq!(package.name, "left-pad");

        let prefer_offline_cache = RegistryMetadataCache::new();
        let offline_result = fetch_package_metadata(
            &prefer_offline_cache,
            "left-pad",
            &http_client,
            &online_registry,
            &store_dir,
            RegistryMetadataMode::PreferOffline,
        )
        .await
        .unwrap();
        assert_eq!(offline_result.name, "left-pad");
    }
}
