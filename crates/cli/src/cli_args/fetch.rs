use clap::Args;
use miette::{miette, Context, IntoDiagnostic};
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_npmrc::Npmrc;
use pacquet_package_manager::Fetch;
use pacquet_package_manifest::DependencyGroup;
use pacquet_tarball::MemCache;
use std::path::Path;

#[derive(Debug, Args)]
pub struct FetchDependencyOptions {
    /// Development packages will not be fetched.
    #[arg(short = 'P', long)]
    prod: bool,
    /// Only development packages will be fetched.
    #[arg(short = 'D', long)]
    dev: bool,
}

impl FetchDependencyOptions {
    fn dependency_groups(&self) -> impl Iterator<Item = DependencyGroup> {
        let &FetchDependencyOptions { prod, dev } = self;
        let has_both = prod == dev;
        let has_prod = has_both || prod;
        let has_dev = has_both || dev;

        std::iter::empty()
            .chain(has_prod.then_some(DependencyGroup::Prod))
            .chain(has_dev.then_some(DependencyGroup::Dev))
            .chain(has_prod.then_some(DependencyGroup::Optional))
    }
}

#[derive(Debug, Args)]
pub struct FetchArgs {
    #[clap(flatten)]
    dependency_options: FetchDependencyOptions,
}

impl FetchArgs {
    pub async fn run(
        self,
        base_dir: &Path,
        npmrc: impl FnOnce() -> &'static Npmrc,
    ) -> miette::Result<()> {
        let lockfile = Lockfile::load_from_dir(base_dir)
            .into_diagnostic()
            .wrap_err("load pnpm-lock.yaml")?
            .ok_or_else(|| miette!("pnpm-lock.yaml is required for fetch"))?;
        let tarball_mem_cache = MemCache::new();
        let http_client = ThrottledClient::new_from_cpu_count();

        Fetch {
            tarball_mem_cache: &tarball_mem_cache,
            http_client: &http_client,
            config: npmrc(),
            lockfile: &lockfile,
            dependency_groups: self.dependency_options.dependency_groups(),
        }
        .run()
        .await
        .wrap_err("fetch packages from lockfile")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacquet_package_manifest::DependencyGroup;
    use pretty_assertions::assert_eq;

    #[test]
    fn dependency_options_to_dependency_groups() {
        use DependencyGroup::{Dev, Optional, Prod};
        let create_list =
            |opts: FetchDependencyOptions| opts.dependency_groups().collect::<Vec<_>>();

        assert_eq!(
            create_list(FetchDependencyOptions { prod: false, dev: false }),
            [Prod, Dev, Optional]
        );
        assert_eq!(
            create_list(FetchDependencyOptions { prod: true, dev: false }),
            [Prod, Optional]
        );
        assert_eq!(create_list(FetchDependencyOptions { prod: false, dev: true }), [Dev]);
        assert_eq!(
            create_list(FetchDependencyOptions { prod: true, dev: true }),
            [Prod, Dev, Optional]
        );
    }
}
