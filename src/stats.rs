use crate::config::Config;

use alpm::PackageReason;
use alpm_utils::DbListExt;

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use anyhow::Result;
use indicatif::HumanBytes;

struct Info<'a> {
    installed_packages: usize,
    foreign_packages: u32,
    explicit_packages: u32,
    total_size: i64,
    max_packages: Vec<(i64, &'a str)>,
}

fn collect_info(config: &Config, max_n: usize) -> Result<Info> {
    let db = config.alpm.localdb();
    let sync_db = config.alpm.syncdbs();

    let installed_packages = db.pkgs().len();
    let mut foreign_packages = 0;
    let mut explicit_packages = 0;
    let mut total_size = 0;

    let mut max_packages = BinaryHeap::with_capacity(max_n + 1);

    for pkg in db.pkgs() {
        max_packages.push(Reverse((pkg.isize(), pkg.name())));
        if max_packages.len() > 10 {
            max_packages.pop();
        }
        if let Err(alpm::Error::PkgNotFound) = sync_db.pkg(pkg.name()) {
            foreign_packages += 1;
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
        installed_packages,
        foreign_packages,
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

pub fn stats(config: &Config) -> Result<i32> {
    let c = config.color;
    let info = collect_info(config, 10)?;

    print_line_separator(config);

    println!(
        "Total installed packages: {}",
        c.stats_value.paint(info.installed_packages.to_string())
    );
    println!(
        "Total foreign installed packages: {}",
        c.stats_value.paint(info.foreign_packages.to_string())
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

    Ok(0)
}
