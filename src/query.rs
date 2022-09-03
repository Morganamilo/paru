use std::collections::{HashMap, HashSet};

use crate::config::{Config, Mode};
use crate::devel::{filter_devel_updates, possible_devel_updates};
use crate::exec;
use crate::install::read_repos;
use crate::util::split_repo_aur_pkgs;

use anyhow::Result;
use futures::try_join;
use raur::{Cache, Raur};
use tr::tr;

pub async fn print_upgrade_list(config: &mut Config) -> Result<i32> {
    let mut cache = HashSet::new();
    let db = config.alpm.localdb();
    let args = &config.args;

    if args.has_arg("n", "native") {
        config.mode = Mode::Repo;
    } else if args.has_arg("m", "foreign") {
        config.mode = Mode::Aur;
    }

    let targets: Vec<_> = if config.targets.is_empty() {
        db.pkgs().iter().map(|p| p.name()).collect::<Vec<_>>()
    } else {
        config.targets.iter().map(|s| s.as_str()).collect()
    };

    let (repo, aur) = split_repo_aur_pkgs(config, &targets);

    let mut repo_ret = 1;
    let mut aur_ret = 1;

    if !repo.is_empty() && config.mode != Mode::Aur {
        let mut args = config.pacman_args();
        args.targets = repo.into_iter().collect();
        repo_ret = exec::pacman(config, &args)?.code();
    }

    if !aur.is_empty() && config.mode != Mode::Repo {
        let error = config.color.error;

        for &pkg in &aur {
            if db.pkg(pkg).is_err() {
                eprintln!(
                    "{} {}",
                    error.paint(tr!("error:")),
                    tr!("package '{}' was not found", pkg)
                );
            }
        }

        let mut args = config.pacman_args();
        args.remove("u").remove("upgrades").arg("q");
        args.targets = aur.into_iter().collect();
        let output = exec::pacman_output(config, &args)?;
        let aur = String::from_utf8(output.stdout)?;

        let aur = aur.trim().lines().collect::<Vec<_>>();

        async fn devel_up(config: &Config) -> Result<Vec<String>> {
            if config.devel {
                let updates = possible_devel_updates(config).await?;
                Ok(updates)
            } else {
                Ok(Vec::new())
            }
        }

        async fn aur_up(config: &Config, cache: &mut Cache, pkgs: &[&str]) -> Result<()> {
            config.raur.cache_info(cache, pkgs).await?;
            Ok(())
        }

        let (_, devel) = try_join!(aur_up(config, &mut cache, &aur), devel_up(config))?;
        let devel = filter_devel_updates(config, &mut cache, &devel).await?;

        let mut repo_paths = HashMap::new();
        let mut repos = Vec::new();

        read_repos(config, &mut repo_paths, &mut repos)?;

        for target in aur {
            let local_pkg = db.pkg(target).unwrap();

            'a: for repo in &repos {
                for pkg in &repo.pkgs {
                    if let Some(name) = pkg.names().find(|n| n == &target) && alpm::Version::new(&*pkg.version()) > local_pkg.version() {
                        print_upgrade(config, name, local_pkg.version().as_str(), &pkg.version());
                        continue 'a;
                    }
                }
            }

            if let Some(pkg) = cache.get(target) {
                let devel = devel.iter().any(|d| *d == pkg.name);

                if alpm::Version::new(&*pkg.version) > local_pkg.version() || devel {
                    aur_ret = 0;

                    let version = if devel {
                        "latest-commit"
                    } else {
                        pkg.version.as_str()
                    };

                    print_upgrade(config, &pkg.name, local_pkg.version().as_str(), version);
                }
            }
        }
    }

    if repo_ret != 0 && aur_ret != 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}

fn print_upgrade(config: &Config, name: &str, local_ver: &str, new_ver: &str) {
    let bold = config.color.bold;
    let upgrade = config.color.upgrade;

    if config.args.has_arg("q", "quiet") {
        println!("{}", name);
    } else {
        print!(
            "{} {} -> {}",
            bold.paint(name),
            upgrade.paint(local_ver),
            upgrade.paint(new_ver)
        );
        if config.alpm.localdb().pkg(name).unwrap().should_ignore() {
            print!("{}", tr!(" [ignored]"));
        }
        println!();
    }
}
