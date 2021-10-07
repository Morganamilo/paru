use crate::config::SortBy;
use crate::config::{Config, Mode, SortMode};
use crate::fmt::{color_repo, print_indent};
use crate::info;
use crate::install::install;
use crate::printtr;
use crate::util::{input, NumberMenu};

use ansi_term::Style;
use anyhow::{ensure, Context, Result};
use indicatif::HumanBytes;
use raur::{Raur, SearchBy};
use regex::RegexSet;
use reqwest::get;
use tr::tr;

enum AnyPkg<'a> {
    RepoPkg(alpm::Package<'a>),
    AurPkg(&'a raur::Package),
}

pub async fn search(config: &Config) -> Result<i32> {
    let quiet = config.args.has_arg("q", "quiet");
    let repo_pkgs = search_repos(config, &config.targets)?;

    let targets = config
        .targets
        .iter()
        .map(|t| t.to_lowercase())
        .collect::<Vec<_>>();

    let pkgs = search_aur(config, &targets)
        .await
        .context(tr!("aur search failed"))?;

    if config.sort_mode == SortMode::TopDown {
        for pkg in &repo_pkgs {
            print_alpm_pkg(config, pkg, quiet);
        }
        for pkg in &pkgs {
            print_pkg(config, pkg, quiet)
        }
    } else {
        for pkg in pkgs.iter().rev() {
            print_pkg(config, pkg, quiet)
        }
        for pkg in repo_pkgs.iter().rev() {
            print_alpm_pkg(config, pkg, quiet);
        }
    }

    Ok((repo_pkgs.is_empty() && pkgs.is_empty()) as i32)
}

fn search_repos<'a>(config: &'a Config, targets: &[String]) -> Result<Vec<alpm::Package<'a>>> {
    if targets.is_empty() || config.mode == Mode::Aur {
        return Ok(Vec::new());
    }

    let mut ret = Vec::new();

    for db in config.alpm.syncdbs() {
        let pkgs = db.search(targets.iter())?;
        ret.extend(pkgs);
    }

    if config.limit != 0 {
        ret.truncate(config.limit);
    }

    Ok(ret)
}

async fn search_target(config: &Config, targets: &mut Vec<String>) -> Result<Vec<raur::Package>> {
    let by = config.search_by;
    let mut pkgs = Ok(Vec::new());
    let mut index = 0;

    for (i, target) in targets.iter().enumerate() {
        index = i;
        pkgs = config.raur.search_by(target, by).await;
        if !matches!(pkgs, Err(raur::Error::Aur(_))) {
            break;
        }
    }

    if pkgs.is_ok() {
        targets.remove(index);
    }

    Ok(pkgs?)
}

async fn search_aur_regex(config: &Config, targets: &[String]) -> Result<Vec<raur::Package>> {
    let url = config.aur_url.join("packages.gz")?;
    let resp = get(url.clone())
        .await
        .with_context(|| format!("get {}", url))?;
    let success = resp.status().is_success();
    ensure!(success, "get {}: {}", url, resp.status());

    let data = resp.text().await?;

    let regex = RegexSet::new(targets)?;

    let pkgs = data
        .lines()
        .skip(1)
        .filter(|pkg| regex.is_match(pkg))
        .collect::<Vec<_>>();
    ensure!(pkgs.len() < 2000, "too many packages");
    let pkgs = config.raur.info(&pkgs).await?;
    Ok(pkgs)
}

async fn search_aur(config: &Config, targets: &[String]) -> Result<Vec<raur::Package>> {
    if targets.is_empty() || config.mode == Mode::Repo {
        return Ok(Vec::new());
    }

    let mut matches = if config.args.has_arg("x", "regex") {
        search_aur_regex(config, targets).await?
    } else {
        let mut targets = targets.iter().map(|t| t.to_lowercase()).collect::<Vec<_>>();
        targets.sort_by_key(|t| t.len());

        let mut matches = Vec::new();

        let by = config.search_by;

        if by == SearchBy::NameDesc {
            let pkgs = search_target(config, &mut targets).await?;
            matches.extend(pkgs);
            matches.retain(|p| {
                let name = p.name.to_lowercase();
                let description = p
                    .description
                    .as_ref()
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();
                targets
                    .iter()
                    .all(|t| name.contains(t) | description.contains(t))
            });
        } else if by == SearchBy::Name {
            let pkgs = search_target(config, &mut targets).await?;
            matches.extend(pkgs);
            matches.retain(|p| targets.iter().all(|t| p.name.to_lowercase().contains(t)));
        } else {
            for target in targets {
                let pkgs = config.raur.search_by(target, by).await?;
                matches.extend(pkgs);
            }
        }

        matches
    };

    match config.sort_by {
        SortBy::Votes => matches.sort_by(|a, b| b.num_votes.cmp(&a.num_votes)),
        SortBy::Popularity => {
            matches.sort_by(|a, b| b.popularity.partial_cmp(&a.popularity).unwrap())
        }
        SortBy::Id => matches.sort_by_key(|p| p.id),
        SortBy::Name => matches.sort_by(|a, b| a.name.cmp(&b.name)),
        SortBy::Base => matches.sort_by(|a, b| a.package_base.cmp(&b.package_base)),
        SortBy::Submitted => matches.sort_by_key(|p| p.first_submitted),
        SortBy::Modified => matches.sort_by_key(|p| p.last_modified),
        _ => (),
    }

    if config.limit != 0 {
        matches.truncate(config.limit);
    }

    Ok(matches)
}

fn print_pkg(config: &Config, pkg: &raur::Package, quiet: bool) {
    if quiet {
        println!("{}", pkg.name);
        return;
    }

    let c = config.color;
    let stats = format!("+{} ~{:.2}", pkg.num_votes, pkg.popularity);
    print!(
        "{}/{} {} [{}]",
        color_repo(c.enabled, "aur"),
        c.ss_name.paint(&pkg.name),
        c.ss_ver.paint(&pkg.version),
        c.ss_stats.paint(stats),
    );

    if let Some(date) = pkg.out_of_date {
        let date = tr!("[Out-of-date: {}]", crate::fmt::ymd(date));
        print!(" {}", c.ss_ood.paint(date));
    }

    if let Ok(repo_pkg) = config.alpm.localdb().pkg(&*pkg.name) {
        let installed = if repo_pkg.version().as_str() != pkg.version {
            tr!("[Installed: {}]", repo_pkg.version())
        } else {
            tr!("[Installed]")
        };

        print!(" {}", c.ss_installed.paint(installed));
    }

    if pkg.maintainer.is_none() {
        print!(" {}", c.ss_orphaned.paint(tr!("[Orphaned]")));
    }

    let none = tr!("None");
    print!("\n    ");
    let desc = pkg
        .description
        .as_deref()
        .unwrap_or(&none)
        .split_whitespace();
    print_indent(Style::new(), 4, 4, config.cols, " ", desc);

    if config.args.count("s", "search") > 1 {
        if let Some(ref url) = pkg.url {
            info::print(c, 14, config.cols, "    URL", url);
        }

        let aur_url = format!("{}packages/{}", config.aur_url, pkg.package_base);
        info::print(c, 14, config.cols, "    AUR URL", aur_url.as_str());
    }
}

fn print_alpm_pkg(config: &Config, pkg: &alpm::Package, quiet: bool) {
    if quiet {
        println!("{}", pkg.name());
        return;
    }

    let c = config.color;
    let stats = format!(
        "{} {}",
        HumanBytes(pkg.download_size() as u64),
        HumanBytes(pkg.isize() as u64)
    );
    let ver: &str = pkg.version().as_ref();
    print!(
        "{}/{} {} [{}]",
        color_repo(c.enabled, pkg.db().unwrap().name()),
        c.ss_name.paint(pkg.name()),
        c.ss_ver.paint(ver),
        c.ss_stats.paint(stats),
    );

    if let Ok(repo_pkg) = config.alpm.localdb().pkg(pkg.name()) {
        let installed = if repo_pkg.version() != pkg.version() {
            tr!("[Installed: {}]", repo_pkg.version())
        } else {
            tr!("[Installed]")
        };

        print!(" {}", c.ss_installed.paint(installed));
    }

    if !pkg.groups().is_empty() {
        print!(" {}", c.ss_orphaned.paint("("));
        print!("{}", c.ss_orphaned.paint(pkg.groups().first().unwrap()));
        for group in pkg.groups().iter().skip(1) {
            print!(" {}", c.ss_orphaned.paint(group));
        }
        print!("{}", c.ss_orphaned.paint(")"));
    }

    print!("\n    ");
    let desc = pkg.desc();
    let desc = desc.unwrap_or_default().split_whitespace();
    print_indent(Style::new(), 4, 4, config.cols, " ", desc);

    if config.args.count("s", "search") > 1 {
        if let Some(url) = pkg.url() {
            info::print(c, 14, config.cols, "    URL", url);
        }
    }
}

pub async fn search_install(config: &mut Config) -> Result<i32> {
    let repo_pkgs = search_repos(config, &config.targets)?;
    let aur_pkgs = search_aur(config, &config.targets).await?;
    let mut all_pkgs = Vec::new();
    let c = config.color;

    for pkg in repo_pkgs {
        all_pkgs.push(AnyPkg::RepoPkg(pkg));
    }
    for pkg in &aur_pkgs {
        all_pkgs.push(AnyPkg::AurPkg(pkg));
    }

    let pad = all_pkgs.len().to_string().len();

    if all_pkgs.is_empty() {
        printtr!("no packages match search");
        return Ok(1);
    }

    let indexes = all_pkgs
        .iter()
        .enumerate()
        .filter_map(|(n, pkg)| {
            let name = match pkg {
                AnyPkg::RepoPkg(pkg) => pkg.name(),
                AnyPkg::AurPkg(pkg) => pkg.name.as_str(),
            };

            if config.targets.iter().any(|targ| targ == name) {
                Some(n)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    for (i, n) in indexes.iter().rev().enumerate() {
        let pkg = all_pkgs.remove(i + n);
        all_pkgs.insert(0, pkg);
    }

    if config.sort_mode == SortMode::TopDown {
        for (n, pkg) in all_pkgs.iter().enumerate() {
            match pkg {
                AnyPkg::RepoPkg(pkg) => {
                    let n = format!("{:>pad$}", n + 1, pad = pad);
                    print!("{} ", c.number_menu.paint(n));
                    print_alpm_pkg(config, pkg, false)
                }
                AnyPkg::AurPkg(pkg) => {
                    let n = format!("{:>pad$}", n + 1, pad = pad);
                    print!("{} ", c.number_menu.paint(n));
                    print_pkg(config, pkg, false)
                }
            };
        }
    } else {
        for (n, pkg) in all_pkgs.iter().enumerate().rev() {
            match pkg {
                AnyPkg::RepoPkg(pkg) => {
                    let n = format!("{:>pad$}", n + 1, pad = pad);
                    print!("{} ", c.number_menu.paint(n));
                    print_alpm_pkg(config, pkg, false)
                }
                AnyPkg::AurPkg(pkg) => {
                    let n = format!("{:>pad$}", n + 1, pad = pad);
                    print!("{} ", c.number_menu.paint(n));
                    print_pkg(config, pkg, false)
                }
            };
        }
    }

    let input = input(config, &tr!("Packages to install (eg: 1 2 3, 1-3):"));

    if input.trim().is_empty() {
        printtr!(" there is nothing to do");
        return Ok(1);
    }

    let menu = NumberMenu::new(&input);
    let mut pkgs = Vec::new();

    if config.sort_mode == SortMode::TopDown {
        for (n, pkg) in all_pkgs.iter().enumerate() {
            if menu.contains(n + 1, "") {
                match pkg {
                    AnyPkg::RepoPkg(pkg) => {
                        pkgs.push(format!("{}/{}", pkg.db().unwrap().name(), pkg.name()))
                    }
                    AnyPkg::AurPkg(pkg) => {
                        pkgs.push(format!("{}/{}", config.aur_namespace(), pkg.name))
                    }
                }
            }
        }
    } else {
        for (n, pkg) in all_pkgs.iter().enumerate().rev() {
            if menu.contains(n + 1, "") {
                match pkg {
                    AnyPkg::RepoPkg(pkg) => {
                        pkgs.push(format!("{}/{}", pkg.db().unwrap().name(), pkg.name()))
                    }
                    AnyPkg::AurPkg(pkg) => {
                        pkgs.push(format!("{}/{}", config.aur_namespace(), pkg.name))
                    }
                }
            }
        }
    }

    if pkgs.is_empty() {
        printtr!(" there is nothing to do")
    } else {
        config.need_root = true;
        install(config, &pkgs).await?;
    }

    Ok(0)
}
