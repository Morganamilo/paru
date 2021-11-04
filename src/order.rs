use crate::config::Config;
use crate::install::flags;
use anyhow::Result;
use aur_depends::{Actions, Conflict, Package, Resolver};
use log::debug;
use std::collections::HashSet;

pub async fn order(config: &mut Config) -> Result<i32> {
    let mut cache = HashSet::new();
    let flags = flags(config);

    config.alpm.take_raw_question_cb();
    let resolver = Resolver::new(&config.alpm, &mut cache, &config.raur, flags);
    let mut actions = resolver.resolve_targets(&config.targets).await?;
    debug!("{:#?}", actions);

    let conflicts = actions.calculate_conflicts(true);
    let inner_conflicts = actions.calculate_inner_conflicts(true);

    print_missing(&actions);
    print_conflicting(conflicts, "LOCAL");
    print_conflicting(inner_conflicts, "INNER");
    print_install(&actions);
    print_build(&mut actions);

    Ok(!actions.missing.is_empty() as i32)
}

fn print_install(actions: &Actions) {
    for pk in &actions.install {
        println!(
            "REPO {} {} {}",
            get_pkg_type(pk),
            pk.pkg.db().unwrap().name(),
            pk.pkg.name()
        );
    }
}

fn print_build(actions: &mut Actions) {
    for build in &mut actions.build {
        let base = build.package_base();

        for b in &build.pkgs {
            println!("AUR {} {} {}", get_pkg_type(b), base, b.pkg.name);
        }
    }
}

fn print_missing(actions: &Actions) {
    for pk in &actions.missing {
        println!("MISSING {} {}", pk.dep, pk.stack.join(" "));
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
