use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{
    ComVer, DependencyPath, Lockfile, LockfileSettings, PackageSnapshot, PackageSnapshotDependency,
    PkgName, PkgNameVerPeer, PkgVerPeer, ProjectSnapshot, RegistryResolution,
    ResolvedDependencySpec, RootProjectSnapshot,
};
use pacquet_testing_utils::bin::CommandTempCwd;
use ssri::Integrity;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

fn fixture_tarball() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tasks/micro-benchmark/fixtures/@fastify+error-3.3.0.tgz");
    fs::read(path).expect("read tarball fixture")
}

fn pacquet_command(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find pacquet binary").with_current_dir(workspace)
}

fn write_lockfile(workspace: &Path, integrity: &Integrity) {
    let root_name = PkgName::from_str("root").unwrap();
    let dep_name = PkgName::from_str("dep").unwrap();
    let root_version = PkgVerPeer::from_str("1.0.0").unwrap();
    let dep_version = PkgVerPeer::from_str("1.0.0").unwrap();
    let root_path = DependencyPath {
        custom_registry: None,
        package_specifier: PkgNameVerPeer::new(root_name.clone(), root_version.clone()),
    };
    let dep_path = DependencyPath {
        custom_registry: None,
        package_specifier: PkgNameVerPeer::new(dep_name.clone(), dep_version.clone()),
    };

    let package_snapshot =
        |dependencies: Option<HashMap<PkgName, PackageSnapshotDependency>>| PackageSnapshot {
            resolution: RegistryResolution { integrity: integrity.clone() }.into(),
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
            dependencies,
            optional_dependencies: None,
            transitive_peer_dependencies: None,
            dev: Some(false),
            optional: Some(false),
        };

    Lockfile {
        lockfile_version: ComVer::new(6, 0).try_into().unwrap(),
        settings: Some(LockfileSettings::new(false, false)),
        never_built_dependencies: None,
        overrides: None,
        project_snapshot: RootProjectSnapshot::Single(ProjectSnapshot {
            specifiers: None,
            dependencies: Some(HashMap::from([(
                root_name,
                ResolvedDependencySpec { specifier: "1.0.0".to_string(), version: root_version },
            )])),
            optional_dependencies: None,
            dev_dependencies: None,
            dependencies_meta: None,
            publish_directory: None,
        }),
        packages: Some(HashMap::from([
            (
                root_path,
                package_snapshot(Some(HashMap::from([(
                    dep_name,
                    PackageSnapshotDependency::PkgVerPeer(dep_version),
                )]))),
            ),
            (dep_path, package_snapshot(None)),
        ])),
    }
    .save_to_dir(workspace)
    .expect("save pnpm-lock.yaml");
}

fn write_lockfile_with_dev_dependency(workspace: &Path, integrity: &Integrity) {
    let root_name = PkgName::from_str("root").unwrap();
    let dev_name = PkgName::from_str("devtool").unwrap();
    let root_version = PkgVerPeer::from_str("1.0.0").unwrap();
    let dev_version = PkgVerPeer::from_str("1.0.0").unwrap();

    let package_snapshot = || PackageSnapshot {
        resolution: RegistryResolution { integrity: integrity.clone() }.into(),
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
    };

    Lockfile {
        lockfile_version: ComVer::new(6, 0).try_into().unwrap(),
        settings: Some(LockfileSettings::new(false, false)),
        never_built_dependencies: None,
        overrides: None,
        project_snapshot: RootProjectSnapshot::Single(ProjectSnapshot {
            specifiers: None,
            dependencies: Some(HashMap::from([(
                root_name.clone(),
                ResolvedDependencySpec {
                    specifier: "1.0.0".to_string(),
                    version: root_version.clone(),
                },
            )])),
            optional_dependencies: None,
            dev_dependencies: Some(HashMap::from([(
                dev_name.clone(),
                ResolvedDependencySpec {
                    specifier: "1.0.0".to_string(),
                    version: dev_version.clone(),
                },
            )])),
            dependencies_meta: None,
            publish_directory: None,
        }),
        packages: Some(HashMap::from([
            (
                DependencyPath {
                    custom_registry: None,
                    package_specifier: PkgNameVerPeer::new(root_name, root_version),
                },
                package_snapshot(),
            ),
            (
                DependencyPath {
                    custom_registry: None,
                    package_specifier: PkgNameVerPeer::new(dev_name, dev_version),
                },
                package_snapshot(),
            ),
        ])),
    }
    .save_to_dir(workspace)
    .expect("save pnpm-lock.yaml");
}

#[test]
fn fetch_should_work_without_package_manifest_and_skip_root_links() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let tarball = fixture_tarball();
    let integrity: Integrity =
        "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="
            .parse()
            .expect("parse tarball integrity");

    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    fs::write(workspace.join(".npmrc"), format!("store-dir=foo/bar\nregistry={registry}\n"))
        .expect("write to .npmrc");

    write_lockfile(&workspace, &integrity);

    server
        .mock("GET", "/root/-/root-1.0.0.tgz")
        .with_status(200)
        .with_body(tarball.clone())
        .expect(1)
        .create();
    server
        .mock("GET", "/dep/-/dep-1.0.0.tgz")
        .with_status(200)
        .with_body(tarball)
        .expect(1)
        .create();

    pacquet_command(&workspace).with_arg("fetch").assert().success();

    assert!(!workspace.join("package.json").exists());
    assert!(!workspace.join("node_modules/root").exists());
    assert!(workspace.join("node_modules/.pnpm/root@1.0.0").exists());
    assert!(workspace.join("node_modules/.pnpm/dep@1.0.0").exists());

    drop(root);
}

#[test]
fn install_offline_should_reuse_prefetched_lockfile_packages() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let tarball = fixture_tarball();
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
    write_lockfile(&workspace, &integrity);

    server
        .mock("GET", "/root/-/root-1.0.0.tgz")
        .with_status(200)
        .with_body(tarball.clone())
        .expect(1)
        .create();
    server
        .mock("GET", "/dep/-/dep-1.0.0.tgz")
        .with_status(200)
        .with_body(tarball)
        .expect(1)
        .create();

    pacquet_command(&workspace).with_arg("fetch").assert().success();
    fs::write(workspace.join(".npmrc"), "store-dir=foo/bar\nregistry=http://127.0.0.1:9/\n")
        .expect("rewrite .npmrc");

    pacquet_command(&workspace).with_args(["install", "--offline"]).assert().success();

    assert!(workspace.join("node_modules/root").exists());
    assert!(workspace.join("node_modules/.pnpm/root@1.0.0").exists());

    drop(root);
}

#[test]
fn install_offline_should_fail_without_lockfile() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

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

    pacquet_command(&workspace).with_args(["install", "--offline"]).assert().failure();

    drop(root);
}

#[test]
fn fetch_prod_should_skip_dev_packages() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let tarball = fixture_tarball();
    let integrity: Integrity =
        "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="
            .parse()
            .expect("parse tarball integrity");

    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    fs::write(workspace.join(".npmrc"), format!("store-dir=foo/bar\nregistry={registry}\n"))
        .expect("write to .npmrc");
    write_lockfile_with_dev_dependency(&workspace, &integrity);

    server
        .mock("GET", "/root/-/root-1.0.0.tgz")
        .with_status(200)
        .with_body(tarball)
        .expect(1)
        .create();

    pacquet_command(&workspace).with_args(["fetch", "--prod"]).assert().success();

    assert!(workspace.join("node_modules/.pnpm/root@1.0.0").exists());
    assert!(!workspace.join("node_modules/.pnpm/devtool@1.0.0").exists());

    drop(root);
}
