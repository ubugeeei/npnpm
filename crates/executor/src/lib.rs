use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ExecutorError {
    #[display("Failed to spawn command: {_0}")]
    #[diagnostic(code(pacquet_executor::spawn_command))]
    SpawnCommand(#[error(source)] std::io::Error),

    #[display("Process exits with an error: {_0}")]
    #[diagnostic(code(pacquet_executor::wait_process))]
    WaitProcess(#[error(source)] std::io::Error),

    #[display("Process exited unsuccessfully with status {status}")]
    #[diagnostic(code(pacquet_executor::exit_status))]
    ExitStatus { status: ExitStatus },

    #[display("Failed to build PATH for child process: {_0}")]
    #[diagnostic(code(pacquet_executor::join_paths))]
    JoinPaths(#[error(not(source))] String),
}

pub fn execute_shell(command: &str) -> Result<(), ExecutorError> {
    execute_shell_with_context(command, None, &[], None)
}

pub fn execute_shell_with_context(
    command: &str,
    cwd: Option<&Path>,
    extra_paths: &[PathBuf],
    npm_command: Option<&str>,
) -> Result<(), ExecutorError> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    apply_context(&mut cmd, cwd, extra_paths, npm_command)?;

    let mut cmd = cmd.spawn().map_err(ExecutorError::SpawnCommand)?;
    wait_for_success(&mut cmd)
}

pub fn execute_binary_with_context(
    binary: &str,
    args: &[String],
    cwd: Option<&Path>,
    extra_paths: &[PathBuf],
    npm_command: Option<&str>,
) -> Result<(), ExecutorError> {
    let mut cmd = Command::new(binary);
    cmd.args(args);
    apply_context(&mut cmd, cwd, extra_paths, npm_command)?;

    let mut cmd = cmd.spawn().map_err(ExecutorError::SpawnCommand)?;
    wait_for_success(&mut cmd)
}

fn apply_context(
    cmd: &mut Command,
    cwd: Option<&Path>,
    extra_paths: &[PathBuf],
    npm_command: Option<&str>,
) -> Result<(), ExecutorError> {
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    if let Some(npm_command) = npm_command {
        cmd.env("npm_command", npm_command);
    }

    if !extra_paths.is_empty() {
        cmd.env("PATH", joined_path_env(extra_paths)?);
    }

    Ok(())
}

fn joined_path_env(extra_paths: &[PathBuf]) -> Result<OsString, ExecutorError> {
    let current_path = env::var_os("PATH");
    let current_paths =
        current_path.as_ref().map(env::split_paths).into_iter().flatten().collect::<Vec<_>>();
    let paths = extra_paths.iter().cloned().chain(current_paths);
    env::join_paths(paths).map_err(|error| ExecutorError::JoinPaths(error.to_string()))
}

fn wait_for_success(child: &mut std::process::Child) -> Result<(), ExecutorError> {
    let status = child.wait().map_err(ExecutorError::WaitProcess)?;
    if status.success() {
        Ok(())
    } else {
        Err(ExecutorError::ExitStatus { status })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[cfg(unix)]
    fn set_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let metadata = fs::metadata(path).unwrap();
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn execute_shell_should_fail_on_non_zero_exit() {
        let error = execute_shell("exit 7").expect_err("command should fail");
        assert!(matches!(error, ExecutorError::ExitStatus { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn execute_binary_with_context_should_use_extra_path_entries() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("node_modules/.bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let script_path = bin_dir.join("hello-local");
        fs::write(&script_path, "#!/bin/sh\nexit 0\n").unwrap();
        set_executable(&script_path);

        execute_binary_with_context("hello-local", &[], Some(dir.path()), &[bin_dir], Some("exec"))
            .expect("binary should be found via PATH");
    }
}
