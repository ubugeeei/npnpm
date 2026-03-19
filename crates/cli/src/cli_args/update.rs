use crate::State;
use clap::Args;
use miette::Context;
use pacquet_package_manager::Update;
use pacquet_package_manifest::DependencyGroup;

#[derive(Debug, Args)]
pub struct UpdateDependencyOptions {
    /// Only update packages in dependencies and optionalDependencies.
    #[arg(short = 'P', long)]
    prod: bool,
    /// Only update packages in devDependencies.
    #[arg(short = 'D', long)]
    dev: bool,
    /// Don't update packages in optionalDependencies.
    #[arg(long)]
    no_optional: bool,
}

impl UpdateDependencyOptions {
    fn dependency_groups(&self) -> impl Iterator<Item = DependencyGroup> {
        let &UpdateDependencyOptions { prod, dev, no_optional } = self;
        let has_both = prod == dev;
        let has_prod = has_both || prod;
        let has_dev = has_both || dev;
        let has_optional = !no_optional;
        std::iter::empty()
            .chain(has_prod.then_some(DependencyGroup::Prod))
            .chain(has_dev.then_some(DependencyGroup::Dev))
            .chain(has_optional.then_some(DependencyGroup::Optional))
    }
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Optional package selectors. When omitted, updates all dependencies.
    pub packages: Vec<String>,
    #[clap(flatten)]
    pub dependency_options: UpdateDependencyOptions,
    /// Update to the latest stable version, even across major versions.
    #[clap(short = 'L', long)]
    pub latest: bool,
}

impl UpdateArgs {
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

        Update {
            tarball_mem_cache,
            resolved_packages,
            registry_metadata_cache,
            http_client,
            config,
            manifest,
            dependency_groups: self.dependency_options.dependency_groups().collect(),
            packages: &self.packages,
            latest: self.latest,
        }
        .run()
        .await
        .wrap_err("updating dependencies")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn dependency_options_to_dependency_groups() {
        use DependencyGroup::{Dev, Optional, Prod};
        let create_list =
            |opts: UpdateDependencyOptions| opts.dependency_groups().collect::<Vec<_>>();

        assert_eq!(
            create_list(UpdateDependencyOptions { prod: false, dev: false, no_optional: false }),
            [Prod, Dev, Optional],
        );
        assert_eq!(
            create_list(UpdateDependencyOptions { prod: true, dev: false, no_optional: false }),
            [Prod, Optional],
        );
        assert_eq!(
            create_list(UpdateDependencyOptions { prod: false, dev: true, no_optional: false }),
            [Dev, Optional],
        );
        assert_eq!(
            create_list(UpdateDependencyOptions { prod: false, dev: false, no_optional: true }),
            [Prod, Dev],
        );
    }
}
