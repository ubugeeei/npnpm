use crate::State;
use clap::Args;
use miette::Context;
use pacquet_package_manager::Remove;
use pacquet_package_manifest::DependencyGroup;

#[derive(Debug, Args)]
pub struct RemoveDependencyOptions {
    /// Only remove the dependency from dependencies.
    #[clap(short = 'P', long)]
    save_prod: bool,
    /// Only remove the dependency from devDependencies.
    #[clap(short = 'D', long)]
    save_dev: bool,
    /// Only remove the dependency from optionalDependencies.
    #[clap(short = 'O', long)]
    save_optional: bool,
}

impl RemoveDependencyOptions {
    fn dependency_groups(&self) -> impl Iterator<Item = DependencyGroup> {
        let &RemoveDependencyOptions { save_prod, save_dev, save_optional } = self;
        let has_filter = save_prod || save_dev || save_optional;

        std::iter::empty()
            .chain((!has_filter || save_prod).then_some(DependencyGroup::Prod))
            .chain((!has_filter || save_dev).then_some(DependencyGroup::Dev))
            .chain((!has_filter || save_optional).then_some(DependencyGroup::Optional))
            .chain((!has_filter).then_some(DependencyGroup::Peer))
    }
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// One or more package names
    #[clap(required = true)]
    pub packages: Vec<String>,
    #[clap(flatten)]
    pub dependency_options: RemoveDependencyOptions,
}

impl RemoveArgs {
    pub async fn run(self, mut state: State) -> miette::Result<()> {
        let State {
            tarball_mem_cache,
            resolved_packages,
            registry_metadata_cache,
            http_client,
            config,
            manifest,
            ..
        } = &mut state;

        Remove {
            tarball_mem_cache,
            resolved_packages,
            registry_metadata_cache,
            http_client,
            config,
            manifest,
            list_dependency_groups: || self.dependency_options.dependency_groups(),
            packages: &self.packages,
        }
        .run()
        .await
        .wrap_err("removing package")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn dependency_options_to_dependency_groups() {
        use DependencyGroup::{Dev, Optional, Peer, Prod};
        let create_list =
            |opts: RemoveDependencyOptions| opts.dependency_groups().collect::<Vec<_>>();

        assert_eq!(
            create_list(RemoveDependencyOptions {
                save_prod: false,
                save_dev: false,
                save_optional: false,
            }),
            [Prod, Dev, Optional, Peer]
        );
        assert_eq!(
            create_list(RemoveDependencyOptions {
                save_prod: true,
                save_dev: false,
                save_optional: false,
            }),
            [Prod]
        );
        assert_eq!(
            create_list(RemoveDependencyOptions {
                save_prod: false,
                save_dev: true,
                save_optional: false,
            }),
            [Dev]
        );
        assert_eq!(
            create_list(RemoveDependencyOptions {
                save_prod: false,
                save_dev: false,
                save_optional: true,
            }),
            [Optional]
        );
    }
}
