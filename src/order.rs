use crate::config::Config;
use crate::resolver::flags;
use anyhow::Result;
use aur_depends::{Actions, Conflict, Package, Resolver};
use log::debug;
use std::collections::HashSet;

pub async fn order(config: &mut Config) -> Result<i32> {
    let mut cache = HashSet::new();
    let flags = flags(config);

    let quiet = config.quiet;

    let repos = config.pkgbuild_repos.clone();
    let repos = repos.aur_depends_repo(config);
    config.alpm.take_raw_question_cb();
    let resolver =
        Resolver::new(&config.alpm, &mut cache, &config.raur, flags).pkgbuild_repos(repos);
    let mut actions = resolver.resolve_targets(&config.targets).await?;
    debug!("{:#?}", actions);

    if !quiet {
        let conflicts = actions.calculate_conflicts(true);
        let inner_conflicts = actions.calculate_inner_conflicts(true);
        print_missing(&actions);
        print_conflicting(conflicts, "LOCAL");
        print_conflicting(inner_conflicts, "INNER");
    }
    print_install(&actions, quiet);
    print_build(config, &mut actions, quiet);

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

fn print_build(config: &Config, actions: &mut Actions, quiet: bool) {
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
            aur_depends::Base::Pkgbuild(c) => {
                for pkg in &c.pkgs {
                    if quiet {
                        println!("{}", pkg.pkg.pkgname);
                    } else {
                        // TODO
                        let path = &config
                            .pkgbuild_repos
                            .repo(&c.repo)
                            .unwrap()
                            .base(config, c.package_base())
                            .unwrap()
                            .path;
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
        print!("MISSING {}", pk.dep);
        for pk in &pk.stack {
            print!(" {}", pk.pkg);
        }
        println!();
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
