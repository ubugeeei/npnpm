pub mod add;
pub mod bin;
pub mod exec;
pub mod install;
pub mod remove;
pub mod root;
pub mod run;
pub mod store;
pub mod update;

use crate::State;
use add::AddArgs;
use bin::BinArgs;
use clap::{Parser, Subcommand};
use exec::ExecArgs;
use install::InstallArgs;
use miette::{Context, IntoDiagnostic};
use pacquet_executor::execute_shell_with_context;
use pacquet_npmrc::Npmrc;
use pacquet_package_manifest::PackageManifest;
use remove::RemoveArgs;
use root::RootArgs;
use run::{project_bin_paths, run_script, RunArgs};
use std::{
    env,
    path::{Component, Path, PathBuf},
};
use store::StoreCommand;
use update::UpdateArgs;

/// Experimental package manager for node.js written in rust.
#[derive(Debug, Parser)]
#[clap(name = "pacquet")]
#[clap(bin_name = "pacquet")]
#[clap(version = "0.2.1")]
#[clap(about = "Experimental package manager for node.js")]
pub struct CliArgs {
    #[clap(subcommand)]
    pub command: CliCommand,

    /// Set working directory.
    #[clap(short = 'C', long, default_value = ".")]
    pub dir: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum CliCommand {
    /// Initialize a package.json
    Init,
    /// Add a package
    Add(AddArgs),
    /// Install packages
    Install(InstallArgs),
    /// Remove packages
    #[clap(alias = "rm", alias = "uninstall", alias = "un")]
    Remove(RemoveArgs),
    /// Update packages
    #[clap(alias = "up", alias = "upgrade")]
    Update(UpdateArgs),
    /// Runs a package's "test" script, if one was provided.
    Test,
    /// Runs a defined package script.
    Run(RunArgs),
    /// Executes a shell command in the context of the project.
    Exec(ExecArgs),
    /// Prints the effective modules directory.
    Root(RootArgs),
    /// Prints the directory into which dependency executables are linked.
    Bin(BinArgs),
    /// Runs an arbitrary command specified in the package's start property of its scripts object.
    Start,
    /// Managing the package store.
    #[clap(subcommand)]
    Store(StoreCommand),
    /// Run a script without explicitly typing `run`.
    #[clap(external_subcommand)]
    Script(Vec<String>),
}

impl CliArgs {
    /// Execute the command
    pub async fn run(self) -> miette::Result<()> {
        let CliArgs { command, dir } = self;
        let base_dir =
            resolve_base_dir(&dir).into_diagnostic().wrap_err("resolve the working directory")?;
        let manifest_path = || base_dir.join("package.json");
        let npmrc = || {
            Npmrc::current(
                || Ok::<_, std::io::Error>(base_dir.clone()),
                home::home_dir,
                Default::default,
            )
            .leak()
        };
        let state = || State::init(manifest_path(), npmrc()).wrap_err("initialize the state");

        match command {
            CliCommand::Init => {
                PackageManifest::init(&manifest_path()).wrap_err("initialize package.json")?;
            }
            CliCommand::Add(args) => args.run(state()?).await?,
            CliCommand::Install(args) => args.run(state()?).await?,
            CliCommand::Remove(args) => args.run(state()?).await?,
            CliCommand::Update(args) => args.run(state()?).await?,
            CliCommand::Test => {
                let manifest = PackageManifest::from_path(manifest_path())
                    .wrap_err("getting the package.json in current directory")?;
                if let Some(script) = manifest.script("test", false)? {
                    execute_shell_with_context(
                        script,
                        Some(&base_dir),
                        &project_bin_paths(&base_dir),
                        Some("test"),
                    )
                    .wrap_err(format!("executing command: \"{0}\"", script))?;
                }
            }
            CliCommand::Run(args) => args.run(manifest_path(), &base_dir)?,
            CliCommand::Exec(args) => args.run(&base_dir)?,
            CliCommand::Root(args) => args.run(npmrc())?,
            CliCommand::Bin(args) => args.run(npmrc())?,
            CliCommand::Start => {
                // Runs an arbitrary command specified in the package's start property of its scripts
                // object. If no start property is specified on the scripts object, it will attempt to
                // run node server.js as a default, failing if neither are present.
                // The intended usage of the property is to specify a command that starts your program.
                let manifest = PackageManifest::from_path(manifest_path())
                    .wrap_err("getting the package.json in current directory")?;
                let command = if let Some(script) = manifest.script("start", true)? {
                    script
                } else {
                    "node server.js"
                };
                execute_shell_with_context(
                    command,
                    Some(&base_dir),
                    &project_bin_paths(&base_dir),
                    Some("start"),
                )
                .wrap_err(format!("executing command: \"{0}\"", command))?;
            }
            CliCommand::Store(command) => command.run(|| npmrc()).await?,
            CliCommand::Script(args) => {
                let Some((command, args)) = args.split_first() else {
                    return Ok(());
                };
                run_script(manifest_path(), &base_dir, command.clone(), args.to_vec(), false)?;
            }
        }

        Ok(())
    }
}

fn resolve_base_dir(dir: &Path) -> std::io::Result<PathBuf> {
    if dir.is_absolute() {
        Ok(normalize_path(dir.to_path_buf()))
    } else {
        env::current_dir().map(|current_dir| normalize_path(current_dir.join(dir)))
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
