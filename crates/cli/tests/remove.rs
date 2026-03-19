use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Server;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_testing_utils::bin::CommandTempCwd;
use ssri::Integrity;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn fixture_tarball() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tasks/micro-benchmark/fixtures/@fastify+error-3.3.0.tgz");
    fs::read(path).expect("read tarball fixture")
}

fn pacquet_command(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find pacquet binary").with_current_dir(workspace)
}

#[test]
fn remove_should_update_manifest_and_cleanup_root_dependency_link() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let tarball = fixture_tarball();
    let integrity: Integrity =
        "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="
            .parse()
            .expect("parse tarball integrity");

    let mut server = Server::new();
    let registry = format!("{}/", server.url());
    fs::write(workspace.join(".npmrc"), format!("store-dir=foo/bar\nregistry={registry}\n"))
        .expect("write to .npmrc");

    let package = serde_json::json!({
        "name": "root",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "root",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/root/-/root-1.0.0.tgz", server.url()),
                    "integrity": integrity.to_string(),
                    "unpackedSize": 16697
                }
            }
        }
    });

    server
        .mock("GET", "/root")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package.to_string())
        .expect(1)
        .create();
    server
        .mock("GET", "/root/-/root-1.0.0.tgz")
        .with_status(200)
        .with_body(tarball)
        .expect(1)
        .create();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "root": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet_command(&workspace).with_arg("install").assert().success();
    assert!(workspace.join("node_modules/root").exists());

    pacquet_command(&workspace).with_args(["remove", "root"]).assert().success();

    let manifest = PackageManifest::from_path(workspace.join("package.json")).unwrap();
    assert_eq!(manifest.dependency_version("root", DependencyGroup::Prod), None);
    assert!(!workspace.join("node_modules/root").exists());

    drop(root);
}
