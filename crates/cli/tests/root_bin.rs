use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn canonicalize(path: &Path) -> PathBuf {
    if cfg!(windows) {
        path.to_path_buf()
    } else {
        dunce::canonicalize(path).expect("canonicalize path")
    }
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/")
}

#[test]
fn root_should_return_modules_dir_from_npmrc() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join(".npmrc"), "modules-dir=foo/bar\n").expect("write to .npmrc");

    let output = pacquet.with_arg("root").output().expect("run pacquet root");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end().pipe(normalize),
        canonicalize(&workspace).join("foo/bar").to_string_lossy().pipe_as_ref(normalize),
    );

    drop(root);
}

#[test]
fn bin_should_return_bin_dir_from_npmrc() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join(".npmrc"), "modules-dir=foo/bar\n").expect("write to .npmrc");

    let output = pacquet.with_arg("bin").output().expect("run pacquet bin");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end().pipe(normalize),
        canonicalize(&workspace).join("foo/bar/.bin").to_string_lossy().pipe_as_ref(normalize),
    );

    drop(root);
}

#[test]
fn root_should_honor_c_option() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let app_dir = workspace.join("app");
    fs::create_dir_all(&app_dir).expect("create app dir");
    fs::write(app_dir.join(".npmrc"), "modules-dir=app_modules\n").expect("write to nested .npmrc");

    let output = pacquet.with_args(["-C", "app", "root"]).output().expect("run pacquet root");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end().pipe(normalize),
        canonicalize(&app_dir).join("app_modules").to_string_lossy().pipe_as_ref(normalize),
    );

    drop(root);
}
