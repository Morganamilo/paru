use std::collections::HashSet;

use crate::config::{Config, Mode};
use crate::devel::{filter_devel_updates, possible_devel_updates};
use crate::exec;
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
        config.mode = Mode::REPO;
    } else if args.has_arg("m", "foreign") {
        config.mode = Mode::AUR | Mode::PKGBUILD;
    }

    let targets: Vec<_> = if config.targets.is_empty() {
        db.pkgs().iter().map(|p| p.name()).collect::<Vec<_>>()
    } else {
        config.targets.iter().map(|s| s.as_str()).collect()
    };

    let (repo, aur) = split_repo_aur_pkgs(config, &targets);

    let mut repo_ret = 1;
    let mut aur_ret = 1;

    if !repo.is_empty() && config.mode.repo() {
        let mut args = config.pacman_args();
        args.targets = repo.into_iter().collect();
        repo_ret = exec::pacman(config, &args)?.code();
    }

    if !aur.is_empty() && (config.mode.pkgbuild() || config.mode.aur()) {
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
        args.remove("u")
            .remove("upgrades")
            .remove("i")
            .remove("info")
            .arg("q");
        args.targets = aur.into_iter().collect();
        let output = exec::pacman_output(config, &args)?;
        let aur = String::from_utf8(output.stdout)?;

        let mut aur = aur.trim().lines().collect::<Vec<_>>();

        if config.mode.pkgbuild() {
            aur.retain(|&target| {
                if !config.mode.aur() {
                    return false;
                }
                if !config.mode.pkgbuild() {
                    return true;
                }
                let local_pkg = db.pkg(target).unwrap();

                if let Some((base, _pkg)) = config.pkgbuild_repos.pkg(config, target)
                    && alpm::Version::new(&*base.srcinfo.version()) > local_pkg.version()
                {
                    print_upgrade(
                        config,
                        target,
                        local_pkg.version().as_str(),
                        &base.srcinfo.version(),
                    );
                    return false;
                }
                true
            });
        }

        if config.mode.aur() {
            async fn devel_up(config: &Config) -> Result<Vec<String>> {
                if config.devel {
                    let updates = possible_devel_updates(config).await?;
                    Ok(updates)
                } else {
                    Ok(Vec::new())
                }
            }

            async fn aur_up(config: &Config, cache: &mut Cache, pkgs: &[&str]) -> Result<()> {
                config
                    .raur
                    .as_ref()
                    .unwrap()
                    .cache_info(cache, pkgs)
                    .await?;
                Ok(())
            }

            let (_, devel) = try_join!(aur_up(config, &mut cache, &aur), devel_up(config))?;
            let devel = filter_devel_updates(config, &mut cache, &devel).await?;

            for target in aur {
                let local_pkg = db.pkg(target).unwrap();
                if let Some(pkg) = cache.get(target) {
                    let devel = devel.iter().any(|d| d.pkg == pkg.name);

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
    }

    if repo_ret != 0 && aur_ret != 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}

fn print_upgrade(config: &Config, name: &str, local_ver: &str, new_ver: &str) {
    if config.args.has_arg("q", "quiet") {
        println!("{}", name);
    } else {
        let mut br = 0;
        let mut dots = 0;
        let mut n = 0;
        let mut pkgver_match = false;

        let mut local_ver_iter = local_ver.chars();
        let mut new_ver_iter = new_ver.chars();
        while let Some(local) = local_ver_iter.next() {
            if let Some(new) = new_ver_iter.next() {
                if local != new {
                    break;
                }
                n += local.len_utf8();
                match local {
                    '.' => {
                        dots += 1;
                        br = n;
                    }
                    ':' => {
                        br = n;
                    }
                    '-' => {
                        pkgver_match = true;
                        br = n;
                        break;
                    }
                    _ => {}
                }
            } else {
                break;
            }
        }

        let (common_ver, local_diff) = local_ver.split_at(br);
        let (_, new_diff) = new_ver.split_at(br);

        let bold = config.color.bold;
        let upgrade = if pkgver_match {
            config.color.bold
        } else {
            match dots {
                0 => config.color.upgrade_major,
                1 => config.color.upgrade_minor,
                _ => config.color.upgrade_patch,
            }
        };

        print!(
            "{} {}{} -> {}{}",
            bold.paint(name),
            common_ver,
            upgrade.paint(local_diff),
            common_ver,
            upgrade.paint(new_diff)
        );
        if config.alpm.localdb().pkg(name).unwrap().should_ignore() {
            print!("{}", tr!(" [ignored]"));
        }
        println!();
    }
}
