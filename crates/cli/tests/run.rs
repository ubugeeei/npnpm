#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).expect("write executable file");
    let metadata = fs::metadata(path).expect("read executable metadata");
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("set executable permissions");
}

#[test]
fn run_should_use_node_modules_bin_in_path() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    fs::create_dir_all(workspace.join("node_modules/.bin")).expect("create .bin directory");
    write_executable(
        &workspace.join("node_modules/.bin/hello-local"),
        "#!/bin/sh\necho from-run\n",
    );
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "scripts": {
                "hello": "hello-local",
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["run", "hello"]).assert().success().stdout("from-run\n");

    drop(root);
}

#[test]
fn test_should_fail_on_non_zero_exit() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "scripts": {
                "test": "exit 7",
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("test").assert().failure();

    drop(root);
}

#[test]
fn exec_should_use_node_modules_bin_in_path() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    fs::create_dir_all(workspace.join("node_modules/.bin")).expect("create .bin directory");
    write_executable(&workspace.join("node_modules/.bin/hello-exec"), "#!/bin/sh\necho \"$1\"\n");

    pacquet.with_args(["exec", "hello-exec", "from-exec"]).assert().success().stdout("from-exec\n");

    drop(root);
}

#[test]
fn start_should_honor_c_option_and_use_local_bin_path() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let app_dir = workspace.join("app");
    fs::create_dir_all(app_dir.join("node_modules/.bin")).expect("create nested .bin directory");

    write_executable(
        &app_dir.join("node_modules/.bin/hello-start"),
        "#!/bin/sh\necho from-start\n",
    );
    fs::write(
        app_dir.join("package.json"),
        serde_json::json!({
            "scripts": {
                "start": "hello-start",
            }
        })
        .to_string(),
    )
    .expect("write nested package.json");

    pacquet.with_args(["-C", "app", "start"]).assert().success().stdout("from-start\n");

    drop(root);
}

#[test]
fn unknown_subcommand_should_run_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    fs::create_dir_all(workspace.join("node_modules/.bin")).expect("create .bin directory");
    write_executable(
        &workspace.join("node_modules/.bin/hello-short"),
        "#!/bin/sh\necho from-shortcut\n",
    );
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "scripts": {
                "hello": "hello-short",
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("hello").assert().success().stdout("from-shortcut\n");

    drop(root);
}
