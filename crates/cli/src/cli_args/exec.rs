use clap::Args;
use pacquet_executor::execute_binary_with_context;
use std::path::Path;

use super::run::project_bin_paths;

#[derive(Debug, Args)]
pub struct ExecArgs {
    /// The command to execute.
    pub command: String,

    /// Arguments passed to the command.
    pub args: Vec<String>,
}

impl ExecArgs {
    /// Execute the subcommand.
    pub fn run(self, base_dir: &Path) -> miette::Result<()> {
        let ExecArgs { command, args } = self;
        execute_binary_with_context(
            &command,
            &args,
            Some(base_dir),
            &project_bin_paths(base_dir),
            Some("exec"),
        )?;
        Ok(())
    }
}
