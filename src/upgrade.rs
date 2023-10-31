use crate::config::{Config, LocalRepos};
use crate::devel::{filter_devel_updates, possible_devel_updates};
use crate::fmt::color_repo;
use crate::util::{input, NumberMenu};
use crate::{repo, RaurHandle};

use std::collections::{HashMap, HashSet};

use alpm::{AlpmList, Db};
use alpm_utils::DbListExt;
use anyhow::Result;
use aur_depends::{Resolver, Updates};
use futures::try_join;
use tr::tr;

#[derive(Default, Debug)]
pub struct Upgrades {
    pub aur_repos: HashMap<String, String>,
    pub pkgbuild_keep: Vec<(String, String)>,
    pub pkgbuild_skip: Vec<(String, String)>,
    pub repo_keep: Vec<String>,
    pub repo_skip: Vec<String>,
    pub aur_keep: Vec<String>,
    pub aur_skip: Vec<String>,
    pub devel: HashSet<String>,
}

pub fn repo_upgrades(config: &Config) -> Result<Vec<alpm::Package>> {
    let flags = alpm::TransFlag::NO_LOCK;
    config.alpm.trans_init(flags)?;
    config
        .alpm
        .sync_sysupgrade(config.args.count("u", "sysupgrade") > 1)?;

    let mut pkgs = config.alpm.trans_add().iter().collect::<Vec<_>>();
    let (dbs, _) = repo::repo_aur_dbs(config);

    pkgs.retain(|p| dbs.iter().any(|db| db.name() == p.db().unwrap().name()));

    pkgs.sort_by(|a, b| {
        dbs.iter()
            .position(|db| db.name() == a.db().unwrap().name())
            .unwrap()
            .cmp(
                &dbs.iter()
                    .position(|db| db.name() == b.db().unwrap().name())
                    .unwrap(),
            )
            .then(a.name().cmp(b.name()))
    });
    //config.alpm.trans_release();
    Ok(pkgs)
}

fn get_version_diff(config: &Config, old: &str, new: &str) -> (String, String) {
    let mut old_iter = old.chars();
    let mut new_iter = new.chars();
    let mut old_split = old_iter.clone();
    let old_col = config.color.old_version;
    let new_col = config.color.new_version;

    while let Some(old_c) = old_iter.next() {
        let new_c = match new_iter.next() {
            Some(c) => c,
            None => break,
        };

        if old_c != new_c {
            break;
        }

        if !old_c.is_alphanumeric() {
            old_split = old_iter.clone();
        }
    }

    let common = old.len() - old_split.as_str().len();

    (
        format!("{}{}", &old[..common], old_col.paint(&old[common..])),
        format!("{}{}", &new[..common], new_col.paint(&new[common..])),
    )
}

#[allow(clippy::too_many_arguments)]
fn print_upgrade(
    config: &Config,
    n: usize,
    n_max: usize,
    pkg: &str,
    db: &str,
    db_pkg_max: usize,
    old: &str,
    old_max: usize,
    new: &str,
) {
    let c = config.color;
    let n = format!("{:>pad$}", n, pad = n_max);
    let db_pkg = format!(
        "{}/{}{:pad$}",
        color_repo(config.color.enabled, db),
        c.bold.paint(pkg),
        "",
        pad = db_pkg_max - (db.len() + pkg.len()) + 1
    );
    let old = format!("{:<pad$}", old, pad = old_max);
    let (old, new) = get_version_diff(config, &old, new);
    println!(
        "{} {} {} -> {}",
        c.number_menu.paint(n),
        c.bold.paint(db_pkg),
        old,
        new
    );
}

async fn get_resolver_upgrades<'a, 'b>(
    config: &Config,
    resolver: &mut Resolver<'a, 'b, RaurHandle>,
    print: bool,
) -> Result<Updates<'a>> {
    if print {
        if config.mode.pkgbuild() {
            let c = config.color;
            println!(
                "{} {}",
                c.action.paint("::"),
                c.bold.paint(tr!("Looking for PKGBUILD upgrades..."))
            );

            if config.mode.aur() {
                let c = config.color;
                println!(
                    "{} {}",
                    c.action.paint("::"),
                    c.bold.paint(tr!("Looking for AUR upgrades..."))
                );
            }
        }

        let dbs = match config.repos {
            LocalRepos::None => None,
            _ => {
                let (_, dbs) = repo::repo_aur_dbs(config);
                let dbs = Some(dbs.into_iter().map(|db| db.name()).collect::<Vec<_>>());
                dbs
            }
        };
        let updates = resolver.updates(dbs.as_deref()).await?;

        Ok(updates)
    } else {
        Ok(Updates::default())
    }
}

async fn get_devel_upgrades(config: &Config, print: bool) -> Result<Vec<String>> {
    if !config.devel || (!config.mode.aur() && !config.mode.pkgbuild()) {
        return Ok(Vec::new());
    }

    let c = config.color;
    if print {
        println!(
            "{} {}",
            c.action.paint("::"),
            c.bold.paint(tr!("Looking for devel upgrades..."))
        );
    }

    possible_devel_updates(config).await
}

pub async fn net_upgrades<'res, 'conf>(
    config: &'conf Config,
    resolver: &mut Resolver<'res, '_, RaurHandle>,
    print: bool,
) -> Result<(Updates<'res>, Vec<String>)> {
    try_join!(
        get_resolver_upgrades(config, resolver, print),
        get_devel_upgrades(config, print)
    )
}

pub async fn get_upgrades<'a, 'b>(
    config: &Config,
    resolver: &mut Resolver<'a, 'b, RaurHandle>,
) -> Result<Upgrades> {
    let (upgrades, devel_upgrades) = net_upgrades(config, resolver, true).await?;
    let (syncdbs, aurdbs) = repo::repo_aur_dbs(config);

    for pkg in upgrades.aur_ignored {
        eprintln!(
            "{} {}",
            config.color.warning.paint(tr!("warning:")),
            tr!(
                "{pkg}: ignoring package upgrade ({old} => {new})",
                pkg = pkg.local.name(),
                old = pkg.local.version(),
                new = pkg.remote.version
            )
        );
    }

    for pkg in upgrades.pkgbuild_ignored {
        eprintln!(
            "{} {}",
            config.color.warning.paint(tr!("warning:")),
            tr!(
                "{pkg}: ignoring package upgrade ({old} => {new})",
                pkg = pkg.local.name(),
                old = pkg.local.version(),
                new = pkg.remote_srcinfo.version(),
            )
        );
    }

    let mut aur_upgrades = upgrades.aur_updates;
    let pkgbuild_upgrades = upgrades.pkgbuild_updates;
    let mut devel_upgrades =
        filter_devel_updates(config, resolver.get_cache_mut(), &devel_upgrades).await?;

    let repo_upgrades = if config.mode.repo() && config.combined_upgrade {
        repo_upgrades(config)?
    } else {
        Vec::new()
    };

    devel_upgrades.sort();
    devel_upgrades.dedup();
    // TODO better devel pkgbuild
    aur_upgrades.retain(|u| !devel_upgrades.iter().any(|t| t.pkg == u.remote.name));

    let mut repo_skip = Vec::new();
    let mut repo_keep = Vec::new();
    let mut aur_skip = Vec::new();
    let mut aur_keep = Vec::new();
    let mut custom_keep = Vec::new();
    let mut custom_skip = Vec::new();

    let mut aur_repos = HashMap::new();
    for pkg in &aur_upgrades {
        if let Some(db) = pkg.local.db() {
            aur_repos.insert(pkg.local.name().to_string(), db.name().to_string());
        }
    }

    if devel_upgrades.is_empty()
        && aur_upgrades.is_empty()
        && repo_upgrades.is_empty()
        && pkgbuild_upgrades.is_empty()
    {
        return Ok(Upgrades::default());
    }

    if !config.upgrade_menu {
        let mut aur = aur_upgrades
            .iter()
            .map(|p| p.remote.name.clone())
            .collect::<Vec<_>>();

        let mut pkgbuild_updates = pkgbuild_upgrades
            .iter()
            .map(|u| (u.repo.clone(), u.local.name().to_string()))
            .collect::<Vec<_>>();

        for devel in &devel_upgrades {
            if devel.repo.as_deref() == Some(config.aur_namespace()) {
                aur.push(devel.pkg.clone());
            } else {
                pkgbuild_updates.push((devel.repo.clone().unwrap(), devel.pkg.clone()));
            }
        }

        let upgrades = Upgrades {
            pkgbuild_keep: pkgbuild_updates,
            pkgbuild_skip: custom_skip,
            aur_repos,
            repo_keep: repo_upgrades.iter().map(|p| p.name().to_string()).collect(),
            aur_keep: aur,
            aur_skip,
            repo_skip,
            devel: devel_upgrades.into_iter().map(|t| t.pkg).collect(),
        };
        return Ok(upgrades);
    }

    let db = config.alpm.localdb();
    let n_max = repo_upgrades.len() + aur_upgrades.len() + devel_upgrades.len();
    let n_max = n_max.to_string().len();
    let mut index =
        repo_upgrades.len() + aur_upgrades.len() + devel_upgrades.len() + pkgbuild_upgrades.len();

    let db_pkg_max = repo_upgrades
        .iter()
        .map(|u| u.name().len() + u.db().unwrap().name().len())
        .chain(
            aur_upgrades
                .iter()
                .map(|u| db_len(u.local.name(), "aur", &aurdbs)),
        )
        .chain(
            devel_upgrades
                .iter()
                .map(|u| db_len(&u.pkg, "devel", &aurdbs)),
        )
        .chain(
            pkgbuild_upgrades
                .iter()
                .map(|u| db_len(u.local.name(), &u.repo, &aurdbs)),
        )
        .max()
        .unwrap_or(0);

    let old_max = repo_upgrades
        .iter()
        .map(|p| db.pkg(p.name()).unwrap().version().as_str().len())
        .chain(aur_upgrades.iter().map(|p| p.local.version().len()))
        .chain(
            devel_upgrades
                .iter()
                .filter_map(|p| db.pkg(p.pkg.as_str()).ok())
                .map(|p| p.version().len()),
        )
        .chain(pkgbuild_upgrades.iter().map(|p| p.local.version().len()))
        .max()
        .unwrap_or(0);

    for pkg in repo_upgrades.iter().rev().rev() {
        let local_pkg = config.alpm.localdb().pkg(pkg.name())?;
        print_upgrade(
            config,
            index,
            n_max,
            pkg.name(),
            pkg.db().unwrap().name(),
            db_pkg_max,
            local_pkg.version(),
            old_max,
            pkg.version(),
        );
        index -= 1;
    }

    for pkg in aur_upgrades.iter().rev().rev() {
        let remote = aurdbs
            .pkg(pkg.local.name())
            .map(|p| format!("{}-aur", p.db().unwrap().name()));
        let remote = remote.as_deref().unwrap_or("aur");
        print_upgrade(
            config,
            index,
            n_max,
            pkg.local.name(),
            remote,
            db_pkg_max,
            pkg.local.version(),
            old_max,
            &pkg.remote.version,
        );
        index -= 1;
    }

    for pkg in devel_upgrades.iter().rev().rev() {
        let pkg = pkg.pkg.as_str();
        let remote = aurdbs
            .pkg(pkg)
            .map(|p| p.db().unwrap().name())
            .map(|p| format!("{}-devel", p));
        let remote = remote.as_deref().unwrap_or("devel");
        let current = aurdbs.pkg(pkg).or_else(|_| db.pkg(pkg)).unwrap();
        let ver = current.version();
        print_upgrade(
            config,
            index,
            n_max,
            pkg,
            remote,
            db_pkg_max,
            ver,
            old_max,
            "latest-commit",
        );
        index -= 1;
    }

    for pkg in pkgbuild_upgrades.iter().rev().rev() {
        let remote = aurdbs
            .pkg(pkg.local.name())
            .map(|p| format!("{}-{}", p.db().unwrap().name(), pkg.repo));
        let remote = remote.as_deref().unwrap_or("aur");
        print_upgrade(
            config,
            index,
            n_max,
            pkg.local.name(),
            remote,
            db_pkg_max,
            pkg.local.version(),
            old_max,
            &pkg.remote_srcinfo.version(),
        );
        index -= 1;
    }

    let input = input(config, &tr!("Packages to exclude (eg: 1 2 3, 1-3):"));
    let input = input.trim();
    let number_menu = NumberMenu::new(input);
    let mut index =
        repo_upgrades.len() + aur_upgrades.len() + devel_upgrades.len() + pkgbuild_upgrades.len();

    for pkg in repo_upgrades.iter().rev().rev() {
        let remote = syncdbs.pkg(pkg.name()).unwrap();
        let db = remote.db().unwrap();
        if !number_menu.contains(index, db.name()) || input.is_empty() {
            repo_keep.push(pkg.name().to_string());
        } else {
            repo_skip.push(pkg.name().to_string());
        }
        index -= 1;
    }

    for pkg in aur_upgrades.iter().rev().rev() {
        let remote = aurdbs
            .pkg(pkg.local.name())
            .map(|p| p.db().unwrap().name())
            .unwrap_or("aur");
        if !number_menu.contains(index, remote) || input.is_empty() {
            aur_keep.push(pkg.local.name().to_string());
        } else {
            aur_skip.push(pkg.local.name().to_string());
        }
        index -= 1;
    }

    //TODO
    for pkg in devel_upgrades.iter().rev().rev() {
        let remote = aurdbs
            .pkg(pkg.pkg.as_str())
            .map(|p| p.db().unwrap().name())
            .map(|p| format!("{}-devel", p));
        let remote = remote.as_deref().unwrap_or("devel");
        let keep = !number_menu.contains(index, &format!("{}-devel", remote)) || input.is_empty();
        let is_aur = pkg.repo.as_deref() == Some(config.aur_namespace());

        match (keep, is_aur) {
            (true, true) => aur_keep.push(pkg.pkg.to_string()),
            (false, true) => aur_skip.push(pkg.pkg.to_string()),
            (true, false) => custom_keep.push((pkg.repo.clone().unwrap(), pkg.pkg.clone())),
            (false, false) => custom_skip.push((pkg.repo.clone().unwrap(), pkg.pkg.clone())),
        }

        index -= 1;
    }

    for pkg in pkgbuild_upgrades.iter().rev().rev() {
        let remote = aurdbs
            .pkg(pkg.local.name())
            .map(|p| p.db().unwrap().name())
            .unwrap_or(&pkg.repo);
        if !number_menu.contains(index, remote) || input.is_empty() {
            custom_keep.push((pkg.repo.clone(), pkg.local.name().to_string()));
        } else {
            custom_skip.push((pkg.repo.clone(), pkg.local.name().to_string()));
        }
        index -= 1;
    }

    let upgrades = Upgrades {
        pkgbuild_keep: custom_keep,
        pkgbuild_skip: custom_skip,
        aur_repos,
        repo_keep,
        repo_skip,
        aur_keep,
        aur_skip,
        devel: devel_upgrades.into_iter().map(|t| t.pkg).collect(),
    };

    Ok(upgrades)
}

fn db_len(name: &str, repo_name: &str, aurdbs: &AlpmList<Db>) -> usize {
    name.len()
        + aurdbs
            .pkg(name)
            .ok()
            .and_then(|pkg| pkg.db())
            .map(|db| db.name().len() + repo_name.len() + 1)
            .unwrap_or(repo_name.len())
}
