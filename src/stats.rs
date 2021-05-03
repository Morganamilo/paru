use crate::config::{version, Config};
use crate::download::cache_info_with_warnings;
use crate::util::repo_aur_pkgs;

use alpm::PackageReason;
use alpm_utils::DbListExt;

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use anyhow::Result;
use indicatif::HumanBytes;

struct Info<'a> {
    total_packages: usize,
    repo_packages: usize,
    aur_packages: usize,
    explicit_packages: usize,
    total_size: i64,
    max_packages: Vec<(i64, &'a str)>,
}

async fn collect_info<'a>(config: &'a Config, max_n: usize) -> Result<Info<'a>> {
    let db = config.alpm.localdb();
    let sync_db = config.alpm.syncdbs();

    let total_packages = db.pkgs().len();
    let mut foreign_packages = Vec::new();
    let mut explicit_packages = 0;
    let mut total_size = 0;

    let mut max_packages = BinaryHeap::with_capacity(max_n + 1);

    for pkg in db.pkgs() {
        max_packages.push(Reverse((pkg.isize(), pkg.name())));
        if max_packages.len() > 10 {
            max_packages.pop();
        }
        if let Err(alpm::Error::PkgNotFound) = sync_db.pkg(pkg.name()) {
            foreign_packages.push(pkg.name());
        }
        if pkg.reason() == PackageReason::Explicit {
            explicit_packages += 1;
        }
        total_size += pkg.isize();
    }

    let (repo, aur) = repo_aur_pkgs(config);

    let max_packages = max_packages
        .into_sorted_vec()
        .into_iter()
        .map(|r| r.0)
        .collect();

    Ok(Info {
        total_packages,
        repo_packages: repo.len(),
        aur_packages: aur.len(),
        explicit_packages,
        total_size,
        max_packages,
    })
}

fn print_line_separator(config: &Config) {
    println!(
        "{}",
        config
            .color
            .stats_line_separator
            .paint("===========================================")
    );
}

pub async fn stats(config: &mut Config) -> Result<i32> {
    let c = config.color;
    let info = collect_info(config, 10).await?;
    version();
    print_line_separator(config);

    println!(
        "Total installed packages: {}",
        c.stats_value.paint(info.total_packages.to_string())
    );
    println!(
        "Aur packages: {}",
        c.stats_value.paint(info.aur_packages.to_string())
    );
    println!(
        "Repo packages: {}",
        c.stats_value.paint(info.repo_packages.to_string())
    );
    println!(
        "Explicitly installed packages: {}",
        c.stats_value.paint(info.explicit_packages.to_string())
    );
    println!(
        "Total Size occupied by packages: {}",
        c.stats_value
            .paint(HumanBytes(info.total_size as u64).to_string())
    );

    print_line_separator(config);

    println!("{}", c.bold.paint("Ten biggest packages:"));
    for (size, name) in info.max_packages {
        println!(
            "{}: {}",
            c.bold.paint(name),
            c.stats_value.paint(HumanBytes(size as u64).to_string())
        );
    }

    print_line_separator(config);

    let aur_packages = repo_aur_pkgs(config)
        .1
        .iter()
        .map(|pkg| pkg.name())
        .map(|s| s.to_owned())
        .collect::<Vec<_>>();

    let warnings = cache_info_with_warnings(
        &config.raur,
        &mut config.cache,
        &aur_packages,
        &config.ignore,
    )
    .await?;

    warnings.all(config.color, config.cols);

    Ok(0)
}
