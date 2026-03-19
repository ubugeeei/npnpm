use clap::Args;
use clap::Subcommand;
use miette::Context;
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_package_manager::{RegistryMetadataCache, ResolvedPackages, StoreAdd};
use pacquet_tarball::MemCache;

#[derive(Debug, Subcommand)]
pub enum StoreCommand {
    /// Checks for modified packages in the store.
    Status,
    /// Functionally equivalent to pnpm add, except this adds new packages to the store directly
    /// without modifying any projects or files outside of the store.
    Add(StoreAddArgs),
    /// Removes unreferenced packages from the store.
    /// Unreferenced packages are packages that are not used by any projects on the system.
    /// Packages can become unreferenced after most installation operations, for instance when
    /// dependencies are made redundant.
    Prune,
    /// Returns the path to the active store directory.
    Path,
}

#[derive(Debug, Args)]
pub struct StoreAddArgs {
    /// One or more package specifiers to prefetch into the store.
    #[clap(required = true)]
    pub packages: Vec<String>,
}

impl StoreCommand {
    /// Execute the subcommand.
    pub async fn run(self, config: impl FnOnce() -> &'static Npmrc) -> miette::Result<()> {
        match self {
            StoreCommand::Status => {
                config().store_dir.status().wrap_err("checking store status")?;
            }
            StoreCommand::Add(args) => {
                let config = config();
                let tarball_mem_cache = MemCache::new();
                let registry_metadata_cache = RegistryMetadataCache::new();
                let resolved_packages = ResolvedPackages::new();
                let http_client = ThrottledClient::new_from_cpu_count();

                StoreAdd {
                    tarball_mem_cache: &tarball_mem_cache,
                    registry_metadata_cache: &registry_metadata_cache,
                    resolved_packages: &resolved_packages,
                    http_client: &http_client,
                    config,
                    packages: &args.packages,
                }
                .run()
                .await
                .wrap_err("adding packages to the store")?;
            }
            StoreCommand::Prune => {
                config().store_dir.prune().wrap_err("pruning store")?;
            }
            StoreCommand::Path => {
                println!("{}", config().store_dir.display());
            }
        }

        Ok(())
    }
}
