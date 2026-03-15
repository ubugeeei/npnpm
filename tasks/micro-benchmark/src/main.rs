use std::{fs, path::Path};

use clap::Parser;
use criterion::{Criterion, Throughput};
use mockito::ServerGuard;
use pacquet_lockfile::{Lockfile, PkgNameVerPeer};
use pacquet_network::ThrottledClient;
use pacquet_store_dir::StoreDir;
use pacquet_tarball::DownloadTarballToStore;
use pipe_trait::Pipe;
use project_root::get_project_root;
use ssri::Integrity;
use tempfile::tempdir;

#[derive(Debug, Parser)]
struct CliArgs {
    #[clap(long)]
    save_baseline: Option<String>,
}

fn bench_tarball(c: &mut Criterion, server: &mut ServerGuard, fixtures_folder: &Path) {
    let mut group = c.benchmark_group("tarball");
    let file = fs::read(fixtures_folder.join("@fastify+error-3.3.0.tgz")).unwrap();
    server.mock("GET", "/@fastify+error-3.3.0.tgz").with_status(201).with_body(&file).create();

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();

    let url = &format!("{0}/@fastify+error-3.3.0.tgz", server.url());
    let package_integrity: Integrity = "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w==".parse().expect("parse integrity string");

    group.throughput(Throughput::Bytes(file.len() as u64));
    group.bench_function("download_dependency", |b| {
        b.to_async(&rt).iter(|| async {
            // NOTE: the tempdir is being leaked, meaning the cleanup would be postponed until the end of the benchmark
            let dir = tempdir().unwrap();
            let store_dir =
                dir.path().to_path_buf().pipe(StoreDir::from).pipe(Box::new).pipe(Box::leak);
            let http_client = ThrottledClient::new_from_cpu_count();

            let cas_map = DownloadTarballToStore {
                http_client: &http_client,
                store_dir,
                package_integrity: &package_integrity,
                package_unpacked_size: Some(16697),
                package_url: url,
            }
            .run_without_mem_cache()
            .await
            .unwrap();
            cas_map.len()
        });
    });

    group.finish();
}

fn legacy_virtual_store_name(package_specifier: &PkgNameVerPeer) -> String {
    package_specifier
        .to_string()
        .replace('/', "+")
        .replace(")(", "_")
        .replace('(', "_")
        .replace(')', "")
}

fn bench_virtual_store_name(c: &mut Criterion, root: &Path) {
    let lockfile = Lockfile::load_from_dir(root.join("crates/testing-utils/src/fixtures/big"))
        .expect("load lockfile fixture")
        .expect("fixture lockfile should exist");
    let package_specifiers = lockfile
        .packages
        .expect("fixture lockfile should contain packages")
        .into_keys()
        .map(|dependency_path| dependency_path.package_specifier)
        .collect::<Vec<_>>();

    let mut group = c.benchmark_group("virtual_store_name");
    group.throughput(Throughput::Elements(package_specifiers.len() as u64));
    group.bench_function("legacy_big_lockfile", |b| {
        b.iter(|| {
            package_specifiers
                .iter()
                .map(|package_specifier| {
                    std::hint::black_box(legacy_virtual_store_name(package_specifier)).len()
                })
                .sum::<usize>()
        });
    });
    group.bench_function("optimized_big_lockfile", |b| {
        b.iter(|| {
            package_specifiers
                .iter()
                .map(|package_specifier| {
                    std::hint::black_box(package_specifier.to_virtual_store_name()).len()
                })
                .sum::<usize>()
        });
    });
    group.finish();
}

pub fn main() -> Result<(), String> {
    let mut server = mockito::Server::new();
    let CliArgs { save_baseline } = CliArgs::parse();
    let root = get_project_root().unwrap();
    let fixtures_folder = root.join("tasks/micro-benchmark/fixtures");

    let mut criterion = Criterion::default().without_plots();
    if let Some(baseline) = save_baseline {
        criterion = criterion.save_baseline(baseline);
    }

    bench_virtual_store_name(&mut criterion, &root);
    bench_tarball(&mut criterion, &mut server, &fixtures_folder);
    criterion.final_summary();

    Ok(())
}
