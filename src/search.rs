use crate::config::Config;
use crate::fmt::{color_repo, print_indent};
use crate::install::install;
use crate::util::{input, NumberMenu};
use crate::{sprint, sprintln};

use ansi_term::Style;
use anyhow::{Context, Result};
use indicatif::HumanBytes;
use raur::{Raur, SearchBy};

pub fn search(config: &Config) -> Result<i32> {
    let quiet = config.args.has_arg("q", "quiet");
    let repo_pkgs = search_repos(config, &config.targets)?;
    for pkg in &repo_pkgs {
        print_alpm_pkg(config, pkg, quiet);
    }

    let targets = config
        .targets
        .iter()
        .map(|t| t.to_lowercase())
        .collect::<Vec<_>>();

    let pkgs = search_aur(config, &targets).context("aur search failed")?;
    for pkg in &pkgs {
        print_pkg(config, pkg, quiet)
    }
    Ok((repo_pkgs.is_empty() && pkgs.is_empty()) as i32)
}

fn search_repos<'a>(config: &'a Config, targets: &[String]) -> Result<Vec<alpm::Package<'a>>> {
    if targets.is_empty() || config.mode == "aur" {
        return Ok(Vec::new());
    }

    let mut ret = Vec::new();

    for db in config.alpm.syncdbs() {
        let pkgs = db.search(targets.iter())?;
        ret.extend(pkgs);
    }

    Ok(ret)
}

fn search_aur(config: &Config, targets: &[String]) -> Result<Vec<raur::Package>> {
    if targets.is_empty() || config.mode == "repo" {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();

    let by = match config.search_by.as_str() {
        "name" => SearchBy::Name,
        "maintainer" => SearchBy::Maintainer,
        "depends" => SearchBy::Depends,
        "makedepends" => SearchBy::MakeDepends,
        "checkdepends" => SearchBy::CheckDepends,
        "optdepends" => SearchBy::OptDepends,
        _ => SearchBy::NameDesc,
    };

    if by == SearchBy::NameDesc {
        let target = targets.iter().max_by_key(|t| t.len()).unwrap();
        let pkgs = config.raur.search_by(target, by)?;
        matches.extend(pkgs);
        matches.retain(|p| {
            targets.iter().all(|t| {
                p.name.contains(t) | p.description.as_ref().unwrap_or(&String::new()).contains(t)
            })
        });
    } else if by == SearchBy::Name {
        let target = targets.iter().max_by_key(|t| t.len()).unwrap();
        let pkgs = config.raur.search_by(target, by)?;
        matches.extend(pkgs);
        matches.retain(|p| targets.iter().all(|t| p.name.contains(t)));
    } else {
        for target in targets {
            let pkgs = config.raur.search_by(target, by)?;
            matches.extend(pkgs);
        }
    }

    match config.sort_by.as_str() {
        "votes" => matches.sort_by(|a, b| b.num_votes.cmp(&a.num_votes)),
        "popularity" => matches.sort_by(|a, b| b.popularity.partial_cmp(&a.popularity).unwrap()),
        "id" => matches.sort_by_key(|p| p.id),
        "name" => matches.sort_by(|a, b| a.name.cmp(&b.name)),
        "base" => matches.sort_by(|a, b| a.package_base.cmp(&b.package_base)),
        "submitted" => matches.sort_by_key(|p| p.first_submitted),
        "modified" => matches.sort_by_key(|p| p.last_modified),
        _ => (),
    }

    Ok(matches)
}

fn print_pkg(config: &Config, pkg: &raur::Package, quiet: bool) {
    if quiet {
        sprintln!("{}", pkg.name);
        return;
    }

    let c = config.color;
    let stats = format!("+{} ~{:.2}", pkg.num_votes, pkg.popularity);
    sprint!(
        "{}/{} {} [{}]",
        color_repo(c.enabled, "aur"),
        c.ss_name.paint(&pkg.name),
        c.ss_ver.paint(&pkg.version),
        c.ss_stats.paint(stats),
    );

    if let Ok(repo_pkg) = config.alpm.localdb().pkg(&pkg.name) {
        let installed = if repo_pkg.version().as_ref() != pkg.version {
            format!("[Installed: {}]", repo_pkg.version())
        } else {
            "[Installed]".to_string()
        };

        sprint!(" {}", c.ss_installed.paint(installed));
    }

    if pkg.maintainer.is_none() {
        sprint!(" {}", c.ss_orphaned.paint("[Orphaned]"));
    }

    sprint!("\n    ");
    let desc = pkg
        .description
        .as_deref()
        .unwrap_or_default()
        .split_whitespace();
    print_indent(Style::new(), 4, 4, config.cols, " ", desc);
}

fn print_alpm_pkg(config: &Config, pkg: &alpm::Package, quiet: bool) {
    if quiet {
        sprintln!("{}", pkg.name());
        return;
    }

    let c = config.color;
    let stats = format!(
        "{} {}",
        HumanBytes(pkg.download_size() as u64),
        HumanBytes(pkg.isize() as u64)
    );
    let ver: &str = pkg.version().as_ref();
    sprint!(
        "{}/{} {} [{}]",
        color_repo(c.enabled, pkg.db().unwrap().name()),
        c.ss_name.paint(pkg.name()),
        c.ss_ver.paint(ver),
        c.ss_stats.paint(stats),
    );

    if let Ok(repo_pkg) = config.alpm.localdb().pkg(pkg.name()) {
        let installed = if repo_pkg.version() != pkg.version() {
            format!("[Installed: {}]", repo_pkg.version())
        } else {
            "[Installed]".to_string()
        };

        sprint!(" {}", c.ss_installed.paint(installed));
    }

    if !pkg.groups().is_empty() {
        for group in pkg.groups() {
            sprint!(" {}", c.ss_orphaned.paint(" ("));
            sprint!(" {}", c.ss_orphaned.paint(group));
            sprint!(" {}", c.ss_orphaned.paint(")"));
        }
    }

    sprint!("\n    ");
    let desc = pkg.desc();
    let desc = desc.as_deref().unwrap_or_default().split_whitespace();
    print_indent(Style::new(), 4, 4, config.cols, " ", desc);
}

pub fn search_install(config: &mut Config) -> Result<i32> {
    let repo_pkgs = search_repos(config, &config.targets)?;
    let aur_pkgs = search_aur(config, &config.targets)?;
    let len = repo_pkgs.len() + aur_pkgs.len();
    let pad = len.to_string().len();
    let c = config.color;

    if len == 0 {
        sprintln!("no packages match search");
        return Ok(1);
    }

    if config.sort_mode == "topdown" {
        for (n, pkg) in repo_pkgs.iter().enumerate() {
            let n = format!("{:>pad$}", n + 1, pad = pad);
            sprint!("{} ", c.number_menu.paint(n));
            print_alpm_pkg(config, pkg, false)
        }

        for (n, pkg) in aur_pkgs.iter().enumerate() {
            let n = format!("{:>pad$}", n + repo_pkgs.len() + 1, pad = pad);
            sprint!("{}{} ", "", c.number_menu.paint(n));
            print_pkg(config, pkg, false)
        }
    } else {
        for (n, pkg) in aur_pkgs.iter().rev().enumerate() {
            let n = format!("{:>pad$}", len - n, pad = pad);
            sprint!("{} ", c.number_menu.paint(n));
            print_pkg(config, pkg, false)
        }

        for (n, pkg) in repo_pkgs.iter().rev().enumerate() {
            let n = format!("{:>pad$}", len - (n + aur_pkgs.len()), pad = pad);
            sprint!("{}{} ", "", c.number_menu.paint(n));
            print_alpm_pkg(config, pkg, false)
        }
    }

    let input = input(config, "Packages to install (eg: 1 2 3, 1-3 or ^4): ");
    let menu = NumberMenu::new(&input);
    let mut pkgs = Vec::new();

    if config.sort_mode == "topdown" {
        for (n, pkg) in repo_pkgs.iter().enumerate() {
            if menu.contains(n + 1, "") {
                pkgs.push(pkg.name().to_string())
            }
        }
        for (n, pkg) in aur_pkgs.iter().enumerate() {
            if menu.contains(n + repo_pkgs.len() + 1, "") {
                pkgs.push(pkg.name.clone())
            }
        }
    } else {
        for (n, pkg) in aur_pkgs.iter().rev().enumerate() {
            if menu.contains(len - n, "") {
                pkgs.push(pkg.name.clone())
            }
        }

        for (n, pkg) in repo_pkgs.iter().rev().enumerate() {
            if menu.contains(len - (n + aur_pkgs.len()), "") {
                pkgs.push(pkg.name().to_string())
            }
        }
    }

    if pkgs.is_empty() {
        sprintln!(" there is nothing to do")
    } else {
        config.need_root = true;
        install(config, &pkgs)?;
    }

    Ok(0)
}
