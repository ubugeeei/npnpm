use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{
    ComVer, Lockfile, LockfileSettings, PackageSnapshot, ProjectSnapshot, PkgName, PkgVerPeer,
    RegistryResolution, ResolvedDependencyMap, ResolvedDependencySpec, RootProjectSnapshot,
};
use pacquet_testing_utils::bin::CommandTempCwd;
use ssri::Integrity;
use std::{
    collections::HashMap,
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

fn fixture_lockfile(specifier: &str, version: &str) -> Lockfile {
    let mut dependencies = ResolvedDependencyMap::new();
    dependencies.insert(
        "root".parse::<PkgName>().expect("parse package name"),
        ResolvedDependencySpec {
            specifier: specifier.to_string(),
            version: version.parse::<PkgVerPeer>().expect("parse resolved version"),
        },
    );

    let packages = HashMap::from([(
        format!("/root@{version}").parse().expect("parse dependency path"),
        PackageSnapshot {
            resolution: RegistryResolution {
                integrity:
                    "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="
                        .parse()
                        .expect("parse tarball integrity"),
            }
            .into(),
            id: None,
            name: None,
            version: None,
            engines: None,
            cpu: None,
            os: None,
            libc: None,
            deprecated: None,
            has_bin: None,
            prepare: None,
            requires_build: None,
            bundled_dependencies: None,
            peer_dependencies: None,
            peer_dependencies_meta: None,
            dependencies: None,
            optional_dependencies: None,
            transitive_peer_dependencies: None,
            dev: Some(false),
            optional: Some(false),
        },
    )]);

    Lockfile {
        lockfile_version: ComVer::new(6, 0).try_into().expect("valid lockfile version"),
        settings: Some(LockfileSettings::new(false, false)),
        never_built_dependencies: None,
        overrides: None,
        project_snapshot: RootProjectSnapshot::Single(ProjectSnapshot {
            specifiers: None,
            dependencies: Some(dependencies),
            optional_dependencies: None,
            dev_dependencies: None,
            dependencies_meta: None,
            publish_directory: None,
        }),
        packages: Some(packages),
    }
}

#[test]
fn install_lockfile_only_should_write_lockfile_without_node_modules() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let integrity: Integrity =
        "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="
            .parse()
            .expect("parse tarball integrity");

    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    fs::write(workspace.join(".npmrc"), format!("store-dir=foo/bar\nregistry={registry}\n"))
        .expect("write to .npmrc");
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
                },
                "dependencies": {
                    "dep": "1.0.0"
                }
            }
        }
    });
    let dep = serde_json::json!({
        "name": "dep",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "dep",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/dep/-/dep-1.0.0.tgz", server.url()),
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
        .mock("GET", "/dep")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(dep.to_string())
        .expect(1)
        .create();

    pacquet_command(&workspace).with_args(["install", "--lockfile-only"]).assert().success();

    let lockfile = Lockfile::load_from_dir(&workspace).unwrap().expect("load lockfile");
    assert!(lockfile
        .packages
        .as_ref()
        .is_some_and(|packages| packages.contains_key(&"/root@1.0.0".parse().unwrap())));
    assert!(!workspace.join("node_modules").exists());

    drop(root);
}

#[test]
fn install_resolution_only_should_update_lockfile_without_node_modules() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let _tarball = fixture_tarball();
    let integrity: Integrity =
        "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="
            .parse()
            .expect("parse tarball integrity");

    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    fs::write(workspace.join(".npmrc"), format!("store-dir=foo/bar\nregistry={registry}\n"))
        .expect("write to .npmrc");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "root": "^1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    let package = serde_json::json!({
        "name": "root",
        "dist-tags": { "latest": "1.1.0" },
        "versions": {
            "1.0.0": {
                "name": "root",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/root/-/root-1.0.0.tgz", server.url()),
                    "integrity": integrity.to_string(),
                    "unpackedSize": 16697
                }
            },
            "1.1.0": {
                "name": "root",
                "version": "1.1.0",
                "dist": {
                    "tarball": format!("{}/root/-/root-1.1.0.tgz", server.url()),
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

    pacquet_command(&workspace).with_args(["install", "--resolution-only"]).assert().success();

    let lockfile = Lockfile::load_from_dir(&workspace).unwrap().expect("load lockfile");
    assert!(lockfile
        .packages
        .as_ref()
        .is_some_and(|packages| packages.contains_key(&"/root@1.1.0".parse().unwrap())));
    assert!(!workspace.join("node_modules").exists());

    drop(root);
}

#[test]
fn install_lockfile_only_should_reuse_existing_lockfile_without_network() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    let server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    fs::write(
        workspace.join(".npmrc"),
        format!("store-dir=foo/bar\nlockfile=false\nregistry={registry}\n"),
    )
    .expect("write to .npmrc");
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
    fixture_lockfile("1.0.0", "1.0.0")
        .save_to_dir(&workspace)
        .expect("save fixture lockfile");

    pacquet_command(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    assert!(!workspace.join("node_modules").exists());

    drop((root, server));
}

#[test]
fn install_frozen_lockfile_should_fail_when_lockfile_is_outdated() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    let server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    fs::write(workspace.join(".npmrc"), format!("store-dir=foo/bar\nregistry={registry}\n"))
        .expect("write to .npmrc");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "root": "2.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    fixture_lockfile("1.0.0", "1.0.0")
        .save_to_dir(&workspace)
        .expect("save fixture lockfile");

    pacquet_command(&workspace).with_args(["install", "--frozen-lockfile"]).assert().failure();

    drop((root, server));
}
