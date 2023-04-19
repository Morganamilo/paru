use crate::config::{version, Config};
use crate::download::cache_info_with_warnings;
use crate::printtr;
use crate::util::repo_aur_pkgs;

use alpm::PackageReason;
use globset::GlobSet;

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use anyhow::Result;
use indicatif::HumanBytes;
use tr::tr;

struct Info<'a> {
    total_packages: usize,
    explicit_packages: usize,
    total_size: i64,
    max_packages: Vec<(i64, &'a str)>,
}

async fn collect_info(config: &Config, max_n: usize) -> Result<Info<'_>> {
    let db = config.alpm.localdb();
    let total_packages = db.pkgs().len();

    let mut explicit_packages = 0;
    let mut total_size = 0;
    let mut max_packages = BinaryHeap::with_capacity(max_n + 1);

    for pkg in db.pkgs() {
        max_packages.push(Reverse((pkg.isize(), pkg.name())));
        if max_packages.len() > 10 {
            max_packages.pop();
        }
        if pkg.reason() == PackageReason::Explicit {
            explicit_packages += 1;
        }
        total_size += pkg.isize();
    }

    let max_packages = max_packages
        .into_sorted_vec()
        .into_iter()
        .map(|r| r.0)
        .collect();

    Ok(Info {
        total_packages,
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

pub async fn stats(config: &Config) -> Result<i32> {
    let mut cache = raur::Cache::new();
    let c = config.color;
    let info = collect_info(config, 10).await?;
    let (repo, possible_aur) = repo_aur_pkgs(config);
    let aur_packages = possible_aur
        .iter()
        .map(|pkg| pkg.name())
        .map(|s| s.to_owned())
        .collect::<Vec<_>>();

    let warnings = cache_info_with_warnings(
        &config.raur,
        &mut cache,
        &aur_packages,
        &config.ignore,
        &GlobSet::empty(),
    )
    .await?;

    version();
    print_line_separator(config);

    printtr!(
        "Total installed packages: {}",
        c.stats_value.paint(info.total_packages.to_string())
    );
    printtr!(
        "Aur packages: {}",
        c.stats_value.paint(warnings.pkgs.len().to_string())
    );
    printtr!(
        "Repo packages: {}",
        c.stats_value.paint(repo.len().to_string())
    );
    printtr!(
        "Explicitly installed packages: {}",
        c.stats_value.paint(info.explicit_packages.to_string())
    );
    printtr!(
        "Total Size occupied by packages: {}",
        c.stats_value
            .paint(HumanBytes(info.total_size as u64).to_string())
    );

    print_line_separator(config);

    println!("{}", c.bold.paint(tr!("Ten biggest packages:")));
    for (size, name) in info.max_packages {
        println!(
            "{}: {}",
            c.bold.paint(name),
            c.stats_value.paint(HumanBytes(size as u64).to_string())
        );
    }

    print_line_separator(config);
    warnings.all(config.color, config.cols);

    Ok(0)
}
