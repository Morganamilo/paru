use std::collections::HashSet;

use crate::config::Config;
use crate::devel::devel_updates;
use crate::util::{split_repo_aur_mode, split_repo_aur_pkgs};
use crate::{esprintln, exec, sprintln};

use anyhow::Result;
use raur_ext::RaurExt;

pub fn print_upgrade_list(config: &mut Config) -> Result<i32> {
    let db = config.alpm.localdb();
    let args = &config.args;

    if args.has_arg("n", "native") {
        config.mode = "repo".into();
    } else if args.has_arg("m", "foreign") {
        config.mode = "aur".into();
    }

    let targets: Vec<_> = if config.targets.is_empty() {
        let all_pkgs = db.pkgs().iter().map(|p| p.name()).collect::<Vec<_>>();
        if config.mode == "aur" {
            split_repo_aur_pkgs(config, &all_pkgs).1
        } else if config.mode == "repo" {
            split_repo_aur_pkgs(config, &all_pkgs).0
        } else {
            all_pkgs
        }
    } else {
        config.targets.iter().map(|s| s.as_str()).collect()
    };

    let (repo, aur) = split_repo_aur_mode(config, &targets);
    let mut repo_ret = 0;
    let mut aur_ret = 0;

    if !repo.is_empty() {
        let mut args = config.pacman_args();
        args.targets = repo.into_iter().collect();
        repo_ret = exec::pacman(config, &args)?.code();
    }

    if !aur.is_empty() {
        let bold = config.color.bold;
        let error = config.color.error;
        let upgrade = config.color.upgrade;

        let mut devel = Vec::new();
        if config.devel {
            devel.extend(devel_updates(config)?);
        }

        for &pkg in &aur {
            if db.pkg(pkg).is_err() {
                esprintln!("{} package '{}' was not found", error.paint("error:"), pkg);
            }
        }

        let mut args = config.pacman_args();
        args.remove("u").remove("upgrades").arg("q");
        args.targets = aur.into_iter().collect();
        let output = exec::pacman_output(config, &args)?;
        let aur = String::from_utf8(output.stdout)?;
        let aur = aur.trim().lines().collect::<Vec<_>>();

        let mut cache = HashSet::new();
        config.raur.cache_info(&mut cache, &aur)?;

        aur_ret = 1;

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
                        sprintln!("{}", pkg.name);
                    } else {
                        sprintln!(
                            "{} {} -> {}",
                            bold.paint(&pkg.name),
                            upgrade.paint(local_pkg.version().as_str()),
                            upgrade.paint(version)
                        );
                    }
                }
            }
        }
    }

    Ok(repo_ret + aur_ret)
}
