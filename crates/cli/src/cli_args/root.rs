use clap::Args;
use pacquet_npmrc::Npmrc;

#[derive(Debug, Args, Default)]
pub struct RootArgs;

impl RootArgs {
    pub fn run(self, config: &Npmrc) -> miette::Result<()> {
        println!("{}", config.modules_dir.display());
        Ok(())
    }
}
