use dashmap::{mapref::entry::Entry, DashMap};
use pacquet_network::ThrottledClient;
use pacquet_registry::{Package, RegistryError};
use std::sync::Arc;
use tokio::sync::OnceCell;

pub type RegistryMetadataCache = DashMap<String, Arc<OnceCell<Arc<Package>>>>;

pub async fn fetch_package_metadata(
    cache: &RegistryMetadataCache,
    name: &str,
    http_client: &ThrottledClient,
    registry: &str,
) -> Result<Arc<Package>, RegistryError> {
    let cell = match cache.entry(name.to_string()) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
            let cell = Arc::new(OnceCell::new());
            entry.insert(cell.clone());
            cell
        }
    };

    let package = cell
        .get_or_try_init(|| async move {
            Package::fetch_from_registry(name, http_client, registry).await.map(Arc::new)
        })
        .await?;

    Ok(package.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future::join_all;
    use mockito::Server;

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

        let results = join_all((0..8).map(|_| async {
            fetch_package_metadata(&cache, "left-pad", &http_client, &registry).await
        }))
        .await;

        for result in results {
            assert_eq!(result.unwrap().name, "left-pad");
        }
    }
}
