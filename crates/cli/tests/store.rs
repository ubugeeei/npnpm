use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Server;
use pacquet_store_dir::{PackageFileInfo, PackageFilesIndex, StoreDir};
use pacquet_testing_utils::bin::CommandTempCwd;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use ssri::{Algorithm, Integrity, IntegrityOpts};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Handle the slight difference between OSes.
///
/// **TODO:** may be we should have handle them in the production code instead?
fn canonicalize(path: &Path) -> PathBuf {
    if cfg!(windows) {
        path.to_path_buf()
    } else {
        dunce::canonicalize(path).expect("canonicalize path")
    }
}

#[test]
fn store_path_should_return_store_dir_from_npmrc() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    eprintln!("Creating .npmrc...");
    fs::write(workspace.join(".npmrc"), "store-dir=foo/bar").expect("write to .npmrc");

    eprintln!("Executing pacquet store path...");
    let output = pacquet.with_args(["store", "path"]).output().expect("run pacquet store path");
    dbg!(&output);

    eprintln!("Exit status code");
    assert!(output.status.success());

    eprintln!("Stdout");
    let normalize = |path: &str| path.replace('\\', "/");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end().pipe(normalize),
        canonicalize(&workspace).join("foo/bar").to_string_lossy().pipe_as_ref(normalize),
    );

    drop(root); // cleanup
}

#[test]
fn store_status_should_succeed_for_an_empty_store() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    eprintln!("Creating .npmrc...");
    fs::write(workspace.join(".npmrc"), "store-dir=foo/bar").expect("write to .npmrc");

    eprintln!("Executing pacquet store status...");
    pacquet.with_args(["store", "status"]).assert().success();

    drop(root); // cleanup
}

#[test]
fn store_status_should_fail_when_store_files_are_tampered() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let store_dir = StoreDir::new(workspace.join("foo/bar"));

    eprintln!("Creating .npmrc...");
    fs::write(workspace.join(".npmrc"), "store-dir=foo/bar").expect("write to .npmrc");

    let file_contents = b"console.log('ok')\n";
    let file_integrity =
        IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(file_contents).result();
    let tarball_integrity =
        IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(b"tarball").result();

    let (file_path, _) = store_dir.write_cas_file(file_contents, false).unwrap();
    let index = PackageFilesIndex {
        files: std::collections::HashMap::from([(
            "index.js".to_string(),
            PackageFileInfo {
                checked_at: None,
                integrity: file_integrity.to_string(),
                mode: 0o644,
                size: Some(file_contents.len() as u64),
            },
        )]),
    };
    store_dir.write_index_file(&tarball_integrity, &index).unwrap();
    fs::write(file_path, b"tampered\n").unwrap();

    eprintln!("Executing pacquet store status...");
    pacquet.with_args(["store", "status"]).assert().failure();

    drop(root); // cleanup
}

fn fixture_tarball() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tasks/micro-benchmark/fixtures/@fastify+error-3.3.0.tgz");
    fs::read(path).expect("read tarball fixture")
}

#[test]
fn store_add_should_prefetch_into_store_without_creating_project_files() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let store_dir = StoreDir::new(workspace.join("foo/bar"));
    let tarball = fixture_tarball();
    let integrity: Integrity =
        "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="
            .parse()
            .expect("parse tarball integrity");

    let mut server = Server::new();
    let registry = format!("{}/", server.url());
    fs::write(workspace.join(".npmrc"), format!("store-dir=foo/bar\nregistry={registry}\n"))
        .expect("write to .npmrc");

    let root_package = serde_json::json!({
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
    let dep_package = serde_json::json!({
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
        .with_body(root_package.to_string())
        .expect(1)
        .create();
    server
        .mock("GET", "/dep")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(dep_package.to_string())
        .expect(1)
        .create();
    server
        .mock("GET", "/root/-/root-1.0.0.tgz")
        .with_status(200)
        .with_body(tarball)
        .expect(1)
        .create();

    eprintln!("Executing pacquet store add...");
    pacquet.with_args(["store", "add", "root"]).assert().success();

    let status = store_dir.status().unwrap();
    assert!(status.checked_files > 0);
    assert!(!workspace.join("node_modules").exists());

    drop(root); // cleanup
}

#[test]
fn store_prune_should_remove_orphaned_cas_files() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let store_dir = StoreDir::new(workspace.join("foo/bar"));

    eprintln!("Creating .npmrc...");
    fs::write(workspace.join(".npmrc"), "store-dir=foo/bar").expect("write to .npmrc");

    let kept_contents = b"console.log('kept')\n";
    let orphan_contents = b"console.log('remove')\n";
    let kept_integrity =
        IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(kept_contents).result();
    let tarball_integrity =
        IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(b"tarball").result();

    let (kept_path, _) = store_dir.write_cas_file(kept_contents, false).unwrap();
    let (orphan_path, _) = store_dir.write_cas_file(orphan_contents, false).unwrap();
    let index = PackageFilesIndex {
        files: std::collections::HashMap::from([(
            "index.js".to_string(),
            PackageFileInfo {
                checked_at: None,
                integrity: kept_integrity.to_string(),
                mode: 0o644,
                size: Some(kept_contents.len() as u64),
            },
        )]),
    };
    store_dir.write_index_file(&tarball_integrity, &index).unwrap();

    eprintln!("Executing pacquet store prune...");
    pacquet.with_args(["store", "prune"]).assert().success();

    assert!(kept_path.exists());
    assert!(!orphan_path.exists());

    drop(root); // cleanup
}
