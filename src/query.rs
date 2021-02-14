use std::collections::HashSet;

use crate::config::{Config, LocalRepos};
use crate::devel::{filter_devel_updates, possible_devel_updates};
use crate::util::split_repo_aur_pkgs;
use crate::{exec, repo};

use alpm_utils::DbListExt;
use anyhow::Result;
use futures::try_join;
use raur::{Cache, Raur};

pub async fn print_upgrade_list(config: &mut Config) -> Result<i32> {
    let mut cache = HashSet::new();
    let db = config.alpm.localdb();
    let args = &config.args;

    if args.has_arg("n", "native") {
        config.mode = "repo".into();
    } else if args.has_arg("m", "foreign") {
        config.mode = "aur".into();
    }

    let targets: Vec<_> = if config.targets.is_empty() {
        db.pkgs().iter().map(|p| p.name()).collect::<Vec<_>>()
    } else {
        config.targets.iter().map(|s| s.as_str()).collect()
    };

    let (mut repo, mut aur);

    if config.repos != LocalRepos::None {
        let aur_repos = repo::configured_local_repos(config);
        aur = Vec::new();
        repo = Vec::new();

        for pkg in targets {
            if let Ok(p) = config.alpm.syncdbs().pkg(pkg) {
                if let Some(db) = p.db() {
                    if aur_repos.iter().any(|repo| repo == &db.name()) {
                        aur.push(pkg);
                    } else {
                        repo.push(pkg);
                    }
                }
            }
        }
    } else {
        let (r, a) = split_repo_aur_pkgs(config, &targets);
        repo = r;
        aur = a;
    }

    let mut repo_ret = 1;
    let mut aur_ret = 1;

    if !repo.is_empty() && config.mode != "aur" {
        let mut args = config.pacman_args();
        args.targets = repo.into_iter().collect();
        repo_ret = exec::pacman(config, &args)?.code();
    }

    if !aur.is_empty() && config.mode != "repo" {
        let bold = config.color.bold;
        let error = config.color.error;
        let upgrade = config.color.upgrade;

        for &pkg in &aur {
            if db.pkg(pkg).is_err() {
                eprintln!("{} package '{}' was not found", error.paint("error:"), pkg);
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

        for target in aur {
            if let Some(pkg) = cache.get(target) {
                let local_pkg = db.pkg(target).unwrap();
                let devel = devel.iter().any(|d| *d == pkg.name);

                if alpm::Version::new(&*pkg.version) > local_pkg.version() || devel {
                    aur_ret = 0;

                    let version = if devel {
                        "latest-commit"
                    } else {
                        pkg.version.as_str()
                    };

                    if config.args.has_arg("q", "quiet") {
                        println!("{}", pkg.name);
                    } else {
                        print!(
                            "{} {} -> {}",
                            bold.paint(&pkg.name),
                            upgrade.paint(local_pkg.version().as_str()),
                            upgrade.paint(version)
                        );
                        if config.alpm.localdb().pkg(target).unwrap().should_ignore() {
                            print!(" [ignored]");
                        }
                        println!();
                    }
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
