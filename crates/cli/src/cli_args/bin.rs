use clap::Args;
use pacquet_npmrc::Npmrc;

#[derive(Debug, Args, Default)]
pub struct BinArgs;

impl BinArgs {
    pub fn run(self, config: &Npmrc) -> miette::Result<()> {
        println!("{}", config.modules_dir.join(".bin").display());
        Ok(())
    }
}
