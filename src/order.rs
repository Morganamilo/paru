use crate::config::{Config, Mode};
use crate::install::{flags, read_srcinfos};
use anyhow::Result;
use aur_depends::{Actions, Conflict, Package, Resolver};
use log::debug;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub async fn order(config: &mut Config) -> Result<i32> {
    let mut cache = HashSet::new();
    let flags = flags(config);

    let mut repos = Vec::new();
    let mut custom_paths = HashMap::new();
    let quiet = config.quiet;

    if config.mode != Mode::Repo {
        for repo in &config.custom_repos {
            let (c, p) = read_srcinfos(
                config,
                &repo.name,
                config.fetch.clone_dir.join("repo").join(&repo.name),
                true,
            )?;
            custom_paths.extend(c);
            repos.push(aur_depends::Repo {
                name: repo.name.clone(),
                pkgs: p,
            });
        }
    }

    config.alpm.take_raw_question_cb();
    let resolver = Resolver::new(&config.alpm, &mut cache, &config.raur, flags).repos(repos);
    let mut actions = resolver.resolve_targets(&config.targets).await?;
    debug!("{:#?}", actions);

    let conflicts = actions.calculate_conflicts(true);
    let inner_conflicts = actions.calculate_inner_conflicts(true);

    if !quiet {
        print_missing(&actions);
        print_conflicting(conflicts, "LOCAL");
        print_conflicting(inner_conflicts, "INNER");
    }
    print_install(&actions, quiet);
    print_build(&mut actions, quiet, &custom_paths);

    Ok(!actions.missing.is_empty() as i32)
}

fn print_install(actions: &Actions, quiet: bool) {
    for pk in &actions.install {
        if quiet {
            println!("{}", pk.pkg.name())
        } else {
            println!(
                "REPO {} {} {}",
                get_pkg_type(pk),
                pk.pkg.db().unwrap().name(),
                pk.pkg.name()
            );
        }
    }
}

fn print_build(actions: &mut Actions, quiet: bool, paths: &HashMap<(String, String), PathBuf>) {
    for build in &actions.build {
        let base = build.package_base();

        match build {
            aur_depends::Base::Aur(a) => {
                for pkg in &a.pkgs {
                    if quiet {
                        println!("{}", pkg.pkg.name);
                    } else {
                        println!("AUR {} {} {}", get_pkg_type(pkg), base, pkg.pkg.name);
                    }
                }
            }
            aur_depends::Base::Custom(c) => {
                for pkg in &c.pkgs {
                    if quiet {
                        println!("{}", pkg.pkg.pkgname);
                    } else {
                        let path = paths
                            .get(&(c.repo.clone(), c.package_base().to_string()))
                            .unwrap();
                        println!(
                            "SRCINFO {} {} {} {} {}",
                            get_pkg_type(pkg),
                            path.display(),
                            c.repo,
                            base,
                            pkg.pkg.pkgname
                        );
                    }
                }
            }
        }
    }
}

fn print_missing(actions: &Actions) {
    for pk in &actions.missing {
        println!("MISSING {}", pk.dep);
        for pk in &pk.stack {
            println!(" {}", pk.pkg);
        }
    }
}

fn print_conflicting(conflicts: Vec<Conflict>, type_str: &str) {
    for conf in conflicts {
        for conflicting in conf.conflicting {
            print!("CONFLICT {} {} {}", type_str, conf.pkg, conflicting.pkg,);
            if let Some(conflict) = conflicting.conflict {
                print!(" {}", conflict)
            }
            println!();
        }
    }
}

fn get_pkg_type<T>(pk: &Package<T>) -> &'static str {
    if pk.target {
        "TARGET"
    } else if pk.make {
        "MAKE"
    } else {
        "DEP"
    }
}
