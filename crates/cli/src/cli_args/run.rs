use clap::Args;
use miette::Context;
use pacquet_executor::execute_shell_with_context;
use pacquet_package_manifest::PackageManifest;
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
pub struct RunArgs {
    /// A pre-defined package script.
    pub command: String,

    /// Any additional arguments passed after the script name
    pub args: Vec<String>,

    /// You can use the --if-present flag to avoid exiting with a non-zero exit code when the
    /// script is undefined. This lets you run potentially undefined scripts without breaking the
    /// execution chain.
    #[clap(long)]
    pub if_present: bool,
}

impl RunArgs {
    /// Execute the subcommand.
    pub fn run(self, manifest_path: PathBuf, base_dir: &Path) -> miette::Result<()> {
        let RunArgs { command, args, if_present } = self;
        run_script(manifest_path, base_dir, command, args, if_present)
    }
}

pub fn run_script(
    manifest_path: PathBuf,
    base_dir: &Path,
    command: String,
    args: Vec<String>,
    if_present: bool,
) -> miette::Result<()> {
    let manifest = PackageManifest::from_path(manifest_path)
        .wrap_err("getting the package.json in current directory")?;

    if let Some(script) = manifest.script(&command, if_present)? {
        let mut command = script.to_string();
        // append an empty space between script and additional args
        command.push(' ');
        // then append the additional args
        command.push_str(&args.iter().map(|arg| shell_escape(arg)).collect::<Vec<_>>().join(" "));
        execute_shell_with_context(
            command.trim(),
            Some(base_dir),
            &project_bin_paths(base_dir),
            Some("run-script"),
        )?;
    }

    Ok(())
}

pub fn project_bin_paths(base_dir: &Path) -> Vec<PathBuf> {
    vec![base_dir.join("node_modules").join(".bin")]
}

fn shell_escape(arg: &str) -> String {
    let needs_quotes = arg.is_empty()
        || arg.chars().any(|char| {
            char.is_whitespace()
                || matches!(char, '\'' | '"' | '\\' | '$' | '`' | '!' | '&' | ';' | '|' | '<' | '>')
        });

    if needs_quotes {
        format!("'{}'", arg.replace('\'', "'\\''"))
    } else {
        arg.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::shell_escape;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_escape_shell_args() {
        assert_eq!(shell_escape("hello"), "hello");
        assert_eq!(shell_escape("hello world"), "'hello world'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }
}
